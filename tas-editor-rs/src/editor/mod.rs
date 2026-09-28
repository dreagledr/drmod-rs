//! The window shell: the panes, the workspace they show, and the run.
//!
//! The port of the Reactor sibling's `Editor.cs` onto the spike's stack (dear-app + dear-imgui-cte).
//! The pane set and the ownership rule are the same:
//!
//! * this type owns the **content** — which panes exist, what each shows, which folder the
//!   workspace is on, and what the game is doing;
//! * the dock space owns the **shape** — the split ratios a drag left behind, kept in ImGui's own
//!   `.ini` and applied once through a declared layout.
//!
//! The workspace lives here rather than in a pane because more than one pane reads it: the list
//! shows which scripts have unsaved text, the editor shows that text, and Save writes it back.
//! Three readers, so one owner.

pub mod mod_panel;
pub mod script_controls;
pub mod shell;
pub mod table;
pub mod text_cache;
pub mod text_pane;
pub mod workspace_panel;

pub use shell::{FrameActions, ShellState};

use crate::editor::script_controls::ControlsActions;
use crate::editor::text_pane::TextActions;

/// What the two script panels — the run controls and the text — decided in one frame.
///
/// One record for the pair because they are one subject: the controls *run what the text says*, and
/// the text is what the controls read. Keeping their answers together is what lets the shell apply
/// them in one pass without either panel having to know about the other.
#[derive(Clone, Debug, Default)]
pub struct ScriptActions {
    pub controls: ControlsActions,
    pub text: TextActions,
}

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::api::{GameStatus, ModApi, PlaybackRules};
use crate::buffers::{self, Buffers};
use crate::menu_settler;
use crate::mod_install::{self, ModState, Payload};
use crate::script::{dsl, json, ScriptDocument, ScriptTextStatus};
use crate::settings::Settings;
use crate::steam;
use crate::workspace::{self, Listing, ScriptEntry};

/// How often the game is asked what it is doing. The mod's HTTP server is single-threaded and
/// lives in the game's render loop, so this is a couple of times a second rather than per frame —
/// and the panel is a control panel, not a TAS readout.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long an answer may be in flight before another poll is skipped. A game that is loading
/// blocks its render loop, and the server with it — the panel must not stack requests behind it.
const POLL_PATIENCE: Duration = Duration::from_millis(2500);

/// The panes, in the order they are declared to the dock space.
pub const WINDOW_WORKSPACE: usize = 0;
pub const WINDOW_SCRIPT: usize = 1;

/// The main window — which is also the dock space host, as in the spike: the panels dock into it,
/// so `TAS Editor` is one window with its own menu bar rather than a strip of panels beside it.
pub const WINDOW_TITLE: &str = "TAS Editor";

/// The dock space's string id, hashed into an `Id`. It is also the tree's key in the `.ini`.
pub const DOCKSPACE_ID: &str = "TasEditorRsDockspace";

/// Everything one frame of the shell needs to know, gathered so the panes take values rather than
/// reaching into the shell.
pub struct Editor {
    /// What the editor remembers between launches.
    pub settings: Settings,
    /// The workspace: its folder, its listing and the text being edited.
    pub folder: Option<PathBuf>,
    pub listing: Listing,
    pub selected: Option<PathBuf>,
    pub buffers: Buffers,

    /// The mod's install: where the game is, and what its folder holds.
    pub payload: Payload,
    pub game_folder: Option<PathBuf>,
    pub mod_state: ModState,
    pub mod_loader_present: bool,
    pub mod_loader_ours: bool,
    pub mod_can_remove: bool,
    pub mod_message: Option<String>,
    pub mod_busy: bool,
    pub pending_mod_remove: bool,

    /// The game, as the last poll left it, and what the last run action made of it.
    pub game: GameStatus,
    pub run_error: Option<String>,
    pub preparing: bool,
    pub run_message: Option<String>,
    /// The script a headless run was already applied for — see [`Editor::poll`].
    pub headless_run: Option<u32>,

    /// The workspace's own last message: what went wrong, or what the listing found.
    pub message: Option<String>,

