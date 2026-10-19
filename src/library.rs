//! Local AMC library. Categories are directories. Deleted items stay in .trash.
//! Official entry points: 0070fb44, 0070fe68, 007116e4 and 007125b4.
use crate::amc;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub category: String,
    pub file: String,
}

#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub categories: Vec<String>,
    pub files: Vec<Key>,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub key: Key,
    pub macro_file: amc::Macro,
    pub original: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Deleted {
    pub category: String,
    pub file: Option<String>,
    pub token: String,
}

pub enum Command {
    List,
    Read(Key),
    CreateCategory(String),
    RenameCategory(String, String),
    DeleteCategory(String),
    Create(Key, amc::Macro),
    Save(Key, Vec<u8>, amc::Macro),
    Rename(Key, Vec<u8>, String),
    Delete(Key, Vec<u8>),
    Restore(Deleted),
}

pub struct Update {
    pub listing: Listing,
    pub document: Option<Document>,
    pub deleted: Option<Deleted>,
    pub renamed_category: Option<(String, String)>,
    pub selected_category: Option<String>,
    pub load_editor: bool,
    pub restored: bool,
    pub message: String,
}

pub fn default_root() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("bloody-a90/macros")
}

fn name(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.starts_with('.')
        || value
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err("名称不能为空，不能以点开头，也不能包含路径分隔符或控制字符".into());
    }
    Ok(())
}

pub fn filename(value: &str) -> Result<String, String> {
    name(value)?;
    Ok(if value.to_ascii_lowercase().ends_with(".amc") {
        value.into()
    } else {
        format!("{value}.amc")
    })
}

fn error(path: &Path, e: std::io::Error) -> String {
    format!("{}：{e}", path.display())
}

fn unique() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{now}-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )
}

