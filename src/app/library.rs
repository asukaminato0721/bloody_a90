use super::{App, path_field};
use crate::{skin, worker::Job};
use bloody_a90::{
    amc,
    library::{self, Command, Deleted, Document, Key, Listing, Update},
    macros::Repeat,
};
use eframe::egui::{self, RichText};
use std::path::PathBuf;

pub(super) struct Editor {
    pub(super) root: String,
    root_draft: String,
    loaded: bool,
    listing: Listing,
    category: String,
    category_name: String,
    file_name: String,
    selected: Option<Key>,
    document: Option<Document>,
    deleted: Option<Deleted>,
    pending_text: Option<(String, String, Repeat)>,
}

impl Default for Editor {
    fn default() -> Self {
        let root = library::default_root().display().to_string();
        Self {
            root_draft: root.clone(),
            root,
            loaded: false,
            listing: Listing::default(),
            category: String::new(),
            category_name: String::new(),
            file_name: "新宏".into(),
            selected: None,
            document: None,
            deleted: None,
            pending_text: None,
        }
    }
}

impl Editor {
    pub(super) fn set_root(&mut self, root: String) {
        *self = Self {
            root_draft: root.clone(),
            root,
            ..Self::default()
        };
    }
}

impl App {
    pub(super) fn library_reply(&mut self, result: Result<Box<Update>, String>) {
        self.library.loaded = true;
        let update = match result {
            Ok(update) => *update,
            Err(error) => {
                self.report(Err(error));
                return;
            }
        };
        let editor = &mut self.library;
        let text_unchanged = editor
            .pending_text
            .take()
            .is_none_or(|(press, release, repeat)| {
                press == self.press && release == self.release && repeat == self.repeat
            });
        if let Some((from, to)) = &update.renamed_category {
            if editor.category == *from {
                editor.category = to.clone();
                editor.category_name = to.clone();
            }
            if let Some(key) = &mut editor.selected
                && key.category == *from
            {
                key.category = to.clone();
            }
            if let Some(doc) = &mut editor.document
                && doc.key.category == *from
            {
                doc.key.category = to.clone();
            }
        }
        if let Some(deleted) = update.deleted {
            if editor.document.as_ref().is_some_and(|doc| {
                doc.key.category == deleted.category
                    && deleted
                        .file
                        .as_ref()
                        .is_none_or(|file| doc.key.file == *file)
            }) {
                editor.document = None;
            }
            editor.selected = None;
            editor.deleted = Some(deleted);
        }
        if update.restored {
            editor.deleted = None;
        }
        editor.listing = update.listing;
        if let Some(category) = update.selected_category {
            editor.category = category.clone();
            editor.category_name = category;
            editor.selected = None;
        }
        if !editor.listing.categories.contains(&editor.category) {
            editor.category = editor
                .listing
                .categories
                .first()
                .cloned()
                .unwrap_or_default();
            editor.category_name = editor.category.clone();
        }
        let loaded = if let Some(doc) = update.document {
            editor.category = doc.key.category.clone();
            editor.category_name = doc.key.category.clone();
            editor.selected = Some(doc.key.clone());
            editor.file_name = doc.key.file.clone();
            let bytes = doc.original.clone();
            editor.document = Some(doc);
            if update.load_editor && text_unchanged {
                self.import_amc(&bytes).map(|_| ())
            } else {
                Ok(())
            }
        } else {
            Ok(())
        };
        self.report(loaded.map(|()| update.message));
    }

    fn library_command(&mut self, command: Command) {
        self.library.pending_text = Some((self.press.clone(), self.release.clone(), self.repeat));
        self.start(Job::Library(
            PathBuf::from(&self.library.root),
            Box::new(command),
        ));
    }

    fn library_macro(&self, description: String) -> amc::Macro {
        amc::Macro {
            description,
            high_language: false,
            repeat: self.repeat,
            press: self.press.clone(),
            release: self.release.clone(),
        }
    }