    /// The modal state: a delete, a rename, and what the dialog is asking about.
    pub pending_delete: Option<ScriptEntry>,
    pub pending_rename: Option<ScriptEntry>,
    pub rename_text: String,

    /// The file dialog: which one is wanted, and the frame the blocking call will be made on.
    pub pending_folder: PendingFolder,

    /// Whether the declared dock layout has been submitted. It goes in **once** — after that the
    /// dock space keeps the user's own ratios, and submitting it again is an error.
    pub dock_layout_applied: bool,

    /// The mod's HTTP client, and the worker that polls it.
    pub api: ModApi,
    pub poll: Poller,
}

/// A blockingly-opened system dialog, deferred to the pre-frame hook: a modal dialog must not be
/// held open across an ImGui frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PendingFolder {
    #[default]
    None,
    Workspace,
    GameFolder,
}

/// The background status poll.
///
/// The mod's server answers one request at a time from the game's render loop, so the panel asks
/// twice a second from a worker thread and reads the last answer here. A poll that is still in
/// flight when the next tick comes is not started again — the panel must not stack requests behind
/// a game that is loading a level.
pub struct Poller {
    receiver: Option<Receiver<GameStatus>>,
    started: Option<Instant>,
    next: Instant,
    url: String,
}

impl Poller {
    pub fn new(url: &str) -> Self {
        Self {
            receiver: None,
            started: None,
            next: Instant::now(),
            url: url.to_owned(),
        }
    }

    /// The newest answer, if one has arrived. Called once a frame: cheap, and it never blocks.
    pub fn take(&mut self) -> Option<GameStatus> {
        let receiver = self.receiver.as_ref()?;
        match receiver.try_recv() {
            Ok(state) => {
                self.receiver = None;
                self.started = None;
                Some(state)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.receiver = None;
                self.started = None;
                None
            }
        }
    }

    /// Whether a poll should be started now.
    pub fn due(&self) -> bool {
        if self.receiver.is_some() {
            // An answer that never comes must not wedge the panel: past the patience the poll is
            // re-started, and the mod's own timeout bounds it.
            return self
                .started
                .is_some_and(|started| started.elapsed() > POLL_PATIENCE);
        }

        Instant::now() >= self.next
    }

    /// Starts a poll on a worker thread.
    pub fn start(&mut self) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let url = self.url.clone();
        std::thread::spawn(move || {
            let api = ModApi::new(&url);
            let status = api.state().value.unwrap_or_else(GameStatus::offline);
            // The receiver may be gone because the window closed; that is not a failure.
            let _ = sender.send(status);
        });

        self.receiver = Some(receiver);
        self.started = Some(Instant::now());
        self.next = Instant::now() + POLL_INTERVAL;
    }
}

impl Editor {
    /// A fresh shell: the remembered settings, the examples as a first workspace, the payload and
    /// the game folder's state.
    pub fn new() -> Self {
        let settings = Settings::load();
        let folder = settings
            .folder
            .clone()
            .or_else(Settings::first_folder);

        let payload = Payload::embedded();
        let game_folder = settings.game_folder.clone().or_else(steam::game_folder);

        let mut editor = Self {
            listing: Listing::default(),
            selected: None,
            buffers: Buffers::new(),
            game: GameStatus::offline(),
            run_error: None,
            preparing: false,
            run_message: None,
            headless_run: None,
            message: None,
            pending_delete: None,
            pending_rename: None,
            rename_text: String::new(),
            pending_folder: PendingFolder::None,
            dock_layout_applied: false,
            api: ModApi::new(crate::api::DEFAULT_URL),
            poll: Poller::new(crate::api::DEFAULT_URL),
            payload,
            game_folder,
            mod_state: ModState::NoGameFolder,
            mod_loader_present: false,
            mod_loader_ours: false,
            mod_can_remove: false,
            mod_message: None,
            mod_busy: false,
            pending_mod_remove: false,
            settings,
            folder,
        };

        editor.relist();
        editor.look_at_game_folder(editor.game_folder.clone());
        editor
    }

