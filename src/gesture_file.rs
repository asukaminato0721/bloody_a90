//! Save gesture documents without truncating an existing file on failure.
use std::{
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);

fn unchanged(path: &Path, expected: &[u8]) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || fs::read(path).map_err(|e| e.to_string())? != expected {
        return Err("文件已改变。请先重新读取文件，或另存为新文件。".into());
    }
    Ok(())
}

/// None creates a new name. Some replaces only the bytes last read by this app.
pub fn save(path: &Path, bytes: &[u8], expected: Option<&[u8]>) -> Result<(), String> {
    crate::gestures::Document::parse(bytes)?;
    save_checked(path, bytes, expected)
}

pub(crate) fn save_checked(
    path: &Path,
    bytes: &[u8],
    expected: Option<&[u8]>,
) -> Result<(), String> {
    if let Some(expected) = expected {
        unchanged(path, expected)?;
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = None;
    for _ in 0..32 {
        let name = parent.join(format!(
            ".a90-gesture-{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&name)
        {
            Ok(file) => {
                temporary = Some((name, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("无法创建临时文件：{e}")),
        }
    }
    let (temporary, mut file) = temporary.ok_or("无法创建手势临时文件")?;
    let result = (|| {
        if let Some(expected) = expected {
            unchanged(path, expected)?;
            let permissions = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
            file.set_permissions(permissions)
                .map_err(|e| e.to_string())?;
        }
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        if let Some(expected) = expected {
            unchanged(path, expected)?;
            fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        } else {
            // hard_link refuses to replace an existing destination.
            fs::hard_link(&temporary, path)
                .map_err(|e| format!("无法创建 {}：{e}", path.display()))?;
        }
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gestures::{Document, Kind};

    #[test]
    fn save_rejects_stale_files_existing_names_and_symlinks() {
        let dir = std::env::temp_dir().join(format!("a90-gesture-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("16in1.gestures");
        let old = Document::new(Kind::Sixteen).to_bytes();
        let mut document = Document::new(Kind::Sixteen);
        document.realtime = true;
        let new = document.to_bytes();
        save(&path, &old, None).unwrap();
        assert!(save(&path, &new, None).is_err());
        assert!(save(&path, &new, Some(b"stale")).is_err());
        assert_eq!(fs::read(&path).unwrap(), old);
        save(&path, &new, Some(&old)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), new);
        let link = dir.join("link.gestures");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(save(&link, &old, Some(&new)).is_err());
        assert_eq!(fs::read(&path).unwrap(), new);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
}
