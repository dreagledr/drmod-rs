//! The frame period a run is pinned to, and the four levers a run applies.
//!
//! The rules are **not part of the script**. They are the mod's state for one run — the text
//! format says so itself (`docs/SCRIPT_DSL.md` §6) — which is why they live in the editor's own
//! settings and never in a `.tas` file: the same script can be run pinned or unpinned, and the
//! file must not be rewritten to say which.
//!
//! The defaults are the reproducible set the demo uses: the tick fixed at 1/60, the cap left as
//! the game keeps it, and the AI's decisions pinned to a frozen seed. Headless is off — it takes
//! the picture away, and that is not something to do behind the author's back.
//!
//! ⚠️ A run applies these; nothing here restores them afterwards. They are session state by
//! design: lifting the cap is what makes a run fast, and the mod is the one that puts the render
//! and the cap back when a headless run ends.

use super::ModApi;
use super::json::ApiJson;

/// The frame period a run is pinned to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpsCapMode {
    /// The cap the game keeps (50 in the pacer's 3 ms units = 60 FPS in gameplay, 30 in menus).
    Default,

    /// The cap lifted: with a fixed tick every frame is exactly 1/60 s of simulation, so the game
    /// runs faster than real time and a run is over sooner.
    Unlimited,

    /// A limit of the editor's own.
    Custom,
}

/// How a run is configured: the four levers the python tools set before a script goes out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackRules {
    pub fixed_tick: bool,
    pub cap: FpsCapMode,
    pub custom_fps: u32,
    pub pin_seed: bool,
    pub seed: u32,
    pub headless: bool,
}

impl Default for PlaybackRules {
    fn default() -> Self {
        Self {
            fixed_tick: true,
            cap: FpsCapMode::Default,
            custom_fps: 60,
            pin_seed: true,
            seed: 1,
            headless: false,
        }
    }
}

impl PlaybackRules {
    /// The tick the fixed period stands for: 1/60 s, the nominal the engine's own
    /// `cSlowRateManager` carries.
    pub const TICK_RATE: f32 = 60.0;

    /// The limits the mod enforces on `/fps` and `/rng` (`1..1000`); a value outside them would be
    /// answered `400`, so it is clamped here instead of being argued about.
    pub const MIN_FPS: u32 = 1;
    pub const MAX_FPS: u32 = 1000;

    /// `POST /fps` for these rules — the cap the run keeps.
    pub fn cap_body(&self) -> String {
        match self.cap {
            FpsCapMode::Unlimited => ApiJson::cap("off"),
            FpsCapMode::Custom => ApiJson::fps(self.custom_fps.clamp(Self::MIN_FPS, Self::MAX_FPS)),
            FpsCapMode::Default => ApiJson::cap("game"),
        }
    }

    /// `POST /dt` for these rules.
    pub fn dt_body(&self) -> String {
        ApiJson::dt(self.fixed_tick)
    }

    /// `POST /rng` for these rules.
    ///
    /// `freeze` and not `seed` on purpose: a frozen LCG answers every call as a function of its
    /// arguments, so the outcome stops depending on how the AI's threads interleave.
    pub fn seed_body(&self) -> String {
        if self.pin_seed {
            ApiJson::seed("freeze", Some(self.seed))
        } else {
            ApiJson::seed("off", None)
        }
    }

    /// What a run does to the mod, in the order it has to happen: the tick, the cap, then the
    /// seed.
    ///
    /// ⚠️ The seed goes **last**, immediately before the script: the mod freezes the LCG on the
    /// first tick of the next script, so a pin applied before anything else would land on nothing.
    /// An error comes back as a message — the caller paints it and does not start the run.
    pub fn apply(&self, api: &ModApi) -> Option<String> {
        for (path, body) in [
            ("/dt", self.dt_body()),
            ("/fps", self.cap_body()),
            ("/rng", self.seed_body()),
        ] {
            let answer = api.post(path, Some(&body));
            if !answer.ok() {
                return Some(format!("{path}: {}", answer.message()));
            }
        }

        None
    }

    /// The seed as the field spells it, read back as a number: decimal, or `0x`-prefixed hex —
    /// the spelling the reproducibility notes use.
    ///
    /// `false` for anything else, which is how a half-typed seed is told apart from one the run can
    /// use.
    pub fn try_seed(text: &str) -> Option<u32> {
        let value = text.trim();
        if value.is_empty() {
            return None;
        }

        match value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            Some(digits) => u32::from_str_radix(digits, 16).ok(),
            None => value.parse::<u32>().ok(),
        }
    }

    /// The rules as one line — what a run would set, for the panel to show before anything is sent.
    pub fn describe(&self) -> String {
        let cap = match self.cap {
            FpsCapMode::Unlimited => "unlimited".to_owned(),
            FpsCapMode::Custom => format!(
                "{} fps",
                self.custom_fps.clamp(Self::MIN_FPS, Self::MAX_FPS)
            ),
            FpsCapMode::Default => "default".to_owned(),
        };

        let seed = if self.pin_seed {
            format!("freeze {}", self.seed)
        } else {
            "off".to_owned()
        };

        format!(
            "fixed tick {} · cap {cap} · seed {seed} · headless {}",
            if self.fixed_tick { "1/60" } else { "off" },
            if self.headless { "on" } else { "off" }
        )
    }

    /// The cap's spelling as the settings file holds it, and the inverse.
    pub fn cap_spelling(&self) -> &'static str {
        match self.cap {
            FpsCapMode::Unlimited => "unlimited",
            FpsCapMode::Custom => "custom",
            FpsCapMode::Default => "default",
        }
    }

    /// Reads a cap back, accepting the panel's own words first and the mod's own spellings for
    /// files written while the two used to be the same.
    pub fn read_cap(value: &str) -> Option<FpsCapMode> {
        match value.to_lowercase().as_str() {
            "default" | "game" => Some(FpsCapMode::Default),
            "unlimited" | "off" | "uncapped" => Some(FpsCapMode::Unlimited),
            "custom" => Some(FpsCapMode::Custom),
            _ => None,
        }
    }
}
