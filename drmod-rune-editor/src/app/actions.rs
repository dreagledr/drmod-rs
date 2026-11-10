//! The actions the panels ask for: the workspace, the dialogs and the run.

use std::path::PathBuf;

use rune::{FileDialogOptions, Tx};

use crate::app::App;
use crate::workspace;

/// Ask for a new workspace folder with the system picker.
pub fn ask_folder(app: &mut App, tx: &mut Tx) {
    let options = FileDialogOptions {
        title: "Папка со скриптами".to_owned(),
        directory: app.folder.clone(),
        ..Default::default()
    };

    tx.open_folder(options, |app: &mut App, result, _tx| match result {
        Ok(Some(folder)) => app.open_folder(folder),
        Ok(None) => {}
        Err(error) => app.message = Some(format!("Выбор папки не открылся: {error}")),
    });
}

impl App {
    pub fn open_folder(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
        self.selected = None;
        self.loaded = None;
        self.reload_listing();
        self.save_settings();

        if let Some(first) = self.listing.scripts.first() {
            let path = first.path.clone();
            self.select(path);
        } else {
            self.clear_loaded();
        }
        self.message = self
            .listing
            .error
            .clone()
            .or_else(|| Some(format!("Скриптов в папке: {}", self.listing.scripts.len())));
    }

    pub fn save(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            self.message = Some("Нечего сохранять".to_owned());
            return;
        };

        match workspace::write(&entry.path, &self.text.text) {
            None => {
                self.buffers
                    .insert(entry.path.clone(), self.text.text.clone());
                self.reload_listing();
                self.message = Some(format!("Сохранено: {}.tas", entry.name));
            }
            Some(error) => self.message = Some(error),
        }
    }

    pub fn new_script(&mut self) {
        let Some(folder) = self.folder.clone() else {
            self.message = Some("Сначала выберите папку со скриптами".to_owned());
            return;
        };

        match workspace::create(&folder) {
            (Some(path), None) => {
                self.reload_listing();
                self.select(path);
                self.message = Some("Новый скрипт".to_owned());
            }
            (_, Some(error)) => self.message = Some(error),
            _ => {}
        }
    }

    pub fn duplicate_script(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            self.message = Some("Выберите скрипт".to_owned());
            return;
        };

        match workspace::duplicate(&entry.path) {
            (Some(path), None) => {
                self.reload_listing();
                self.select(path);
                self.message = Some(format!("Копия: {}", entry.name));
            }
            (_, Some(error)) => self.message = Some(error),
            _ => {}
        }
    }

    pub fn begin_rename(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            self.message = Some("Выберите скрипт".to_owned());
            return;
        };

        self.rename_text = rune::TextEdit::new(entry.name);
        self.rename_text.set_caret(self.rename_text.text.len());
        // A dialog opens on its content; the model's own flag is what an incoming keystroke checks.
        self.rename_text.focused = true;
        self.rename_target = Some(entry.path);
    }

    pub fn confirm_rename(&mut self) {
        let Some(from) = self.rename_target.take() else {
            return;
        };
        let name = self.rename_text.text.clone();

        match workspace::rename(&from, &name) {
            (Some(to), None) => {
                crate::buffers::renamed(&mut self.buffers, &from, &to);
                self.reload_listing();
                self.select(to);
                self.message = Some("Переименовано".to_owned());
            }
            (_, Some(error)) => self.message = Some(error),
            _ => {}
        }
    }

    pub fn begin_delete(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            self.message = Some("Выберите скрипт".to_owned());
            return;
        };
        self.delete_target = Some(entry.path);
    }

    pub fn confirm_delete(&mut self) {
        let Some(path) = self.delete_target.take() else {
            return;
        };

        match workspace::delete(&path) {
            None => {
                crate::buffers::without(&mut self.buffers, &path);
                if self.selected.as_deref() == Some(path.as_path()) {
                    self.selected = None;
                    self.loaded = None;
                }
                self.reload_listing();
                if let Some(first) = self.listing.scripts.first() {
                    let first = first.path.clone();
                    self.select(first);
                } else {
                    self.clear_loaded();
                }
                self.message = Some("Скрипт удалён".to_owned());
            }
            Some(error) => self.message = Some(error),
        }
    }

    pub fn run(&mut self) {
        if self.worker.busy() || self.preparing {
            return;
        }

        let Some(document) = self.status.document.clone() else {
            self.run_error = self.status.error.clone();
            return;
        };

        let rules = match self.rules_with_seed() {
            Ok(rules) => rules,
            Err(error) => {
                self.run_error = Some(error);
                return;
            }
        };

        self.save_settings();
        self.run_error = None;
        self.run_message = None;
        self.preparing = true;
        self.worker.run(document, rules);
    }

    pub fn apply_rules(&mut self) {
        if self.worker.busy() || self.preparing {
            return;
        }

        let rules = match self.rules_with_seed() {
            Ok(rules) => rules,
            Err(error) => {
                self.run_error = Some(error);
                return;
            }
        };

        self.save_settings();
        self.run_error = None;
        self.run_message = None;
        self.preparing = true;
        self.worker.apply(rules);
    }

    pub fn cancel_run(&mut self) {
        if self.worker.busy() {
            return;
        }

        self.run_error = None;
        self.preparing = true;
        self.worker.stop();
    }
}
