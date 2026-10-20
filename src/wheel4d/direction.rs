//! A KWin internal window uses global logical coordinates. It accepts no input.
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};
use zbus::blocking::{Connection, Proxy};

pub(super) struct Icon {
    bus: Connection,
    pub(super) file: PathBuf,
    plugin: String,
}

impl Icon {
    #[cfg(test)]
    pub(super) fn loaded(&self) -> bool {
        Proxy::new(
            &self.bus,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .unwrap()
        .call("isScriptLoaded", &self.plugin)
        .unwrap()
    }

    pub(super) fn start(
        bus: Connection,
        directory: &std::path::Path,
        plugin: &str,
    ) -> Result<Self, String> {
        Self::start_kind(
            bus,
            directory,
            plugin,
            "direction",
            include_str!("direction.qml"),
        )
    }

    pub(super) fn start_kind(
        bus: Connection,
        directory: &std::path::Path,
        plugin: &str,
        kind: &str,
        template: &str,
    ) -> Result<Self, String> {
        let icon = Self {
            bus,
            file: directory.join(format!("{kind}.qml")),
            plugin: format!("{plugin}-{kind}"),
        };
        let destination = icon
            .bus
            .unique_name()
            .ok_or("图标服务缺少总线名称")?
            .as_str();
        // D-Bus names and a generated plugin name are the only replacements.
        let source = template
            .replace("A90_DESTINATION", destination)
            .replace("A90_PLUGIN", &icon.plugin);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&icon.file)
            .map_err(|e| e.to_string())?;
        file.write_all(source.as_bytes())
            .map_err(|e| e.to_string())?;
        drop(file);
        let scripting = Proxy::new(
            &icon.bus,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .map_err(|e| e.to_string())?;
        let id: i32 = scripting
            .call(
                "loadDeclarativeScript",
                &(
                    icon.file.to_str().ok_or("图标脚本路径不是 UTF-8")?,
                    &icon.plugin,
                ),
            )
            .map_err(|e| e.to_string())?;
        if id < 0 {
            return Err("KWin 拒绝加载滚轮方向图标".into());
        }
        let path = format!("/Scripting/Script{id}");
        let script = Proxy::new(
            &icon.bus,
            "org.kde.KWin",
            path.as_str(),
            "org.kde.kwin.Script",
        )
        .map_err(|e| e.to_string())?;
        let _: () = script.call("run", &()).map_err(|e| e.to_string())?;
        drop(script);
        drop(scripting);
        Ok(icon)
    }
}

impl Drop for Icon {
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
