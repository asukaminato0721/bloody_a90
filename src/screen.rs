//! Logical screen size before any window exists, for sizing the main window.
//! A window created larger than the screen keeps the compositor's size on
//! KWin Wayland; a later resize request is not applied.
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_output, wl_registry},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

/// The largest output in logical points (after scaling), or None when it
/// cannot be read. Only Wayland reports this before a window is mapped.
pub fn logical_size() -> Option<[f32; 2]> {
    std::env::var_os("WAYLAND_DISPLAY")?;
    let connection = Connection::connect_to_env().ok()?;
    let mut queue = connection.new_event_queue();
    connection.display().get_registry(&queue.handle(), ());
    let mut state = State::default();
    queue.roundtrip(&mut state).ok()?;
    let manager = state.manager.clone()?;
    for output in &state.outputs {
        manager.get_xdg_output(output, &queue.handle(), ());
    }
    queue.roundtrip(&mut state).ok()?;
    state
        .sizes
        .into_iter()
        .filter(|[w, h]| *w > 0 && *h > 0)
        .max_by_key(|[w, h]| i64::from(*w) * i64::from(*h))
        .map(|[w, h]| [w as f32, h as f32])
}

#[derive(Default)]
struct State {
    manager: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    outputs: Vec<wl_output::WlOutput>,
    sizes: Vec<[i32; 2]>,
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
                "wl_output" => state
                    .outputs
                    .push(registry.bind(name, version.min(2), qh, ())),
                "zxdg_output_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(3), qh, ()))
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::LogicalSize { width, height } = event {
            state.sizes.push([width, height]);
        }
    }
}

delegate_noop!(State: ignore wl_output::WlOutput);
delegate_noop!(State: ignore zxdg_output_manager_v1::ZxdgOutputManagerV1);