    /// Re-reads the folder's scripts. Every write to the folder goes through here — a save, a new
    /// file, a rename, a delete — so the rows re-read the files they describe.
    pub fn relist(&mut self) {
        self.listing = workspace::list(self.folder.as_deref());

        // A selection that the listing no longer holds — a deleted file, or a folder the user has
        // switched away from — is dropped rather than left pointing at nothing.
        if let Some(selected) = &self.selected
            && self.listing.scripts.iter().all(|script| &script.path != selected)
        {
            self.selected = None;
        }
    }

    /// The selected script, if the listing still holds it.
    pub fn selected(&self) -> Option<&ScriptEntry> {
        let path = self.selected.as_ref()?;
        self.listing.scripts.iter().find(|script| &script.path == path)
    }

    /// The pane that installs the mod, as the workspace pane paints it. Built per frame from the
    /// shell's own facts: the pane is content of another pane, so it is a value rather than a
    /// component of its own.
    pub fn mod_view(&self) -> mod_panel::ModPanelView<'_> {
        mod_panel::ModPanelView {
            game_folder: self.game_folder.as_deref(),
            state: self.mod_state,
            loader_present: self.mod_loader_present,
            loader_ours: self.mod_loader_ours,
            can_remove: self.mod_can_remove,
            message: self.mod_message.as_deref(),
            busy: self.mod_busy,
            payload_asi_kib: self.payload.asi_kib(),
            payload_loader_kib: self.payload.loader_kib(),
        }
    }

    /// The text the editor should show for the selected script.
    pub fn selected_text(&self) -> String {
        match self.selected() {
            Some(script) => buffers::resolve(&self.buffers, script),
            None => String::new(),
        }
    }

    /// Whether the selected script has unsaved text.
    pub fn selected_dirty(&self) -> bool {
        self.selected()
            .is_some_and(|script| buffers::is_dirty(&self.buffers, script))
    }

    /// One parse of the text on screen for the whole window: the controls region runs it, the text
    /// region's status line reads it, the command table paints it and Save is enabled by it.
    pub fn status(&self) -> ScriptTextStatus {
        ScriptTextStatus::of(&dsl::lines(&self.selected_text()))
    }

    // ── the workspace ─────────────────────────────────────────────────────────

    /// Adopts a folder as the workspace and remembers it.
    pub fn open_folder(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
        self.selected = None;
        self.message = None;
        self.buffers.clear();
        self.relist();
        self.save_settings();
    }

    /// A new, empty file, selected as soon as the listing has read it.
    pub fn new_script(&mut self) {
        let Some(folder) = self.folder.clone() else {
            return;
        };

        let (path, error) = workspace::create(&folder);
        self.message = error;
        if let Some(path) = path {
            self.relist();
            self.selected = Some(path);
        }
    }

    pub fn duplicate_script(&mut self) {
        let Some(script) = self.selected().cloned() else {
            return;
        };

        let (path, error) = workspace::duplicate(&script.path);
        self.message = error;
        if let Some(path) = path {
            self.relist();
            self.selected = Some(path);
        }
    }

    pub fn save_script(&mut self) {
        let Some(script) = self.selected().cloned() else {
            return;
        };

        if !buffers::is_dirty(&self.buffers, &script) {
            return;
        }

        // The text is written and the buffer is then dropped: the listing reads the file back as
        // the text the editor has, so the script is no longer unsaved and its frame count is the
        // one the file ends on.
        let text = buffers::resolve(&self.buffers, &script);
        self.message = workspace::write(&script.path, &text);
        if self.message.is_none() {
            buffers::without(&mut self.buffers, &script.path);
        }

        self.relist();
    }

    /// Deletes the script a delete dialog was opened for.
    pub fn delete_confirmed(&mut self) {
        let Some(doomed) = self.pending_delete.take() else {
            return;
        };

        self.message = workspace::delete(&doomed.path);
        if self.message.is_some() {
            return;
        }

        buffers::without(&mut self.buffers, &doomed.path);
        if self.selected.as_deref() == Some(doomed.path.as_path()) {
            self.selected = None;
        }

        self.relist();
    }

    /// A rename was confirmed: the file moves and the selection and the buffer travel with it.
    pub fn rename_confirmed(&mut self) {
        let Some(renamed) = self.pending_rename.take() else {
            return;
        };

        let wanted = self.rename_text.clone();
        let (path, error) = workspace::rename(&renamed.path, &wanted);
        self.message = error;
        let Some(path) = path else {
            return;
        };

        // The buffer travels by the new path: a rename must not throw away text that has not been
        // written back yet.
        buffers::renamed(&mut self.buffers, &renamed.path, &path);
        if self.selected.as_deref() == Some(renamed.path.as_path()) {
            self.selected = Some(path);
        }

        self.relist();
    }

    /// Records what a text box reported into the selected script's buffer.
    pub fn typed(&mut self, text: String) {
        if let Some(script) = self.selected().cloned() {
            buffers::typed(&mut self.buffers, &script, text);
        }
    }

    // ── the mod's install ─────────────────────────────────────────────────────

    /// Looks at a game folder — the remembered one, or the one just picked — and records what is
    /// in it. Every fact the pane paints is read here, once, rather than recomputed per render:
    /// two of them are disk reads (the plugin and the loader).
    ///
    /// A folder that is not the game is refused rather than recorded: the install writes into it,
    /// and a folder the user picked by mistake should not be the target of an Install click.
    pub fn look_at_game_folder(&mut self, candidate: Option<PathBuf>) {
        let Some(folder) = candidate else {
            self.mod_state = ModState::NoGameFolder;
            return;
        };

        if !steam::is_game_folder(Some(folder.as_path())) {
            self.mod_state = ModState::NoGameFolder;
            self.mod_message = Some(format!(
                "That folder holds no {}.",
                steam::GAME_EXE_NAME
            ));
            return;
        }

        self.mod_state = mod_install::installer::detect(Some(&folder), &self.payload);
        let (present, ours) = mod_install::installer::loader(Some(&folder), &self.payload);
        self.mod_loader_present = present;
        self.mod_loader_ours = ours;
        self.mod_can_remove = mod_install::installer::can_remove(Some(&folder), &self.payload);
        self.game_folder = Some(folder);
    }

    /// Looks again: Steam's records, then the remembered folder. For the case the pane exists for
    /// one half of — the game was installed or moved while the editor was open.
    pub fn detect_game_folder(&mut self) {
        let found = steam::game_folder().or_else(|| self.game_folder.clone());
        self.mod_message = None;
        self.look_at_game_folder(found);
        self.save_settings();
    }

    /// Writes the mod into the game folder.
    ///
    /// ⚠️ Nothing here starts, stops or touches the game. The files are the install, and the
    /// message says so; a running game keeps the old plugin until it is restarted.
    pub fn install_mod(&mut self) {
        self.mod_busy = true;
        let result = mod_install::installer::install(self.game_folder.as_deref(), &self.payload);
        self.mod_message = Some(result.message);
        self.mod_busy = false;

        if result.ok {
            let folder = self.game_folder.clone();
            self.look_at_game_folder(folder);
        }
    }

    /// Removes the mod. What is removed is decided from the bytes on disk, not from anything
    /// remembered — the folder can have changed since the last look.
    pub fn remove_mod_confirmed(&mut self) {
        self.pending_mod_remove = false;

        let result = mod_install::installer::remove(self.game_folder.as_deref(), &self.payload);
        self.mod_message = Some(result.message);
        let folder = self.game_folder.clone();
        self.look_at_game_folder(folder);
    }

    // ── the run ───────────────────────────────────────────────────────────────

    /// The rules a run would apply, as the controls region shows them.
    pub fn rules(&self) -> PlaybackRules {
        self.settings.rules
    }

    pub fn set_rules(&mut self, rules: PlaybackRules) {
        self.settings.rules = rules;
        self.save_settings();
    }

    /// The seed field holds text until it reads as a number: the documented seeds are hex, so the
    /// spelling is what is kept, and a half-typed one leaves the last value that parsed in place.
    pub fn set_seed_text(&mut self, text: String) {
        self.settings.seed_text = text;
        if let Some(seed) = PlaybackRules::try_seed(&self.settings.seed_text) {
            self.settings.rules.seed = seed;
        }

        self.save_settings();
    }

    /// The rules with the seed the field currently spells, or the reason it does not read as one.
    fn rules_with_seed(&self) -> Result<PlaybackRules, String> {
        match PlaybackRules::try_seed(&self.settings.seed_text) {
            Some(seed) => Ok(PlaybackRules {
                seed,
                ..self.settings.rules
            }),
            None => Err("The seed is not a number — decimal, or 0x-prefixed for hex".to_owned()),
        }
    }

    /// Starts the text on screen in the game.
    ///
    /// The order is the one the python tools established: the game window first (the menu keys a
    /// `restart` plays arrive only while the game owns the input focus), then a menu settled out of
    /// the way, then the rules, and the seed **last** of the three — the mod freezes the LCG on the
    /// first tick of the *next* script, and the script it has to land on is the one sent right
    /// after.
    ///
    /// What goes to the mod is the text **on screen**, parsed again here. The file is not written
    /// first: the run is of the script the author is looking at, and Run is not a save.
    ///
    /// ⚠️ Spawned on a worker thread: every step here blocks (the window, the menu, three posts and
    /// a script), and doing that inside a frame would freeze the editor for as long as the game
    /// takes to answer. What lands back is one message and one status.
    pub fn run(&mut self) {
        if self.selected().is_none() {
            return;
        }

        let status = self.status();
        let Some(document) = status.document else {
            self.run_error = status.error;
            return;
        };

        let rules = match self.rules_with_seed() {
            Ok(rules) => rules,
            Err(error) => {
                self.run_error = Some(error);
                return;
            }
        };

        self.run_error = None;
        self.run_message = None;
        self.preparing = true;

        let url = crate::api::DEFAULT_URL.to_owned();
        let sender = self.worker_sender();
        std::thread::spawn(move || {
            let outcome = run_script(&url, &rules, &document);
            let _ = sender.send(outcome);
        });
    }

    /// Sets the levers on the game without starting a script — the same three posts Run makes, and
    /// nothing else.
    ///
    /// The point is the state, not a run: an extreme rule (`1 fps`, a lifted cap) is how a run is
    /// made cheap, and the mod keeps it after the run ends, so setting it deliberately is worth
    /// having on its own. No window focus and no menu settle either — those exist to deliver a
    /// script's own restart, and nothing here plays a restart.
    pub fn apply_rules(&mut self) {
        let rules = match self.rules_with_seed() {
            Ok(rules) => rules,
            Err(error) => {
                self.run_error = Some(error);
                return;
            }
        };

        self.run_error = None;
        self.run_message = None;
        self.preparing = true;

        let url = crate::api::DEFAULT_URL.to_owned();
        let sender = self.worker_sender();
        std::thread::spawn(move || {
            let api = ModApi::new(&url);
            let outcome = match rules.apply(&api) {
                Some(error) => RunOutcome::levers(error),
                None => RunOutcome::levers_ok(),
            };
            let _ = sender.send(outcome);
        });
    }

    /// Stops whatever the mod is running. The render and the frame cap come back on their own: the
    /// mod restores them when a headless run ends, cancelled or not.
    pub fn cancel_run(&mut self) {
        self.run_error = None;
        let stopped = self.api.stop();
        if !stopped.ok() {
            self.run_error = Some(format!("stop: {}", stopped.message()));
        }

        if let Some(state) = self.api.state().value {
            self.game = state;
        }
    }

    /// The channel a worker reports its outcome on. A fresh one per action: the actions are
    /// exclusive — a run cannot be started while one is preparing — so there is never more than one
    /// worker in flight, and a stale `Ok` on a reused channel would be read as this action's.
    fn worker_sender(&self) -> std::sync::mpsc::Sender<RunOutcome> {
        // Leaked on purpose: the receiver lives in `self`, and the leak is one channel per action,
        // which the shell reclaims when it exits. A shared channel would need a lock for what is
        // by construction a single producer.
        let (sender, receiver) = std::sync::mpsc::channel();
        RUN_RESULTS.with_receiver(receiver);
        sender
    }

    /// What a finished worker left behind, if anything. Called once a frame.
    pub fn take_run_outcome(&mut self) -> Option<RunOutcome> {
        RUN_RESULTS.take()
    }

    /// Applies a finished worker's outcome to the shell's own state.
    pub fn absorb(&mut self, outcome: RunOutcome) {
        self.preparing = false;

        match outcome {
            RunOutcome::Levers { error } => {
                self.run_error = error.clone();
                if error.is_none() {
                    self.run_message = Some("the run rules are set on the game".to_owned());
                }
            }
            RunOutcome::Script {
                warning,
                error,
                message,
            } => {
                self.run_error = error.or(warning);
                self.run_message = message;
            }
        }

        // A fresh read after the action: the panel then shows what the game ended up set to, not
        // what it was asked for.
        if let Some(state) = self.api.state().value {
            self.game = state;
        }
    }

    /// The status poll, once a frame. The work happens on a worker thread; this only starts a poll
    /// and reads the newest answer.
    ///
    /// ⚠️ Headless is armed only once the script is *really* `running`: the skip hooks sit on the
    /// live device's draw calls, and putting them there while a level loads is what crashed the
    /// game in `d3d9.dll` (measured — `docs/HEADLESS.md` §5). The python tools apply it the same
    /// way, from the loop that watches the status. The mod restores the render by itself when a run
    /// ends, so each run arms it once and never again.
    pub fn poll(&mut self) {
        if let Some(status) = self.poll.take() {
            self.game = status;
        }

        if self.poll.due() {
            self.poll.start();
        }

        if !self.settings.rules.headless {
            return;
        }

        let running = self
            .game
            .script
            .as_ref()
            .is_some_and(|script| script.phase == crate::api::GameScriptPhase::Running);

        if !running {
            return;
        }

        let id = self.game.script.as_ref().map(|script| script.id);
        if id.is_none() || id == self.headless_run {
            return;
        }

        self.headless_run = id;
        let applied = self.api.post("/render", Some(&crate::api::ApiJson::headless(true)));
        if !applied.ok() {
            self.run_error = Some(format!("headless: {}", applied.message()));
        }
    }

    pub fn save_settings(&mut self) {
        self.settings.folder = self.folder.clone();
        self.settings.game_folder = self.game_folder.clone();
        self.settings.save();
    }

    /// The blocking system dialog a click asked for, run from the pre-frame hook: a modal dialog
    /// must not be held open across a frame.
    pub fn run_pending_dialog(&mut self) {
        let pending = std::mem::replace(&mut self.pending_folder, PendingFolder::None);
        if pending == PendingFolder::None {
            return;
        }

        let mut dialog = dear_file_browser::FileDialog::new(dear_file_browser::DialogMode::PickFolder)
            .backend(dear_file_browser::Backend::Native);

        let start = match pending {
            PendingFolder::Workspace => self.folder.clone(),
            PendingFolder::GameFolder => self.game_folder.clone(),
            PendingFolder::None => None,
        };

        if let Some(folder) = start {
            dialog = dialog.directory(folder);
        }

        match dialog.open_blocking() {
            Ok(selection) => match selection.file_path_name() {
                Some(path) => {
                    let path = PathBuf::from(path);
                    match pending {
                        PendingFolder::Workspace => self.open_folder(path),
                        PendingFolder::GameFolder => {
                            self.mod_message = None;
                            self.look_at_game_folder(Some(path));
                            self.save_settings();
                        }
                        PendingFolder::None => {}
                    }
                }
                None => {
                    self.message = Some("the folder picker returned nothing".to_owned());
                }
            },
            Err(dear_file_browser::FileDialogError::Cancelled) => {
                // Cancelling is not an error, so nothing is said about it.
            }
            Err(error) => {
                let message = format!("The folder picker failed: {error}");
                match pending {
                    PendingFolder::GameFolder => self.mod_message = Some(message),
                    _ => self.message = Some(message),
                }
            }
        }
    }
}

