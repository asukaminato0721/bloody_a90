use super::{Backend, Operation};
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask},
    rust_connection::RustConnection,
};

pub(super) struct X11 {
    connection: RustConnection,
    root: u32,
    wm_state: u32,
    allowed: u32,
    minimize: u32,
    maximize_h: u32,
    maximize_v: u32,
    change_state: u32,
    net_state: u32,
    state_h: u32,
    state_v: u32,
}

impl X11 {
    pub(super) fn connect() -> Result<Self, String> {
        let (connection, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = connection.setup().roots[screen].root;
        let atom = |name: &str| {
            connection
                .intern_atom(false, name.as_bytes())
                .map_err(|e| e.to_string())?
                .reply()
                .map(|r| r.atom)
                .map_err(|e| e.to_string())
        };
        let check = atom("_NET_SUPPORTING_WM_CHECK")?;
        let supporting = connection
            .get_property(false, root, check, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .value32()
            .and_then(|mut v| v.next())
            .filter(|id| *id != 0)
            .ok_or("窗口动作需要支持 EWMH 的 X11 窗口管理器")?;
        let valid = connection
            .get_property(false, supporting, check, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .value32()
            .and_then(|mut v| v.next())
            == Some(supporting);
        if !valid {
            return Err("X11 窗口管理器标识无效".into());
        }
        Ok(Self {
            wm_state: atom("WM_STATE")?,
            allowed: atom("_NET_WM_ALLOWED_ACTIONS")?,
            minimize: atom("_NET_WM_ACTION_MINIMIZE")?,
            maximize_h: atom("_NET_WM_ACTION_MAXIMIZE_HORZ")?,
            maximize_v: atom("_NET_WM_ACTION_MAXIMIZE_VERT")?,
            change_state: atom("WM_CHANGE_STATE")?,
            net_state: atom("_NET_WM_STATE")?,
            state_h: atom("_NET_WM_STATE_MAXIMIZED_HORZ")?,
            state_v: atom("_NET_WM_STATE_MAXIMIZED_VERT")?,
            connection,
            root,
        })
    }

    fn managed(&self, window: u32) -> Result<bool, String> {
        Ok(self
            .connection
            .get_property(false, window, self.wm_state, self.wm_state, 0, 2)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .value32()
            .is_some_and(|mut v| v.next().is_some()))
    }

    fn under_pointer(&self) -> Result<Option<u32>, String> {
        let mut path = Vec::new();
        let mut current = self.root;
        for _ in 0..128 {
            let pointer = self
                .connection
                .query_pointer(current)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            if !pointer.same_screen || pointer.child == 0 {
                break;
            }
            current = pointer.child;
            path.push(current);
        }
        for &window in path.iter().rev() {
            if self.managed(window)? {
                return Ok(Some(window));
            }
        }
        // A window-manager decoration can be a sibling of the client. Search
        // only inside this pointer-selected frame, never across root windows.
        let Some(frame) = path.first() else {
            return Ok(None);
        };
        let mut pending = vec![*frame];
        for _ in 0..128 {
            let Some(window) = pending.pop() else {
                break;
            };
            if self.managed(window)? {
                return Ok(Some(window));
            }
            let children = self
                .connection
                .query_tree(window)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?
                .children;
            pending.extend(
                children
                    .into_iter()
                    .take(128usize.saturating_sub(pending.len())),
            );
        }
        Ok(None)
    }
}

impl Backend for X11 {
    fn execute(&mut self, operation: Operation) -> Result<(), String> {
        if operation == Operation::MinimizeAll {
            return Err("全部最小化目前需要 KDE Wayland 桌面".into());
        }
        let Some(window) = self.under_pointer()? else {
            return Ok(());
        };
        let allowed = self
            .connection
            .get_property(false, window, self.allowed, AtomEnum::ATOM, 0, 128)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let allowed: Vec<_> = allowed.value32().into_iter().flatten().collect();
        let (message, data) = match operation {
            Operation::Minimize if allowed.contains(&self.minimize) => {
                (self.change_state, [3, 0, 0, 0, 0])
            }
            Operation::Maximize
                if allowed.contains(&self.maximize_h) && allowed.contains(&self.maximize_v) =>
            {
                (self.net_state, [1, self.state_v, self.state_h, 2, 0])
            }
            _ => return Ok(()),
        };
        let event = ClientMessageEvent::new(32, window, message, data);
        self.connection
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
        self.connection.flush().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::{
        protocol::{
            Event,
            xproto::{CreateWindowAux, InputFocus, PropMode, WindowClass},
        },
        wrapper::ConnectionExt as _,
    };

    #[test]
    #[ignore = "requires an isolated X11 server; installs only a test window-manager marker"]
    fn pointer_client_decoration_capabilities_and_requests() {
        let (wm, screen) = x11rb::connect(None).unwrap();
        let root = wm.setup().roots[screen].root;
        let atom = |name: &str| {
            wm.intern_atom(false, name.as_bytes())
                .unwrap()
                .reply()
                .unwrap()
                .atom
        };
        let check = atom("_NET_SUPPORTING_WM_CHECK");
        let previous = wm
            .get_property(false, root, check, AtomEnum::WINDOW, 0, 1)
            .unwrap()
            .reply()
            .unwrap();
        assert!(
            previous.value.is_empty(),
            "run this test on an isolated Xvfb server"
        );
        let window = |parent, x, y, width, height| {
            let id = wm.generate_id().unwrap();
            wm.create_window(
                0,
                id,
                parent,
                x,
                y,
                width,
                height,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
            id
        };
        let marker = window(root, 0, 0, 1, 1);
        wm.change_property32(PropMode::REPLACE, root, check, AtomEnum::WINDOW, &[marker])
            .unwrap();
        wm.change_property32(
            PropMode::REPLACE,
            marker,
            check,
            AtomEnum::WINDOW,
            &[marker],
        )
        .unwrap();
        let active = window(root, 450, 40, 200, 200);
        let frame = window(root, 100, 100, 250, 250);
        let client = window(frame, 10, 30, 230, 210);
        let child = window(client, 10, 10, 150, 150);
        let decoration = window(frame, 0, 0, 250, 25);
        let state = atom("WM_STATE");
        for id in [active, client] {
            wm.change_property32(PropMode::REPLACE, id, state, state, &[1, 0])
                .unwrap();
        }
        let active_atom = atom("_NET_ACTIVE_WINDOW");
        wm.change_property32(
            PropMode::REPLACE,
            root,
            active_atom,
            AtomEnum::WINDOW,
            &[active],
        )
        .unwrap();
        let allowed = atom("_NET_WM_ALLOWED_ACTIONS");
        let allowed_values = [
            atom("_NET_WM_ACTION_MINIMIZE"),
            atom("_NET_WM_ACTION_MAXIMIZE_HORZ"),
            atom("_NET_WM_ACTION_MAXIMIZE_VERT"),
        ];
        wm.change_property32(
            PropMode::REPLACE,
            client,
            allowed,
            AtomEnum::ATOM,
            &allowed_values,
        )
        .unwrap();
        for id in [active, frame, client, child, decoration] {
            wm.map_window(id).unwrap();
        }
        wm.set_input_focus(InputFocus::POINTER_ROOT, active, x11rb::CURRENT_TIME)
            .unwrap();
        wm.change_window_attributes(
            root,
            &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                .event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
        )
        .unwrap()
        .check()
        .unwrap();
        wm.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, 150, 160)
            .unwrap();
        wm.get_input_focus().unwrap().reply().unwrap();
        while wm.poll_for_event().unwrap().is_some() {}
        let mut backend = X11::connect().unwrap();
        assert_eq!(backend.under_pointer().unwrap(), Some(client));
        backend.execute(Operation::Minimize).unwrap();
        let Event::ClientMessage(message) = wm.wait_for_event().unwrap() else {
            panic!("expected WM_CHANGE_STATE")
        };
        assert_eq!(message.window, client);
        assert_eq!(message.type_, atom("WM_CHANGE_STATE"));
        assert_eq!(message.data.as_data32(), [3, 0, 0, 0, 0]);
        // A title-bar child has no WM_STATE. Resolve its sibling client.
        wm.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, 150, 110)
            .unwrap();
        wm.get_input_focus().unwrap().reply().unwrap();
        assert_eq!(backend.under_pointer().unwrap(), Some(client));
        backend.execute(Operation::Maximize).unwrap();
        let Event::ClientMessage(message) = wm.wait_for_event().unwrap() else {
            panic!("expected _NET_WM_STATE")
        };
        assert_eq!(message.window, client);
        assert_eq!(message.type_, atom("_NET_WM_STATE"));
        assert_eq!(
            message.data.as_data32(),
            [
                1,
                atom("_NET_WM_STATE_MAXIMIZED_VERT"),
                atom("_NET_WM_STATE_MAXIMIZED_HORZ"),
                2,
                0
            ]
        );
        assert_eq!(wm.get_input_focus().unwrap().reply().unwrap().focus, active);
        // An unsupported operation must not fall through to the active window.
        wm.change_property32(PropMode::REPLACE, client, allowed, AtomEnum::ATOM, &[])
            .unwrap();
        wm.get_input_focus().unwrap().reply().unwrap();
        backend.execute(Operation::Minimize).unwrap();
        backend.execute(Operation::Maximize).unwrap();
        assert!(wm.poll_for_event().unwrap().is_none());
        wm.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, 5, 5)
            .unwrap();
        wm.get_input_focus().unwrap().reply().unwrap();
        assert_eq!(backend.under_pointer().unwrap(), None);
        backend.execute(Operation::Minimize).unwrap();
        assert!(wm.poll_for_event().unwrap().is_none());
        wm.delete_property(root, check).unwrap();
        wm.delete_property(root, active_atom).unwrap();
        for id in [frame, active, marker] {
            wm.destroy_window(id).unwrap();
        }
        wm.flush().unwrap();
    }
}