    pub(super) fn library_page(&mut self, ui: &mut egui::Ui) {
        // The smoke test only renders controls. It does not read the user's library.
        if !self.library.loaded && !self.pending && self.smoke_frames.is_none() {
            self.library.loaded = true;
            self.library_command(Command::List);
        }
        let mut command = None;
        let mut change_root = None;
        let available = !self.pending;
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                path_field(ui, &mut self.library.root_draft, "宏库目录", 440.0);
                if skin::button(
                    ui,
                    "打开宏库",
                    120.0,
                    available && !self.library.root_draft.trim().is_empty(),
                )
                .clicked()
                {
                    change_root = Some(self.library.root_draft.trim().to_owned());
                    command = Some(Command::List);
                }
                if skin::button(ui, "刷新", 90.0, available).clicked() {
                    command = Some(Command::List);
                }
            });
            ui.label(
                RichText::new(format!(
                    "当前宏库：{} · 分类对应目录，宏对应 AMC 文件。删除项保留在该宏库的 .trash 目录。",
                    self.library.root
                ))
                .color(skin::GRAY),
            );
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                ui.vertical(|ui| {
                    ui.set_min_width(220.0);
                    ui.spacing_mut().item_spacing.y = 6.0;
                    skin::step(ui, "①", "收藏夹");
                    for category in &self.library.listing.categories {
                        if skin::list_item(ui, category, self.library.category == *category, 220.0)
                            .clicked()
                        {
                            self.library.category = category.clone();
                            self.library.category_name = category.clone();
                            self.library.selected = None;
                        }
                    }
                    if self.library.listing.categories.is_empty() {
                        ui.label("暂无分类");
                    }
                });
                ui.vertical(|ui| {
                    ui.set_min_width(330.0);
                    ui.spacing_mut().item_spacing.y = 6.0;
                    skin::step(ui, "②", "文件");
                    for key in self
                        .library
                        .listing
                        .files
                        .iter()
                        .filter(|k| k.category == self.library.category)
                    {
                        if skin::list_item(
                            ui,
                            &key.file,
                            self.library.selected.as_ref() == Some(key),
                            330.0,
                        )
                        .clicked()
                        {
                            self.library.selected = Some(key.clone());
                            self.library.file_name = key.file.clone();
                        }
                    }
                    if self
                        .library
                        .listing
                        .files
                        .iter()
                        .all(|k| k.category != self.library.category)
                    {
                        ui.label("暂无宏文件");
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("分类名称");
                ui.add(
                    egui::TextEdit::singleline(&mut self.library.category_name)
                        .desired_width(210.0),
                );
                let named = available && !self.library.category_name.trim().is_empty();
                if skin::button(ui, "新建分类", 110.0, named).clicked() {
                    command = Some(Command::CreateCategory(
                        self.library.category_name.trim().into(),
                    ));
                }
                let selected = available && !self.library.category.is_empty();
                if skin::button(ui, "分类重命名", 120.0, named && selected).clicked() {
                    command = Some(Command::RenameCategory(
                        self.library.category.clone(),
                        self.library.category_name.trim().into(),
                    ));
                }
                if skin::button(ui, "删除分类", 110.0, selected).clicked() {
                    command = Some(Command::DeleteCategory(self.library.category.clone()));
                }
            });
            ui.horizontal(|ui| {
                ui.label("宏名称");
                ui.add(
                    egui::TextEdit::singleline(&mut self.library.file_name).desired_width(210.0),
                );
                let named = available
                    && !self.library.category.is_empty()
                    && !self.library.file_name.trim().is_empty();
                let key = library::filename(self.library.file_name.trim()).map(|file| Key {
                    category: self.library.category.clone(),
                    file,
                });
                let named = named && key.is_ok();
                if let Err(error) = &key {
                    ui.colored_label(skin::ERROR, error);
                }
                if skin::button(ui, "新建空宏", 110.0, named).clicked()
                    && let Ok(key) = &key
                {
                    command = Some(Command::Create(
                        key.clone(),
                        amc::Macro {
                            description: key.file.clone(),
                            high_language: false,
                            repeat: Repeat::Once,
                            press: String::new(),
                            release: String::new(),
                        },
                    ));
                }
                if skin::button(ui, "另存为新宏", 120.0, named && self.compiled.is_ok()).clicked()
                    && let Ok(key) = &key
                {
                    command = Some(Command::Create(
                        key.clone(),
                        self.library_macro(key.file.clone()),
                    ));
                }
            });
            ui.horizontal(|ui| {
                let doc = self
                    .library
                    .document
                    .as_ref()
                    .filter(|d| self.library.selected.as_ref() == Some(&d.key));
                if skin::button(
                    ui,
                    "载入编辑器",
                    130.0,
                    available && self.library.selected.is_some(),
                )
                .clicked()
                    && let Some(key) = &self.library.selected
                {
                    command = Some(Command::Read(key.clone()));
                }
                if skin::button(
                    ui,
                    "保存",
                    100.0,
                    available && doc.is_some() && self.compiled.is_ok(),
                )
                .clicked()
                    && let Some(doc) = doc
                {
                    command = Some(Command::Save(
                        doc.key.clone(),
                        doc.original.clone(),
                        self.library_macro(doc.macro_file.description.clone()),
                    ));
                }
                if skin::button(ui, "宏重命名", 110.0, available && doc.is_some()).clicked()
                    && let Some(doc) = doc
                {
                    command = Some(Command::Rename(
                        doc.key.clone(),
                        doc.original.clone(),
                        self.library.file_name.trim().into(),
                    ));
                }
                if skin::button(ui, "删除宏", 100.0, available && doc.is_some()).clicked()
                    && let Some(doc) = doc
                {
                    command = Some(Command::Delete(doc.key.clone(), doc.original.clone()));
                }
                if skin::button(
                    ui,
                    "恢复上次删除",
                    140.0,
                    available && self.library.deleted.is_some(),
                )
                .clicked()
                    && let Some(deleted) = &self.library.deleted
                {
                    command = Some(Command::Restore(deleted.clone()));
                }
            });
            if let Some(doc) = &self.library.document {
                ui.label(format!(
                    "当前编辑文件：{} / {}",
                    doc.key.category, doc.key.file
                ));
            }
        });
        if let Some(root) = change_root {
            self.library.set_root(root);
            self.library.loaded = true;
        }
        if let Some(command) = command {
            self.library_command(command);
        }
        ui.add_space(12.0);
        ui.separator();
        self.oscar(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_operations_preserve_editor_changes() {
        let dir = std::env::temp_dir().join(format!(
            "a90-library-editor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        struct Temp(PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _temp = Temp(dir.clone());
        let store = library::Store::new(&dir);
        store.run(Command::CreateCategory("Game".into())).unwrap();
        let key = Key {
            category: "Game".into(),
            file: "One.amc".into(),
        };
        let file = amc::Macro {
            description: "One".into(),
            high_language: false,
            repeat: Repeat::Once,
            press: "KeyDownUp 4".into(),
            release: String::new(),
        };
        let mut app = App::from_context(&egui::Context::default());
        app.library.set_root(dir.display().to_string());
        app.library_reply(Ok(Box::new(
            store.run(Command::Create(key.clone(), file)).unwrap(),
        )));
        assert_eq!(app.press, "KeyDownUp 4");
        let original = app.library.document.as_ref().unwrap().original.clone();
        app.press = "KeyDownUp 5".into();
        app.recompile();
        app.library_reply(Ok(Box::new(
            store
                .run(Command::Rename(key, original, "Two.amc".into()))
                .unwrap(),
        )));
        assert_eq!(app.press, "KeyDownUp 5");
        let key = app.library.document.as_ref().unwrap().key.clone();
        assert_eq!(key.file, "Two.amc");
        app.library.pending_text = Some((app.press.clone(), app.release.clone(), app.repeat));
        // Text entered while a file read runs must remain in the editor.
        app.press = "KeyDownUp 6".into();
        app.recompile();
        app.library_reply(Ok(Box::new(store.run(Command::Read(key)).unwrap())));
        assert_eq!(app.press, "KeyDownUp 6");
        assert!(app.compiled.is_ok());
        assert_eq!(app.compiled.as_ref().unwrap().press[0], 0x2106);
    }
}
