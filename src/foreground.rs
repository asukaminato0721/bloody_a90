//! Foreground application identifiers from KWin Wayland or an X11 window manager.
//! This module only reads window metadata. It does not change window focus.
use std::{collections::HashMap, os::fd::AsRawFd};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, protocol::wl_registry};
use wayland_protocols_plasma::plasma_window_management::client::{
    org_kde_plasma_window as window, org_kde_plasma_window_management as management,
};
use x11rb::{
    connection::Connection as _,
    protocol::xproto::{AtomEnum, ConnectionExt as _},
    rust_connection::RustConnection,
};

pub enum Monitor {
    Plasma(Box<Plasma>),
    X11(Box<X11>),
}

impl Monitor {
    pub fn connect() -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            Plasma::connect().map(|p| Self::Plasma(Box::new(p)))
        } else {
            X11::connect().map(|x| Self::X11(Box::new(x)))
        }
    }

    pub fn current(&mut self) -> Result<Option<String>, String> {
        match self {
            Self::Plasma(plasma) => plasma.current(),
            Self::X11(x11) => x11.current(),
        }
    }
}

#[derive(Default)]
struct Window {
    app: String,
    ready: bool,
}

#[derive(Default)]
struct State {
    manager: Option<management::OrgKdePlasmaWindowManagement>,
    windows: HashMap<wayland_client::backend::ObjectId, Window>,
    active: Option<wayland_client::backend::ObjectId>,
}

pub struct Plasma {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}

impl Plasma {
    fn connect() -> Result<Self, String> {
        let connection =
            Connection::connect_to_env().map_err(|e| format!("无法连接 Wayland：{e}"))?;
        let mut queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        let mut state = State::default();
        // Registry, window list, then each window's initial metadata.
        for _ in 0..3 {
            queue
                .roundtrip(&mut state)
                .map_err(|e| format!("无法读取 KWin 窗口：{e}"))?;
        }
        if state.manager.is_none() {
            return Err("当前 Wayland 桌面不提供 KWin 窗口接口。应用关联不可用。".into());
        }
        Ok(Self {
            connection,
            queue,
            state,
        })
    }

    fn current(&mut self) -> Result<Option<String>, String> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| e.to_string())?;
        self.connection.flush().map_err(|e| e.to_string())?;
        if let Some(guard) = self.queue.prepare_read() {
            let mut fd = libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // Zero timeout. Only read when the compositor socket is ready.
            let ready = unsafe { libc::poll(&mut fd, 1, 0) };
            if ready < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err("KWin 窗口连接已关闭".into());
            }
            if fd.revents & libc::POLLIN != 0 {
                guard.read().map_err(|e| e.to_string())?;
            }
        }
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| e.to_string())?;
        // Dispatch can create requests for newly announced windows.
        self.connection.flush().map_err(|e| e.to_string())?;
        Ok(self
            .state
            .active
            .as_ref()
            .and_then(|id| self.state.windows.get(id))
            .filter(|w| w.ready && !w.app.is_empty())
            .map(|w| w.app.clone()))
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == "org_kde_plasma_window_management"
            && version >= 13
        {
            // Version 13 supplies UUIDs and initial-state events. The client
            // never requests activation, geometry, desktop or state changes.
            state.manager = Some(registry.bind(name, 13, qh, ()));
        }
    }
}

impl Dispatch<management::OrgKdePlasmaWindowManagement, ()> for State {
    fn event(
        state: &mut Self,
        manager: &management::OrgKdePlasmaWindowManagement,
        event: management::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let management::Event::WindowWithUuid { uuid, .. } = event {
            let window = manager.get_window_by_uuid(uuid, qh, ());
            state.windows.insert(window.id(), Window::default());
        }
    }
}

impl Dispatch<window::OrgKdePlasmaWindow, ()> for State {
    fn event(
        state: &mut Self,
        window: &window::OrgKdePlasmaWindow,
        event: window::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = window.id();
        match event {
            window::Event::AppIdChanged { app_id } => {
                if let Some(w) = state.windows.get_mut(&id) {
                    w.app = app_id;
                }
            }
            window::Event::StateChanged { flags } => {
                if flags & 1 != 0 {
                    state.active = Some(id);
                } else if state.active.as_ref() == Some(&id) {
                    state.active = None;
                }
            }
            window::Event::InitialState => {
                if let Some(w) = state.windows.get_mut(&id) {
                    w.ready = true;
                }
            }
            window::Event::Unmapped => {
                state.windows.remove(&id);
                if state.active.as_ref() == Some(&id) {
                    state.active = None;
                }
                window.destroy();
            }
            _ => {}
        }
    }
}

pub struct X11 {
    connection: RustConnection,
    root: u32,
    active: u32,
}

impl X11 {
    fn connect() -> Result<Self, String> {
        let (connection, screen) =
            x11rb::connect(None).map_err(|e| format!("无法连接 X11：{e}"))?;
        let root = connection.setup().roots[screen].root;
        let active = connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom;
        Ok(Self {
            connection,
            root,
            active,
        })
    }

    fn current(&self) -> Result<Option<String>, String> {
        let reply = self
            .connection
            .get_property(false, self.root, self.active, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let Some(window) = reply
            .value32()
            .and_then(|mut values| values.next())
            .filter(|w| *w != 0)
        else {
            return Ok(None);
        };
        let reply = self
            .connection
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
            .map_err(|e| e.to_string())?
            .reply();
        let reply = match reply {
            Ok(reply) => reply,
            // A window can close between the two property requests.
            Err(x11rb::errors::ReplyError::X11Error(error))
                if error.error_kind == x11rb::protocol::ErrorKind::Window =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error.to_string()),
        };
        // ICCCM WM_CLASS contains instance\0class\0. Use the application class.
        let mut names = reply.value.split(|b| *b == 0).filter(|s| !s.is_empty());
        let instance = names.next();
        Ok(names
            .next()
            .or(instance)
            .map(|v| String::from_utf8_lossy(v).into_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::{
        protocol::xproto::{CreateWindowAux, PropMode, WindowClass},
        wrapper::ConnectionExt as _,
    };

    #[test]
    #[ignore = "requires an isolated X11 server and A90_TEST_X11=1"]
    fn x11_active_window_class() {
        assert_eq!(std::env::var("A90_TEST_X11").as_deref(), Ok("1"));
        let monitor = X11::connect().unwrap();
        let connection = &monitor.connection;
        let window = connection.generate_id().unwrap();
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                monitor.root,
                0,
                0,
                100,
                100,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
        connection
            .change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                b"instance\0TestApplication\0",
            )
            .unwrap()
            .check()
            .unwrap();
        connection
            .change_property32(
                PropMode::REPLACE,
                monitor.root,
                monitor.active,
                AtomEnum::WINDOW,
                &[window],
            )
            .unwrap()
            .check()
            .unwrap();
        assert_eq!(
            monitor.current().unwrap().as_deref(),
            Some("TestApplication")
        );
        connection.destroy_window(window).unwrap().check().unwrap();
        // The root can still refer to a window that the server removed.
        assert_eq!(monitor.current().unwrap(), None);
        connection
            .change_property32(
                PropMode::REPLACE,
                monitor.root,
                monitor.active,
                AtomEnum::WINDOW,
                &[0],
            )
            .unwrap()
            .check()
            .unwrap();
        assert_eq!(monitor.current().unwrap(), None);
    }
}