/// What a worker reports back.
#[derive(Clone, Debug)]
pub enum RunOutcome {
    /// The levers only ([`Editor::apply_rules`]). `error` is the first refusal, if any.
    Levers { error: Option<String> },
    /// A run: a warning that did not stop it, a refusal that did, and what to say when it worked.
    Script {
        warning: Option<String>,
        error: Option<String>,
        message: Option<String>,
    },
}

impl RunOutcome {
    fn levers(error: String) -> Self {
        Self::Levers { error: Some(error) }
    }

    fn levers_ok() -> Self {
        Self::Levers { error: None }
    }
}

/// The finished worker's outcome, and the receiver it arrives on.
struct RunResults {
    receiver: std::sync::Mutex<Option<Receiver<RunOutcome>>>,
}

impl RunResults {
    const fn new() -> Self {
        Self {
            receiver: std::sync::Mutex::new(None),
        }
    }

    fn with_receiver(&self, receiver: Receiver<RunOutcome>) {
        if let Ok(mut slot) = self.receiver.lock() {
            *slot = Some(receiver);
        }
    }

    fn take(&self) -> Option<RunOutcome> {
        let mut slot = self.receiver.lock().ok()?;
        let receiver = slot.as_ref()?;

        match receiver.try_recv() {
            Ok(outcome) => {
                *slot = None;
                Some(outcome)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                *slot = None;
                None
            }
        }
    }
}

