use super::{Backend, Operation};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zbus::{
    blocking::{Connection, Proxy},
    message::Header,
};

const PATH: &str = "/org/bloody/A90/WindowAction";
const INTERFACE: &str = "org.bloody.A90.WindowAction";

struct Reply {
    owner: String,
    sender: SyncSender<(String, String)>,
}

#[zbus::interface(name = "org.bloody.A90.WindowAction")]
impl Reply {
    fn completed(&self, token: &str, error: &str, #[zbus(header)] header: Header<'_>) {
        if header
            .sender()
            .is_some_and(|sender| sender.as_str() == self.owner)
        {
            let _ = self.sender.try_send((token.into(), error.into()));
        }
    }
}

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}
struct Script {
    bus: Connection,
    plugin: String,
    file: PathBuf,
}
impl Drop for Script {
    fn drop(&mut self) {
        if let Ok(proxy) = Proxy::new(
            &self.bus,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        ) {
            let _: Result<bool, _> = proxy.call("unloadScript", &self.plugin);
        }
        let _ = fs::remove_file(&self.file);
    }
}

pub(super) struct KWin {
    bus: Connection,
    results: Receiver<(String, String)>,
    directory: Directory,
    serial: u64,
}

impl KWin {
    pub(super) fn connect() -> Result<Self, String> {
        let bus = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.method_timeout(Duration::from_secs(2)).build())
            .map_err(|e| format!("无法连接窗口服务：{e}"))?;
        let dbus = Proxy::new(
            &bus,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(|e| e.to_string())?;
        let owner: String = dbus
            .call("GetNameOwner", &"org.kde.KWin")
            .map_err(|e| format!("窗口动作需要 KWin 脚本接口：{e}"))?;
        let (sender, results) = mpsc::sync_channel(16);
        bus.object_server()
            .at(PATH, Reply { owner, sender })
            .map_err(|e| e.to_string())?;
        let name = format!(
            "a90-window-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        let path = std::env::temp_dir().join(name);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|e| e.to_string())?;
        let mut result = Self {
            bus,
            results,
            directory: Directory(path),
            serial: 0,
        };
        // Check the actual JS API and callback path before grabbing input.
        result.run_body("if (typeof workspace.windowAt !== 'function' || !workspace.cursorPos || typeof workspace.windowList !== 'function' || !workspace.currentDesktop || typeof workspace.currentActivity !== 'string') { throw new Error('KWin window API unavailable'); } workspace.windowAt(workspace.cursorPos, 0);")?;
        Ok(result)
    }

    fn run_body(&mut self, body: &str) -> Result<(), String> {
        self.serial = self.serial.checked_add(1).ok_or("窗口动作计数已满")?;
        let token = self.serial.to_string();
        let destination = self
            .bus
            .unique_name()
            .ok_or("窗口动作缺少总线名称")?
            .as_str();
        // Only D-Bus names, an integer token, and constant code enter this script.
        let source = format!(
            "let error = ''; try {{ {body} }} catch (e) {{ error = String(e); }}\ncallDBus('{destination}', '{PATH}', '{INTERFACE}', 'Completed', '{token}', error);\n"
        );
        let file = self.directory.0.join(format!("action-{token}.js"));
        let plugin = format!(
            "{}-{token}",
            self.directory.0.file_name().unwrap().to_string_lossy()
        );
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .map_err(|e| format!("无法创建窗口动作脚本：{e}"))?;
        let _script = Script {
            bus: self.bus.clone(),
            plugin,
            file,
        };
        output
            .write_all(source.as_bytes())
            .map_err(|e| e.to_string())?;
        drop(output);
        let scripting = Proxy::new(
            &self.bus,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .map_err(|e| e.to_string())?;
        let id: i32 = scripting
            .call(
                "loadScript",
                &(
                    _script.file.to_str().ok_or("脚本路径不是 UTF-8")?,
                    &_script.plugin,
                ),
            )
            .map_err(|e| format!("无法加载窗口动作脚本：{e}"))?;
        if id < 0 {
            return Err("KWin 拒绝加载窗口动作脚本".into());
        }
        let path = format!("/Scripting/Script{id}");
        let script = Proxy::new(
            &self.bus,
            "org.kde.KWin",
            path.as_str(),
            "org.kde.kwin.Script",
        )
        .map_err(|e| e.to_string())?;
        let _: () = script
            .call("run", &())
            .map_err(|e| format!("无法执行窗口动作：{e}"))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let (reply, error) = self
                .results
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| "窗口动作未返回结果")?;
            if reply == token {
                return if error.is_empty() {
                    Ok(())
                } else {
                    Err(format!("窗口动作失败：{error}"))
                };
            }
        }
    }
}

