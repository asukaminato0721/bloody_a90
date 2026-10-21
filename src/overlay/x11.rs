use super::{Place, Spec};
use x11rb::{
    connection::Connection,
    protocol::{
        shape::{ConnectionExt as _, SK, SO},
        xproto::{
            AtomEnum, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, CreateGCAux,
            CreateWindowAux, ImageFormat, PropMode, StackMode, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

pub(crate) struct Window {
    connection: RustConnection,
    window: u32,
    gc: u32,
    root: u32,
    depth: u8,
    dimensions: [u16; 2],
    visible: bool,
    spec: Spec,
}

impl Window {
    pub(super) fn connect(spec: Spec) -> Result<Self, String> {
        let (connection, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let screen = &connection.setup().roots[screen];
        if screen.root_depth != 24 && screen.root_depth != 32 {
            return Err("提示板需要 24 或 32 位 X11 色深".into());
        }
        connection
            .shape_query_version()
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| format!("X11 不支持提示板鼠标穿透：{e}"))?;
        let (root, depth, visual, dimensions) = (
            screen.root,
            screen.root_depth,
            screen.root_visual,
            [screen.width_in_pixels, screen.height_in_pixels],
        );
        let window = connection.generate_id().map_err(|e| e.to_string())?;
        let gc = connection.generate_id().map_err(|e| e.to_string())?;
        connection
            .create_window(
                depth,
                window,
                root,
                0,
                0,
                spec.width as u16,
                spec.height as u16,
                0,
                WindowClass::INPUT_OUTPUT,
                visual,
                &CreateWindowAux::new()
                    .override_redirect(1)
                    .background_pixel(0x1d1e22),
            )
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
        let result = (|| {
            connection
                .shape_rectangles(
                    SO::SET,
                    SK::INPUT,
                    ClipOrdering::UNSORTED,
                    window,
                    0,
                    0,
                    &[],
                )
                .map_err(|e| e.to_string())?
                .check()
                .map_err(|e| e.to_string())?;
            // WM_HINTS InputHint=false. The window never requests keyboard focus.
            connection
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    AtomEnum::WM_HINTS,
                    AtomEnum::WM_HINTS,
                    &[1, 0, 0, 0, 0, 0, 0, 0, 0],
                )
                .map_err(|e| e.to_string())?;
            connection
                .change_property8(
                    PropMode::REPLACE,
                    window,
                    AtomEnum::WM_NAME,
                    AtomEnum::STRING,
                    spec.id.as_bytes(),
                )
                .map_err(|e| e.to_string())?;
            connection
                .create_gc(gc, window, &CreateGCAux::new().graphics_exposures(0))
                .map_err(|e| e.to_string())?
                .check()
                .map_err(|e| e.to_string())?;
            connection.flush().map_err(|e| e.to_string())
        })();
        if let Err(error) = result {
            let _ = connection.destroy_window(window);
            return Err(error);
        }
        Ok(Self {
            connection,
            window,
            gc,
            root,
            depth,
            dimensions,
            visible: false,
            spec,
        })
    }

    pub(super) fn present(&mut self, pixels: &[u8]) -> Result<bool, String> {
        let Spec { width, height, .. } = self.spec;
        if pixels.len() != self.spec.bytes() {
            return Err("提示板像素长度无效".into());
        }
        if !self.visible {
            let limit = [
                (i32::from(self.dimensions[0]) - width as i32).max(0),
                (i32::from(self.dimensions[1]) - height as i32).max(0),
            ];
            let [x, y] = match self.spec.place {
                Place::Pointer => {
                    let pointer = self
                        .connection
                        .query_pointer(self.root)
                        .map_err(|e| e.to_string())?
                        .reply()
                        .map_err(|e| e.to_string())?;
                    [
                        (i32::from(pointer.root_x) + 24).clamp(0, limit[0]),
                        (i32::from(pointer.root_y) + 24).clamp(0, limit[1]),
                    ]
                }
                // Leave room for a bottom panel. X11 has no exclusive zones.
                Place::BottomRight => [(limit[0] - 24).max(0), (limit[1] - 64).max(0)],
            };
            self.connection
                .configure_window(
                    self.window,
                    &ConfigureWindowAux::new()
                        .x(x)
                        .y(y)
                        .stack_mode(StackMode::ABOVE),
                )
                .map_err(|e| e.to_string())?;
            self.connection
                .map_window(self.window)
                .map_err(|e| e.to_string())?;
            self.visible = true;
        }
        // Rows keep each PutImage request below the server request-size limit.
        for (index, rows) in pixels.chunks((width * 4 * 32) as usize).enumerate() {
            self.connection
                .put_image(
                    ImageFormat::Z_PIXMAP,
                    self.window,
                    self.gc,
                    width as u16,
                    (rows.len() / (width * 4) as usize) as u16,
                    0,
                    (index * 32) as i16,
                    0,
                    self.depth,
                    rows,
                )
                .map_err(|e| e.to_string())?;
        }
        self.connection.flush().map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub(super) fn hide(&mut self) -> Result<(), String> {
        if self.visible {
            self.connection
                .unmap_window(self.window)
                .map_err(|e| e.to_string())?;
            self.visible = false;
        }
        self.connection.flush().map_err(|e| e.to_string())
    }

    pub(super) fn pump(&mut self) -> Result<(), String> {
        while self
            .connection
            .poll_for_event()
            .map_err(|e| e.to_string())?
            .is_some()
        {}
        std::thread::sleep(std::time::Duration::from_millis(16));
        Ok(())
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.connection.free_gc(self.gc);
        let _ = self.connection.destroy_window(self.window);
        let _ = self.connection.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto;
    #[test]
    #[ignore = "requires an isolated X11 server"]
    fn x11_tip_has_empty_input_region_and_keeps_focus() {
        let spec = crate::gesture_tip::SPEC;
        let mut window = Window::connect(spec).unwrap();
        let focus = window
            .connection
            .get_input_focus()
            .unwrap()
            .reply()
            .unwrap()
            .focus;
        let image = crate::gesture_tip::sample();
        window.present(&super::super::bgra(&image)).unwrap();
        window
            .connection
            .get_input_focus()
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(
            window
                .connection
                .get_input_focus()
                .unwrap()
                .reply()
                .unwrap()
                .focus,
            focus
        );
        assert!(
            window
                .connection
                .shape_get_rectangles(window.window, SK::INPUT)
                .unwrap()
                .reply()
                .unwrap()
                .rectangles
                .is_empty()
        );
        assert_eq!(
            window
                .connection
                .get_window_attributes(window.window)
                .unwrap()
                .reply()
                .unwrap()
                .map_state,
            xproto::MapState::VIEWABLE
        );
        let pixels = window
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                window.window,
                0,
                0,
                spec.width as u16,
                spec.height as u16,
                u32::MAX,
            )
            .unwrap()
            .reply()
            .unwrap();
        assert!(pixels.data.windows(3).any(|pixel| pixel == [48, 35, 172]));
        window.hide().unwrap();
        assert_eq!(
            window
                .connection
                .get_window_attributes(window.window)
                .unwrap()
                .reply()
                .unwrap()
                .map_state,
            xproto::MapState::UNMAPPED
        );
    }
}
