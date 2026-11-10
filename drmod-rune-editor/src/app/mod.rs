//! The editor's state and the actions a frame asks for.
//!
//! The view is a pure function of this state; clicks are queued handlers and land here between
//! frames. That is Rune's model, and it is why every action is a method rather than a closure over
//! a mutable borrow of the tree.

pub mod actions;

use std::path::PathBuf;

use drmod_script::projection::{self, ScriptFrame};
use drmod_script::{ScriptTextStatus, dsl};
use rune::{KeyEvent, Span, TextEdit, Tx};
use rune_kit::Kit;
use rune_kit::syntax::SyntaxTheme;

use crate::api::{DEFAULT_URL, FpsCapMode, GameScriptPhase, GameStatus, PlaybackRules};
use crate::poller::Poller;
use crate::settings::Settings;
use crate::work::{Outcome, Worker};
use crate::workspace::{self, Listing, ScriptEntry};

pub struct App {
    /// The kit from the current theme. Rebuilt when the theme flips.
    pub kit: Kit,
    pub syntax: SyntaxTheme,
    pub dark: bool,

    pub settings: Settings,
    pub folder: Option<PathBuf>,
    pub listing: Listing,
    pub selected: Option<PathBuf>,
    pub buffers: crate::buffers::Buffers,

    /// The editor's text model. The view clones it into the field; edits come back here.
    pub text: TextEdit,
    pub loaded: Option<PathBuf>,
    /// The one reading of the text on screen. Rebuilt only when the text changes.
    pub status: ScriptTextStatus,
    pub frames: Vec<ScriptFrame>,
    pub spans: Vec<Span>,
    pub read_text: String,

    pub table: rune_kit::TableState,

    pub sidebar: rune_kit::shell::PaneState,
    pub inspector: rune_kit::shell::PaneState,
    /// The frames pane's height in points, and the body's last measured height.
    pub table_h: f32,
    pub body_h: f32,

    pub game: GameStatus,
    pub rules: PlaybackRules,
    pub seed: TextEdit,
    pub fps: TextEdit,
    pub run_error: Option<String>,
    pub run_message: Option<String>,
    pub preparing: bool,
    pub message: Option<String>,

    pub rename_text: TextEdit,
    pub rename_target: Option<PathBuf>,
    pub delete_target: Option<PathBuf>,
    pub commands_open: bool,
    pub mod_open: bool,

    pub poll: Poller,
    pub worker: Worker,
    pub headless: bool,
    pub headless_run: Option<u32>,
}

impl App {
    pub fn new() -> Self {
        let settings = Settings::load();
        let mut folder = settings.folder.clone();
        if folder.as_deref().is_none_or(|folder| !folder.is_dir()) {
            folder = Settings::first_folder();
        }

        let listing = workspace::list(folder.as_deref());
        let dark = !settings.light_theme;
        let kit = Kit::neutral(dark);
        let syntax = SyntaxTheme::from_kit(&kit);
        let rules = settings.rules;
        let seed = TextEdit::new(settings.seed_text.clone());
        let fps = TextEdit::new(rules.custom_fps.to_string());
        // Headless runs skip the network by default, so a snapshot is deterministic and does not
        // wait on a game that is not there. A test can opt back in with `DRMOD_RUNE_POLL=1` and a
        // mock mod on the port.
        let headless = rune::tooling::headless_run().is_some()
            && std::env::var_os("DRMOD_RUNE_POLL").is_none();

        let mut app = Self {
            kit,
            syntax,
            dark,
            settings,
            folder,
            listing,
            selected: None,
            buffers: Default::default(),
            text: TextEdit::new(""),
            loaded: None,
            status: ScriptTextStatus::of(""),
            frames: Vec::new(),
            spans: Vec::new(),
            read_text: String::new(),
            table: rune_kit::TableState::new().key("frames"),
            sidebar: rune_kit::shell::PaneState::new(230.0),
            inspector: rune_kit::shell::PaneState::new(330.0),
            table_h: 280.0,
            body_h: 844.0,
            game: GameStatus::offline(),
            rules,
            seed,
            fps,
            run_error: None,
            run_message: None,
            preparing: false,
            message: None,
            rename_text: TextEdit::new(""),
            rename_target: None,
            delete_target: None,
            commands_open: false,
            mod_open: false,
            poll: Poller::new(DEFAULT_URL),
            worker: Worker::new(),
            headless,
            headless_run: None,
        };

        if let Some(first) = app.listing.scripts.first() {
            let path = first.path.clone();
            app.select(path);
        }

        app
    }

    /// Reads the text on screen, once, when it changed.
    pub fn refresh_text(&mut self) {
        let text = self.text.text.clone();
        if self.read_text == text {
            return;
        }

        self.read_text = text.clone();
        self.status = ScriptTextStatus::of(&text);
        self.frames = if self.status.document.is_some() {
            projection::project(&text)
        } else {
            Vec::new()
        };
        self.spans = crate::highlight::spans(&text, &self.syntax);
    }

    pub fn selected_entry(&self) -> Option<&ScriptEntry> {
        let selected = self.selected.as_deref()?;
        self.listing
            .scripts
            .iter()
            .find(|entry| entry.path == selected)
    }

    pub fn dirty(&self) -> bool {
        self.selected_entry()
            .is_some_and(|entry| crate::buffers::is_dirty(&self.buffers, entry))
    }