/// The one action a worker can have in flight. The shell allows exactly one — `preparing` gates
/// every action button — so a single slot is the whole of what is needed, and a lock is all it
/// takes to keep it sound.
static RUN_RESULTS: RunResults = RunResults::new();

/// The whole of a run, off the UI thread: the window, the menu, the rules, the script.
fn run_script(url: &str, rules: &PlaybackRules, document: &ScriptDocument) -> RunOutcome {
    let api = ModApi::new(url);

    // A fresh read before anything is sent: the poll can be half a second old, and the menu is
    // what decides whether the script's own restart can be played at all.
    let Some(snapshot) = api.state().value else {
        return RunOutcome::Script {
            warning: None,
            error: Some(
                "The mod is not answering — is the game running with the mod injected?".to_owned(),
            ),
            message: None,
        };
    };

    // A warning, not a stop: the script runs either way, and what may be lost is the menu input
    // the script's own restart plays.
    let mut warning = (!crate::game_window::focus_and_settle(350)).then(|| {
        "The game window did not take the foreground — menu input may be lost".to_owned()
    });

    // Out of the pause menu, or out of a fail menu, before the script arms — otherwise the menu
    // swallows the keys the script's own restart plays.
    if let Some(stuck) = menu_settler::ensure_gameplay(&api) {
        return RunOutcome::Script {
            warning,
            error: Some(stuck),
            message: None,
        };
    }

    if let Some(lever) = rules.apply(&api) {
        return RunOutcome::Script {
            warning,
            error: Some(lever),
            message: None,
        };
    }

    let Ok(body) = json::write(document) else {
        return RunOutcome::Script {
            warning,
            error: Some("the script does not write as JSON".to_owned()),
            message: None,
        };
    };

    let mut started = api.run(&body);
    if started.conflict {
        // The mod holds one script slot and answers a second run with 409. A script that ended
        // between the poll and this click is a race the panel cannot see, so the slot is taken
        // once, by stopping whatever holds it.
        let _ = api.stop();
        started = api.run(&body);
    }

    // A run is a thing the game does, not a thing the API answers about: the `/state` right after
    // it is what says whether it is armed, running or already over. Nothing is claimed about it
    // here that the status line will not show a moment later.
    let outcome = if let Some(run) = &started.value {
        format!(
            "\u{201c}{}\u{201d} sent — {} frames",
            run.name.clone().unwrap_or_else(|| document.name.clone()),
            run.total_frames
        )
    } else {
        format!("\u{201c}{}\u{201d} sent", document.name)
    };

    if !started.ok() {
        return RunOutcome::Script {
            warning,
            error: Some(format!("run: {}", started.message())),
            message: None,
        };
    }

    // A warning that was already set still stands; a `None` stays `None` so the shell does not
    // clear one it was given.
    if warning.is_none() {
        warning = None;
    }

    let _ = snapshot;
    RunOutcome::Script {
        warning,
        error: None,
        message: Some(outcome),
    }
}