impl Backend for KWin {
    fn execute(&mut self, operation: Operation) -> Result<(), String> {
        let body = match operation {
            Operation::Minimize => {
                "const windows = workspace.windowAt(workspace.cursorPos, 1); if (windows.length && windows[0].minimizable) { windows[0].minimized = true; }"
            }
            Operation::Maximize => {
                "const windows = workspace.windowAt(workspace.cursorPos, 1); if (windows.length && windows[0].maximizable) { windows[0].setMaximize(true, true); }"
            }
            Operation::MinimizeAll => {
                "const desktop = workspace.currentDesktop; const activity = workspace.currentActivity; for (const window of workspace.windowList()) { const onDesktop = window.desktops.length === 0 || window.desktops.indexOf(desktop) !== -1; const onActivity = window.activities.length === 0 || window.activities.indexOf(activity) !== -1; if (window.minimizable && onDesktop && onActivity) { window.minimized = true; } }"
            }
        };
        self.run_body(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait_for(backend: &mut KWin, body: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let result = backend.run_body(body);
            if result.is_ok() {
                return;
            }
            assert!(Instant::now() < deadline, "{result:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn exercise(backend: &mut KWin, select: &str) {
        wait_for(backend, select);
        let actions = crate::host_window::Handle::start().unwrap();
        let submit = |object| {
            let now = Instant::now();
            let action = crate::gestures::Action::new(24, object, 3, "窗口动作");
            let mut vm = crate::host_program::Vm::new(
                crate::host_program::prepare(&action).unwrap(),
                now,
                false,
            );
            let outputs = vm.tick(now, [0; 3]).unwrap();
            let [crate::host_program::Output::Window(operation)] = outputs.as_slice() else {
                panic!("expected window output");
            };
            actions.execute(*operation).unwrap();
        };
        backend.run_body(&format!("{select} target.minimized = false; target.setMaximize(false, false); const p = workspace.cursorPos; target.frameGeometry = {{x:p.x-80,y:p.y-60,width:240,height:180}}; active.frameGeometry = {{x:p.x+300,y:p.y+200,width:200,height:150}}; workspace.activeWindow = active; workspace.raiseWindow(target);")).unwrap();
        let under_pointer = format!(
            "{select} if (workspace.windowAt(workspace.cursorPos,1)[0] !== target) {{ throw new Error('pointer is not over target'); }}"
        );
        wait_for(backend, &under_pointer);
        // The wheel monitor must use the pointer window even when another
        // window has focus. Exercise native Wayland and Xwayland alike.
        let wheel =
            crate::wheel4d::Monitor::start_with_direction(&crate::wheel4d::Settings::default())
                .unwrap();
        let mut wheel_settings = crate::wheel4d::Settings::default();
        assert!(!wheel.horizontal(&wheel_settings).unwrap());
        let app = wheel.application();
        assert!(
            !app.is_empty(),
            "pointer window has no application identifier"
        );
        // At one third of the window width, a 20% rule must select horizontal
        // scrolling. Unmatched identifiers keep the 90% default divider.
        wheel_settings
            .applications
            .push(crate::wheel4d::Application {
                app: app.clone(),
                divider: 200,
            });
        assert!(wheel.horizontal(&wheel_settings).unwrap());
        wheel_settings.applications[0].app = format!("{app}.unmatched");
        assert!(!wheel.horizontal(&wheel_settings).unwrap());
        wheel_settings.applications.clear();
        backend.run_body(&format!("{select} const p = workspace.cursorPos; target.frameGeometry = {{x:p.x-230,y:p.y-60,width:240,height:180}};")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !wheel.horizontal(&wheel_settings).unwrap() {
            assert!(
                Instant::now() < deadline,
                "right wheel region was not observed"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let icon_visible = "const tip = workspace.stackingOrder.find(w => w.caption === 'Bloody A90 horizontal wheel' && !w.deleted); if (!tip || tip.active) { throw new Error('wheel direction icon unavailable or focused'); } const r = tip.frameGeometry; if (r.width !== 28 || r.height !== 24 || workspace.windowAt({x:r.x+14,y:r.y+12},0).indexOf(tip) !== -1) { throw new Error('wheel icon geometry or input passthrough differs'); }";
        let icon_hidden = "if (workspace.stackingOrder.some(w => w.caption === 'Bloody A90 horizontal wheel' && !w.deleted)) { throw new Error('wheel icon remained visible'); }";
        wait_for(backend, icon_visible);
        backend.run_body(&format!("{select} const p = workspace.cursorPos; target.frameGeometry = {{x:p.x-80,y:p.y-60,width:240,height:180}};")).unwrap();
        wait_for(backend, icon_hidden);
        backend.run_body(&format!("{select} const p = workspace.cursorPos; target.frameGeometry = {{x:p.x-230,y:p.y-60,width:240,height:180}};")).unwrap();
        wait_for(backend, icon_visible);
        crate::gesture_runtime::tests::exercise_wheel_and_gesture_capture(wheel);
        wait_for(
            backend,
            "if (workspace.stackingOrder.some(w => w.caption === 'Bloody A90 horizontal wheel' && !w.deleted)) { throw new Error('wheel icon remained after capture stopped'); }",
        );
        submit(1);
        wait_for(
            backend,
            &format!(
                "{select} if (!target.minimized || active.minimized || workspace.activeWindow !== active) {{ throw new Error('minimize changed the wrong window or focus'); }}"
            ),
        );
        backend
            .run_body(&format!(
                "{select} target.minimized = false; workspace.raiseWindow(target);"
            ))
            .unwrap();
        wait_for(backend, &under_pointer);
        submit(2);
        wait_for(
            backend,
            &format!(
                "{select} if (target.maximizeMode !== 3 || active.maximizeMode !== 0) {{ throw new Error('maximize changed the wrong window'); }}"
            ),
        );
        backend.execute(Operation::Maximize).unwrap();
        backend.run_body(&format!("{select} if (target.maximizeMode !== 3) {{ throw new Error('maximize must not toggle restore'); }}")).unwrap();
        submit(0);
        wait_for(
            backend,
            &format!(
                "{select} if (!target.minimized || !active.minimized) {{ throw new Error('all minimize did not minimize both windows'); }}"
            ),
        );
        backend.run_body(&format!("{select} const here = workspace.currentDesktop; if (workspace.desktops.length < 2) {{ workspace.createDesktop(1, 'A90 isolated second desktop'); }} const other = workspace.desktops.find(desktop => desktop !== here); if (!other) {{ throw new Error('second desktop unavailable'); }} target.desktops = [here]; active.desktops = [other]; target.minimized = false; active.minimized = false; workspace.currentDesktop = here;")).unwrap();
        submit(0);
        wait_for(
            backend,
            &format!(
                "{select} if (!target.minimized || active.minimized) {{ throw new Error('all minimize affected another desktop'); }}"
            ),
        );
        // Sticky windows are visible on the current desktop and must be included.
        backend
            .run_body(&format!(
                "{select} target.desktops = []; target.minimized = false;"
            ))
            .unwrap();
        submit(0);
        wait_for(
            backend,
            &format!(
                "{select} if (!target.minimized || active.minimized) {{ throw new Error('all minimize ignored a sticky window'); }}"
            ),
        );
        backend.run_body(&format!("{select} target.desktops = [workspace.currentDesktop]; active.desktops = [workspace.currentDesktop]; target.minimized = false; active.minimized = false;")).unwrap();
        assert!(actions.poll().is_empty());
    }

    struct NativeClient(std::process::Child);
    struct TestFile(PathBuf);
    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    impl Drop for NativeClient {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[test]
    #[ignore = "requires KWin; checks scripting and cleanup without changing windows"]
    fn kwin_window_api_and_callback() {
        let backend = KWin::connect().unwrap();
        let path = backend.directory.0.clone();
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
        drop(backend);
        assert!(!path.exists());
    }

    #[test]
    #[ignore = "requires A90_TEST_KWIN_ISOLATED=1 and private KWin; creates only an internal test overlay"]
    fn isolated_kwin_internal_overlay_position_focus_and_cleanup() {
        assert_eq!(std::env::var("A90_TEST_KWIN_ISOLATED").as_deref(), Ok("1"));
        let mut backend = KWin::connect().unwrap();
        let file = backend.directory.0.join("overlay.qml");
        fs::write(&file, r##"
import QtQuick
import QtQuick.Window as QuickWindow
import org.kde.kwin
import org.kde.plasma.workspace.dbus 1.0 as DBus
QuickWindow.Window {
        title: "A90 isolated wheel overlay"
        x: 100
        y: 120
        width: 24
        height: 24
        color: "transparent"
        flags: Qt.ToolTip | Qt.FramelessWindowHint | Qt.WindowDoesNotAcceptFocus | Qt.WindowTransparentForInput
        visible: true
        Component.onCompleted: DBus.SessionBus.asyncCall({service: "A90_TEST_BUS", path: "/org/bloody/A90/WindowAction", iface: "org.bloody.A90.WindowAction", member: "Completed", arguments: ["overlay-probe", ""]}, function(reply) {}, function(reply) { console.warn("A90 overlay callback failed", reply.error.message); })
        Rectangle { anchors.fill: parent; color: "#d22c35"; radius: 4 }
}
"##.replace("A90_TEST_BUS", backend.bus.unique_name().unwrap().as_str())).unwrap();
        let overlay = Script {
            bus: backend.bus.clone(),
            plugin: format!("a90-overlay-probe-{}", std::process::id()),
            file,
        };
        let scripting = Proxy::new(
            &backend.bus,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .unwrap();
        let id: i32 = scripting
            .call(
                "loadDeclarativeScript",
                &(overlay.file.to_str().unwrap(), &overlay.plugin),
            )
            .unwrap();
        assert!(id >= 0);
        let path = format!("/Scripting/Script{id}");
        let script = Proxy::new(
            &backend.bus,
            "org.kde.KWin",
            path.as_str(),
            "org.kde.kwin.Script",
        )
        .unwrap();
        let _: () = script.call("run", &()).unwrap();
        drop(script);
        drop(scripting);
        let (token, error) = backend
            .results
            .recv_timeout(Duration::from_secs(3))
            .expect("QML overlay did not complete construction");
        assert_eq!(token, "overlay-probe");
        assert!(error.is_empty());
        wait_for(
            &mut backend,
            "const tip = workspace.stackingOrder.find(w => w.caption === 'A90 isolated wheel overlay'); if (!tip) { throw new Error('internal overlay not mapped: ' + JSON.stringify(workspace.stackingOrder.map(w => ({caption:w.caption, normal:w.normalWindow, geometry:w.frameGeometry})))); } const r = tip.frameGeometry; if (r.x !== 100 || r.y !== 120 || r.width !== 24 || r.height !== 24 || tip.active) { throw new Error('internal overlay geometry or focus differs'); } if (workspace.windowAt({x:112,y:132},0).indexOf(tip) !== -1) { throw new Error('transparent overlay intercepts pointer input'); }",
        );
        drop(overlay);
        wait_for(
            &mut backend,
            "if (workspace.stackingOrder.some(w => w.caption === 'A90 isolated wheel overlay')) { throw new Error('internal overlay remained after script unload'); }",
        );
    }

    #[test]
    #[ignore = "requires A90_TEST_KWIN_ISOLATED=1, private KWin, Xwayland, KWrite and libei 1.6"]
    fn isolated_kwin_minimize_and_maximize_pointer_window() {
        use x11rb::{
            connection::Connection as _,
            protocol::xproto::{ConnectionExt, CreateWindowAux, PropMode, WindowClass},
            wrapper::ConnectionExt as _,
        };
        assert_eq!(
            std::env::var("A90_TEST_KWIN_ISOLATED").as_deref(),
            Ok("1"),
            "start a private KWin session"
        );
        let (client, screen) = x11rb::connect(None).unwrap();
        let root = client.setup().roots[screen].root;
        let name = client
            .intern_atom(false, b"_NET_WM_NAME")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let utf8 = client
            .intern_atom(false, b"UTF8_STRING")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let wm_class = client
            .intern_atom(false, b"WM_CLASS")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let string = client
            .intern_atom(false, b"STRING")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let mut windows = Vec::new();
        for title in ["A90 isolated target", "A90 isolated active"] {
            let window = client.generate_id().unwrap();
            client
                .create_window(
                    0,
                    window,
                    root,
                    100,
                    100,
                    240,
                    180,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &CreateWindowAux::new().background_pixel(0x202020),
                )
                .unwrap()
                .check()
                .unwrap();
            client
                .change_property8(PropMode::REPLACE, window, name, utf8, title.as_bytes())
                .unwrap();
            client
                .change_property8(
                    PropMode::REPLACE,
                    window,
                    wm_class,
                    string,
                    b"a90-isolated-window\0A90IsolatedWindow\0",
                )
                .unwrap();
            client.map_window(window).unwrap();
            windows.push(window);
        }
        client.flush().unwrap();
        let mut backend = KWin::connect().unwrap();
        let select = "const windows = workspace.windowList(); const target = windows.find(w => w.caption === 'A90 isolated target'); const active = windows.find(w => w.caption === 'A90 isolated active'); if (!target || !active) { throw new Error('test windows not ready'); }";
        exercise(&mut backend, select);
        backend
            .run_body(&format!("{select} target.minimized = true;"))
            .unwrap();
        let document = TestFile(backend.directory.0.join("native-text.txt"));
        fs::write(&document.0, "").unwrap();
        // Force a native Wayland client. This process cannot fall back to X11.
        let native = NativeClient(
            std::process::Command::new("kwrite")
                .env("QT_QPA_PLATFORM", "wayland")
                .env("QT_IM_MODULE", "compose")
                .arg(&document.0)
                .env_remove("DISPLAY")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .expect("KWrite is required for the native Wayland check"),
        );
        let select_native = format!(
            "const windows = workspace.windowList(); const target = windows.find(w => w.pid === {}); const active = windows.find(w => w.caption === 'A90 isolated active'); if (!target || !active) {{ throw new Error('native test window not ready'); }}",
            native.0.id()
        );
        exercise(&mut backend, &select_native);
        // Type into the native editor, save with Ctrl+S, then compare the file.
        backend
            .run_body(&format!("{select_native} workspace.activeWindow = target;"))
            .unwrap();
        wait_for(
            &mut backend,
            &format!(
                "{select_native} if (workspace.activeWindow !== target) {{ throw new Error('native editor is not active'); }}"
            ),
        );
        let now = Instant::now();
        let action = crate::gestures::Action::input_text("中😀\r\nA90\n").unwrap();
        let mut vm = crate::host_program::Vm::new(
            crate::host_program::prepare(&action).unwrap(),
            now,
            false,
        );
        let outputs = vm.tick(now, [0; 3]).unwrap();
        let [crate::host_program::Output::Text(plan)] = outputs.as_slice() else {
            panic!("expected text output");
        };
        crate::host_text::input_and_save_private_editor_for_test(plan).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let saved = fs::read_to_string(&document.0).unwrap();
            if saved == "中😀\nA90\n" {
                break;
            }
            assert!(Instant::now() < deadline, "native text mismatch: {saved:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(native);
        drop(document);
        let script_directory = backend.directory.0.clone();
        drop(backend);
        assert!(!script_directory.exists());
        for window in windows {
            client.destroy_window(window).unwrap();
        }
        client.flush().unwrap();
    }
}