    pub fn select(&mut self, path: PathBuf) {
        if self.loaded.as_deref() == Some(path.as_path()) {
            return;
        }

        let Some(entry) = self
            .listing
            .scripts
            .iter()
            .find(|entry| entry.path == path)
            .cloned()
        else {
            return;
        };

        let text = crate::buffers::resolve(&self.buffers, &entry);

        self.selected = Some(path.clone());
        self.loaded = Some(path);
        self.text = TextEdit::new(text);
        self.read_text.clear();
        self.refresh_text();
    }

    /// A keystroke landed in the editor: the model is the buffer now, and the reading follows.
    pub fn text_changed(&mut self) {
        if let Some(entry) = self.selected_entry().cloned() {
            crate::buffers::typed(&mut self.buffers, &entry, self.text.text.clone());
        }
        self.refresh_text();
        self.message = None;
    }

    pub fn reload_listing(&mut self) {
        self.listing = workspace::list(self.folder.as_deref());
        if let Some(selected) = &self.selected
            && !self
                .listing
                .scripts
                .iter()
                .any(|entry| &entry.path == selected)
        {
            self.clear_loaded();
        }
    }

    /// Nothing is selected: the editor goes back to its empty state rather than keeping the text
    /// of a file the list no longer has.
    pub fn clear_loaded(&mut self) {
        self.selected = None;
        self.loaded = None;
        self.text = TextEdit::new("");
        self.read_text.clear();
        self.refresh_text();
    }

    pub fn save_settings(&mut self) {
        self.settings.folder = self.folder.clone();
        self.settings.rules = self.rules;
        self.settings.seed_text = self.seed.text.clone();
        self.settings.light_theme = !self.dark;
        self.settings.save();
    }

    pub fn toggle_theme(&mut self) {
        self.dark = !self.dark;
        self.kit = Kit::neutral(self.dark);
        self.syntax = SyntaxTheme::from_kit(&self.kit);
        self.spans = crate::highlight::spans(&self.text.text, &self.syntax);
        self.settings.light_theme = !self.dark;
        self.settings.save();
    }

    pub fn can_run(&self) -> bool {
        self.game.online && !self.preparing && !self.game.script_active() && !self.worker.busy()
    }

    /// The rules, with the seed field read back. A half-typed seed is refused rather than
    /// silently replaced by zero.
    pub fn rules_with_seed(&self) -> Result<PlaybackRules, String> {
        let mut rules = self.rules;
        if rules.pin_seed {
            rules.seed = PlaybackRules::try_seed(&self.seed.text)
                .ok_or_else(|| "Сид должен быть числом: 123 или 0x7B".to_owned())?;
        }
        Ok(rules)
    }

    pub fn set_cap(&mut self, cap: FpsCapMode) {
        self.rules.cap = cap;
        self.save_settings();
    }

    pub fn poll(&mut self, _seconds: f64, tx: &mut Tx) -> bool {
        let mut changed = false;

        if let Some(status) = self.poll.take()
            && status != self.game
        {
            self.game = status;
            changed = true;
        }

        if !self.headless && self.poll.due() {
            self.poll.start();
        }

        if let Some(outcome) = self.worker.take() {
            self.absorb(outcome);
            changed = true;
        }

        if self.headless {
            return changed;
        }

        // Headless is armed only once the script is *really* running: the skip hooks sit on the
        // live device's draw calls, and putting them there while a level loads is what crashed the
        // game. The mod restores the render by itself when the run ends.
        if self.rules.headless
            && let Some(script) = &self.game.script
            && script.phase == GameScriptPhase::Running
            && self.headless_run != Some(script.id)
        {
            self.headless_run = Some(script.id);
            let api = crate::api::ModApi::new(DEFAULT_URL);
            let applied = api.post("/render", Some(&crate::api::ApiJson::headless(true)));
            if !applied.ok() {
                self.run_error = Some(format!("headless: {}", applied.message()));
                changed = true;
            }
        }

        if changed {
            tx.instant();
        }

        changed
    }

    pub fn on_key(&mut self, event: &KeyEvent, tx: &mut Tx) -> bool {
        if event.is_command('s') {
            self.save();
            return true;
        }

        if event.is_command('o') {
            actions::ask_folder(self, tx);
            return true;
        }

        if event.is_command('n') {
            self.new_script();
            return true;
        }

        if event.is_command('r') {
            self.run();
            return true;
        }

        false
    }

    fn absorb(&mut self, outcome: Outcome) {
        self.preparing = false;
        self.poll.kick();

        match outcome {
            Outcome::Levers { error } => {
                self.run_error = error.clone();
                self.run_message = error
                    .is_none()
                    .then(|| "Правила применены к игре".to_owned());
            }
            Outcome::Run {
                warning,
                error,
                message,
            } => {
                self.run_error = error.or(warning);
                self.run_message = message;
            }
            Outcome::Stop { error } => {
                self.run_error = error;
                self.run_message = Some("Прогон остановлен".to_owned());
            }
        }
    }

    /// The status line over the frames: the document's own summary, or the parser's refusal.
    pub fn script_summary(&self) -> String {
        match &self.status.document {
            Some(document) => format!(
                "{} · {} команд · последний кадр {}",
                document.name,
                document.commands.len(),
                ScriptTextStatus::last_frame(document)
            ),
            None => self
                .status
                .error
                .clone()
                .unwrap_or_else(|| "Нет скрипта".to_owned()),
        }
    }

    pub fn line_count(&self) -> usize {
        dsl::lines(&self.text.text).lines().count()
    }
}
