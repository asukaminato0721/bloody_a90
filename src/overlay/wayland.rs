use super::{Place, Spec};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    os::fd::{AsFd, AsRawFd, FromRawFd},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    protocol::{wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_surface},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1 as shell, zwlr_layer_surface_v1 as layer,
};

#[derive(Default)]
struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    shell: Option<shell::ZwlrLayerShellV1>,
    generation: u64,
    configured: bool,
    closed: bool,
    buffers: BTreeMap<u64, wl_buffer::WlBuffer>,
}

pub(crate) struct Window {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    surface: Option<(wl_surface::WlSurface, layer::ZwlrLayerSurfaceV1)>,
    serial: u64,
    spec: Spec,
}

impl Window {
    pub(super) fn connect(spec: Spec) -> Result<Self, String> {
        let connection =
            Connection::connect_to_env().map_err(|e| format!("无法连接{}：{e}", spec.name))?;
        let mut queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        let mut state = State::default();
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        if state.compositor.is_none() || state.shm.is_none() || state.shell.is_none() {
            return Err(format!(
                "当前 Wayland 桌面不提供{}所需的叠加层。请关闭该选项后重试。",
                spec.name
            ));
        }
        Ok(Self {
            connection,
            queue,
            state,
            surface: None,
            serial: 0,
            spec,
        })
    }

    pub(super) fn present(&mut self, pixels: &[u8]) -> Result<bool, String> {
        let Spec { width, height, .. } = self.spec;
        if pixels.len() != self.spec.bytes() {
            return Err("提示板像素长度无效".into());
        }
        if self.surface.is_none() {
            self.state.generation = self.state.generation.wrapping_add(1);
            self.state.configured = false;
            self.state.closed = false;
            let qh = self.queue.handle();
            let compositor = self.state.compositor.as_ref().unwrap();
            let surface = compositor.create_surface(&qh, ());
            let region = compositor.create_region(&qh, ());
            // An empty input region makes pointer events pass through.
            surface.set_input_region(Some(&region));
            region.destroy();
            let layer = self.state.shell.as_ref().unwrap().get_layer_surface(
                &surface,
                None,
                shell::Layer::Overlay,
                self.spec.id.into(),
                &qh,
                self.state.generation,
            );
            layer.set_size(width, height);
            match self.spec.place {
                Place::Pointer => {
                    layer.set_anchor(layer::Anchor::Top | layer::Anchor::Right);
                    layer.set_margin(28, 28, 0, 0);
                }
                Place::BottomRight => {
                    layer.set_anchor(layer::Anchor::Bottom | layer::Anchor::Right);
                    layer.set_margin(0, 24, 24, 0);
                }
            }
            layer.set_exclusive_zone(0);
            layer.set_keyboard_interactivity(layer::KeyboardInteractivity::None);
            surface.commit(); // Wait for configure before attaching a buffer.
            self.surface = Some((surface, layer));
            self.connection.flush().map_err(|e| e.to_string())?;
            return Ok(false);
        }
        if self.state.closed {
            return Err(format!("桌面关闭了{}", self.spec.name));
        }
        if !self.state.configured || self.state.buffers.len() >= 3 {
            return Ok(false);
        }
        let fd = unsafe { libc::memfd_create(c"a90-overlay".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.set_len(pixels.len() as u64)
            .and_then(|()| file.write_all(pixels))
            .map_err(|e| e.to_string())?;
        let qh = self.queue.handle();
        let pool = self.state.shm.as_ref().unwrap().create_pool(
            file.as_fd(),
            pixels.len() as i32,
            &qh,
            (),
        );
        self.serial = self.serial.wrapping_add(1);
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            (width * 4) as i32,
            wl_shm::Format::Argb8888,
            &qh,
            self.serial,
        );
        pool.destroy();
        self.state.buffers.insert(self.serial, buffer.clone());
        let surface = &self.surface.as_ref().unwrap().0;
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, width as i32, height as i32);
        surface.commit();
        self.connection.flush().map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub(super) fn hide(&mut self) -> Result<(), String> {
        if let Some((surface, layer)) = self.surface.take() {
            layer.destroy();
            surface.destroy();
        }
        self.state.configured = false;
        for (_, buffer) in std::mem::take(&mut self.state.buffers) {
            buffer.destroy();
        }
        self.connection.flush().map_err(|e| e.to_string())
    }

    pub(super) fn pump(&mut self) -> Result<(), String> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| e.to_string())?;
        self.connection.flush().map_err(|e| e.to_string())?;
        if let Some(guard) = self.queue.prepare_read() {
            let mut descriptor = libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut descriptor, 1, 16) };
            if ready < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(format!("{}连接已关闭", self.spec.name));
            }
            if descriptor.revents & libc::POLLIN != 0 {
                guard.read().map_err(|e| e.to_string())?;
            }
        }
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.hide();
        for (_, buffer) in std::mem::take(&mut self.state.buffers) {
            buffer.destroy();
        }
        let _ = self.connection.flush();
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
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()))
                }
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "zwlr_layer_shell_v1" => {
                    state.shell = Some(registry.bind(name, version.min(4), qh, ()))
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<layer::ZwlrLayerSurfaceV1, u64> for State {
    fn event(
        state: &mut Self,
        surface: &layer::ZwlrLayerSurfaceV1,
        event: layer::Event,
        generation: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if *generation != state.generation {
            return;
        }
        match event {
            layer::Event::Configure { serial, .. } => {
                surface.ack_configure(serial);
                state.configured = true;
            }
            layer::Event::Closed => state.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, u64> for State {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        serial: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release)
            && let Some(buffer) = state.buffers.remove(serial)
        {
            buffer.destroy();
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_region::WlRegion);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore shell::ZwlrLayerShellV1);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a Wayland desktop with layer-shell; briefly shows a click-through tip"]
    fn wayland_tip_draws_and_hides() {
        let mut window = Window::connect(crate::gesture_tip::SPEC).unwrap();
        let pixels = vec![0x77; window.spec.bytes()];
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !window.present(&pixels).unwrap() {
            assert!(std::time::Instant::now() < until);
            window.pump().unwrap();
        }
        assert!(window.state.configured);
        for _ in 0..4 {
            window.pump().unwrap();
        }
        window.hide().unwrap();
        assert!(window.surface.is_none());
        // A fresh role must be configured when the next stroke begins.
        while !window.present(&pixels).unwrap() {
            assert!(std::time::Instant::now() < until);
            window.pump().unwrap();
        }
        window.hide().unwrap();
    }
}