fn rename_new(source: &Path, dest: &Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(source.as_os_str().as_bytes()).map_err(|_| "源路径包含零字节")?;
    let to = CString::new(dest.as_os_str().as_bytes()).map_err(|_| "目标路径包含零字节")?;
    // RENAME_NOREPLACE also rejects dangling symlinks and concurrent creates.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result != 0 {
        return Err(error(dest, std::io::Error::last_os_error()));
    }
    Ok(())
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn directory(&self, category: &str) -> Result<PathBuf, String> {
        name(category)?;
        let path = self.root.join(category);
        let meta = fs::symlink_metadata(&path).map_err(|e| error(&path, e))?;
        if !meta.is_dir() || meta.is_symlink() {
            return Err("宏库分类必须是实际目录".into());
        }
        Ok(path)
    }

    fn path(&self, key: &Key) -> Result<PathBuf, String> {
        name(&key.file)?;
        if !key.file.to_ascii_lowercase().ends_with(".amc") {
            return Err("宏库文件必须使用 .amc 扩展名".into());
        }
        Ok(self.directory(&key.category)?.join(&key.file))
    }

    fn bytes(&self, key: &Key) -> Result<Vec<u8>, String> {
        let path = self.path(key)?;
        let meta = fs::symlink_metadata(&path).map_err(|e| error(&path, e))?;
        if !meta.is_file() || meta.is_symlink() {
            return Err("宏库文件必须是实际文件".into());
        }
        fs::read(&path).map_err(|e| error(&path, e))
    }

    fn unchanged(&self, key: &Key, expected: &[u8]) -> Result<(), String> {
        if self.bytes(key)? != expected {
            return Err("宏文件已被其他程序修改。请重新载入后再操作。".into());
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Listing, String> {
        let dirs = match fs::read_dir(&self.root) {
            Ok(dirs) => dirs,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Listing::default()),
            Err(e) => return Err(error(&self.root, e)),
        };
        let mut listing = Listing::default();
        for dir in dirs {
            let dir = dir.map_err(|e| error(&self.root, e))?;
            let category = dir.file_name().to_string_lossy().into_owned();
            if name(&category).is_err()
                || !dir.file_type().map_err(|e| error(&dir.path(), e))?.is_dir()
            {
                continue;
            }
            listing.categories.push(category.clone());
            for entry in fs::read_dir(dir.path()).map_err(|e| error(&dir.path(), e))? {
                let entry = entry.map_err(|e| error(&dir.path(), e))?;
                let file = entry.file_name().to_string_lossy().into_owned();
                if name(&file).is_ok()
                    && file.to_ascii_lowercase().ends_with(".amc")
                    && entry
                        .file_type()
                        .map_err(|e| error(&entry.path(), e))?
                        .is_file()
                {
                    listing.files.push(Key {
                        category: category.clone(),
                        file,
                    });
                }
            }
        }
        listing.categories.sort();
        listing
            .files
            .sort_by(|a, b| (&a.category, &a.file).cmp(&(&b.category, &b.file)));
        Ok(listing)
    }

    pub fn read(&self, key: &Key) -> Result<Document, String> {
        let original = self.bytes(key)?;
        let macro_file = amc::parse(&original)?;
        Ok(Document {
            key: key.clone(),
            macro_file,
            original,
        })
    }

    fn write(&self, key: &Key, file: &amc::Macro, expected: Option<&[u8]>) -> Result<(), String> {
        file.compile()?;
        let path = self.path(key)?;
        if let Some(expected) = expected {
            self.unchanged(key, expected)?;
        }
        let temp = path.with_file_name(format!(".a90-{}.tmp", unique()));
        let result = (|| {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| error(&temp, e))?;
            output
                .write_all(&file.to_bytes())
                .and_then(|()| output.sync_all())
                .map_err(|e| error(&temp, e))?;
            if let Some(expected) = expected {
                self.unchanged(key, expected)?;
                fs::rename(&temp, &path).map_err(|e| error(&path, e))?;
            } else {
                // The complete temporary file becomes visible at once.
                // hard_link refuses to replace an existing name.
                fs::hard_link(&temp, &path).map_err(|e| error(&path, e))?;
            }
            Ok(())
        })();
        let _ = fs::remove_file(&temp);
        result
    }

    fn trash(&self, category: &str, file: Option<&str>) -> Result<Deleted, String> {
        let source = if let Some(file) = file {
            self.path(&Key {
                category: category.into(),
                file: file.into(),
            })?
        } else {
            self.directory(category)?
        };
        let trash = self.root.join(".trash");
        match fs::create_dir(&trash) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let meta = fs::symlink_metadata(&trash).map_err(|e| error(&trash, e))?;
                if !meta.is_dir() || meta.is_symlink() {
                    return Err("宏库回收目录无效".into());
                }
            }
            Err(e) => return Err(error(&trash, e)),
        }
        let token = unique();
        rename_new(&source, &trash.join(&token))?;
        Ok(Deleted {
            category: category.into(),
            file: file.map(String::from),
            token,
        })
    }

    pub fn run(&self, command: Command) -> Result<Update, String> {
        let mut read = None;
        let mut update = Update {
            listing: Listing::default(),
            document: None,
            deleted: None,
            renamed_category: None,
            selected_category: None,
            load_editor: false,
            restored: false,
            message: String::new(),
        };
        update.message = match command {
            Command::List => "已读取宏库".into(),
            Command::Read(key) => {
                read = Some(key);
                update.load_editor = true;
                "已载入宏文件".into()
            }
            Command::CreateCategory(category) => {
                name(&category)?;
                fs::create_dir_all(&self.root).map_err(|e| error(&self.root, e))?;
                let path = self.root.join(&category);
                fs::create_dir(&path).map_err(|e| error(&path, e))?;
                update.selected_category = Some(category);
                "已建立分类".into()
            }
            Command::RenameCategory(from, to) => {
                let source = self.directory(&from)?;
                name(&to)?;
                let dest = self.root.join(&to);
                if dest.try_exists().map_err(|e| error(&dest, e))? {
                    return Err("目标分类已存在".into());
                }
                rename_new(&source, &dest)?;
                update.renamed_category = Some((from, to));
                "分类已重命名".into()
            }
            Command::DeleteCategory(category) => {
                update.deleted = Some(self.trash(&category, None)?);
                "分类已移入宏库回收目录".into()
            }
            Command::Create(key, file) => {
                self.write(&key, &file, None)?;
                read = Some(key);
                update.load_editor = true;
                "已建立宏文件".into()
            }
            Command::Save(key, expected, file) => {
                self.write(&key, &file, Some(&expected))?;
                read = Some(key);
                update.load_editor = true;
                "宏文件已保存".into()
            }
            Command::Rename(key, expected, file) => {
                self.unchanged(&key, &expected)?;
                let to = Key {
                    category: key.category.clone(),
                    file: filename(&file)?,
                };
                let source = self.path(&key)?;
                let dest = self.path(&to)?;
                rename_new(&source, &dest)?;
                read = Some(to);
                "宏文件已重命名".into()
            }
            Command::Delete(key, expected) => {
                self.unchanged(&key, &expected)?;
                update.deleted = Some(self.trash(&key.category, Some(&key.file))?);
                "宏文件已移入宏库回收目录".into()
            }
            Command::Restore(deleted) => {
                name(&deleted.category)?;
                name(&deleted.token)?;
                let source = self.root.join(".trash").join(&deleted.token);
                let meta = fs::symlink_metadata(self.root.join(".trash"))
                    .map_err(|e| error(&source, e))?;
                if !meta.is_dir() || meta.is_symlink() {
                    return Err("宏库回收目录无效".into());
                }
                let dest = match &deleted.file {
                    Some(file) => self.path(&Key {
                        category: deleted.category,
                        file: file.clone(),
                    })?,
                    None => self.root.join(&deleted.category),
                };
                if dest.try_exists().map_err(|e| error(&dest, e))? {
                    return Err("原名称已被使用，无法恢复".into());
                }
                rename_new(&source, &dest)?;
                update.restored = true;
                "已恢复删除项".into()
            }
        };
        update.listing = self.list()?;
        update.document = read.as_ref().map(|key| self.read(key)).transpose()?;
        Ok(update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(key: &str) -> amc::Macro {
        amc::Macro {
            description: key.into(),
            high_language: false,
            repeat: crate::macros::Repeat::Once,
            press: "KeyDownUp 4".into(),
            release: String::new(),
        }
    }

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn categories_documents_conflicts_and_recovery() {
        let temp = Temp(std::env::temp_dir().join(format!("a90-library-{}", unique())));
        let store = Store::new(&temp.0);
        assert!(store.list().unwrap().categories.is_empty());
        store.run(Command::CreateCategory("游戏".into())).unwrap();
        let key = Key {
            category: "游戏".into(),
            file: "测试.AMC".into(),
        };
        let doc = store
            .run(Command::Create(key.clone(), file("first")))
            .unwrap()
            .document
            .unwrap();
        assert_eq!(doc.macro_file.description, "first");
        assert_eq!(
            store.list().unwrap().files.as_slice(),
            std::slice::from_ref(&key)
        );
        assert!(
            store
                .run(Command::Create(key.clone(), file("overwrite")))
                .is_err()
        );
        let changed = store
            .run(Command::Save(
                key.clone(),
                doc.original.clone(),
                file("second"),
            ))
            .unwrap()
            .document
            .unwrap();
        assert!(
            store
                .run(Command::Save(key.clone(), doc.original, file("stale")))
                .is_err()
        );
        assert_eq!(store.read(&key).unwrap().macro_file.description, "second");
        let renamed = store
            .run(Command::Rename(key, changed.original, "新名称".into()))
            .unwrap()
            .document
            .unwrap();
        let deleted = store
            .run(Command::Delete(renamed.key.clone(), renamed.original))
            .unwrap()
            .deleted
            .unwrap();
        assert!(store.list().unwrap().files.is_empty());
        store.run(Command::Restore(deleted)).unwrap();
        assert_eq!(
            store.read(&renamed.key).unwrap().macro_file.description,
            "second"
        );
        store
            .run(Command::RenameCategory("游戏".into(), "应用".into()))
            .unwrap();
        let deleted = store
            .run(Command::DeleteCategory("应用".into()))
            .unwrap()
            .deleted
            .unwrap();
        assert!(store.list().unwrap().categories.is_empty());
        store.run(Command::Restore(deleted)).unwrap();
        assert_eq!(store.list().unwrap().categories, ["应用"]);
        assert_eq!(store.list().unwrap().files.len(), 1);
    }

    #[test]
    fn reject_paths_links_and_invalid_programs() {
        let temp = Temp(std::env::temp_dir().join(format!("a90-library-{}", unique())));
        let store = Store::new(&temp.0);
        for category in ["", ".", "..", "../outside", "a/b", "a\\b", ".trash"] {
            assert!(store.run(Command::CreateCategory(category.into())).is_err());
        }
        store.run(Command::CreateCategory("valid".into())).unwrap();
        let key = Key {
            category: "valid".into(),
            file: "macro.amc".into(),
        };
        let mut invalid = file("invalid");
        invalid.press = "unknown instruction".into();
        assert!(store.run(Command::Create(key.clone(), invalid)).is_err());
        assert!(store.list().unwrap().files.is_empty());
        std::os::unix::fs::symlink("/tmp", temp.0.join("link")).unwrap();
        assert!(
            store
                .run(Command::Create(
                    Key {
                        category: "link".into(),
                        file: "escape.amc".into()
                    },
                    file("no")
                ))
                .is_err()
        );
        std::os::unix::fs::symlink("/etc/passwd", temp.0.join("valid/macro.amc")).unwrap();
        assert!(store.read(&key).is_err());
        assert!(store.list().unwrap().files.is_empty());
    }

    #[test]
    fn recovery_and_rename_do_not_replace_existing_names() {
        let temp = Temp(std::env::temp_dir().join(format!("a90-library-{}", unique())));
        let store = Store::new(&temp.0);
        store.run(Command::CreateCategory("Game".into())).unwrap();
        let key = Key {
            category: "Game".into(),
            file: "One.amc".into(),
        };
        let doc = store
            .run(Command::Create(key.clone(), file("old")))
            .unwrap()
            .document
            .unwrap();
        let deleted = store
            .run(Command::Delete(key.clone(), doc.original))
            .unwrap()
            .deleted
            .unwrap();
        let current = store
            .run(Command::Create(key.clone(), file("current")))
            .unwrap()
            .document
            .unwrap();
        assert!(store.run(Command::Restore(deleted)).is_err());
        let other = Key {
            category: "Game".into(),
            file: "Other.amc".into(),
        };
        store
            .run(Command::Create(other.clone(), file("other")))
            .unwrap();
        assert!(
            store
                .run(Command::Rename(
                    key.clone(),
                    current.original,
                    "Other.amc".into()
                ))
                .is_err()
        );
        assert_eq!(store.read(&key).unwrap().macro_file.description, "current");
        assert_eq!(store.read(&other).unwrap().macro_file.description, "other");
    }

    #[test]
    #[ignore = "requires A90_AMC_LIBRARY pointing to an existing categorized AMC library"]
    fn existing_library_files_compile() {
        let store = Store::new(std::env::var_os("A90_AMC_LIBRARY").expect("A90_AMC_LIBRARY"));
        let listing = store.list().unwrap();
        assert!(!listing.files.is_empty());
        for key in listing.files {
            store.read(&key).unwrap().macro_file.compile().unwrap();
        }
    }
}
