//! The run rules: what a run sets, spelled the way the mod's endpoints read it.
//!
//! A port of the C# sibling's `PlaybackRulesTests.cs`.
//!
//! The bodies are asserted byte for byte on purpose — they are a contract with a program in another
//! repository (`src/api.rs`, and its readers in `docs/API.md`), and a field spelled `frames` instead
//! of `fps` is a `400` nobody would see until the game refused a run.

use drmod_tas_editor::api::{FpsCapMode, PlaybackRules};

#[test]
fn the_tick_is_pinned_to_one_sixtieth_or_left_to_the_game() {
    assert_eq!(
        PlaybackRules::default().dt_body(),
        r#"{"fixed":true,"ticks":true}"#
    );

    // Off: the mod restores its own measured delta, and the synthetic clocks are not asked for.
    assert_eq!(
        PlaybackRules {
            fixed_tick: false,
            ..PlaybackRules::default()
        }
        .dt_body(),
        r#"{"fixed":false}"#
    );
}

#[test]
fn the_frame_cap_is_the_games_default_unlimited_or_the_editors_own() {
    let rules = PlaybackRules::default();

    assert_eq!(rules.cap_body(), r#"{"cap":"game"}"#);

    assert_eq!(
        PlaybackRules {
            cap: FpsCapMode::Unlimited,
            ..rules
        }
        .cap_body(),
        r#"{"cap":"off"}"#
    );

    assert_eq!(
        PlaybackRules {
            cap: FpsCapMode::Custom,
            custom_fps: 144,
            ..rules
        }
        .cap_body(),
        r#"{"fps":144}"#
    );
}

#[test]
fn a_custom_cap_is_clamped_to_what_the_mod_accepts() {
    // The mod answers `400` outside `1..1000`; a number box allows any number, and a refusal over a
    // typed zero would be a run that never starts for a reason nobody can see.
    let rules = PlaybackRules {
        cap: FpsCapMode::Custom,
        ..PlaybackRules::default()
    };

    assert_eq!(
        PlaybackRules {
            custom_fps: 0,
            ..rules
        }
        .cap_body(),
        r#"{"fps":1}"#
    );

    assert_eq!(
        PlaybackRules {
            custom_fps: 5000,
            ..rules
        }
        .cap_body(),
        r#"{"fps":1000}"#
    );
}

#[test]
fn the_seed_is_frozen_with_its_number_or_handed_back_to_the_game() {
    let rules = PlaybackRules::default();

    assert_eq!(rules.seed_body(), r#"{"pin":"freeze","seed":1}"#);

    assert_eq!(
        PlaybackRules {
            seed: 0x55555555,
            ..rules
        }
        .seed_body(),
        r#"{"pin":"freeze","seed":1431655765}"#
    );

    // Unpinned: no seed at all — `freeze` is what carries one.
    assert_eq!(
        PlaybackRules {
            pin_seed: false,
            ..rules
        }
        .seed_body(),
        r#"{"pin":"off"}"#
    );
}

#[test]
fn a_seed_is_read_as_decimal_or_hex() {
    // The reproducibility notes spell their seeds in hex (`0x55555555`, `docs/API.md` §3.10), so the
    // field has to speak both — and to say "no" for anything in between, which is how a half-typed
    // seed is told apart from one a run can use.
    let cases: [(&str, Option<u32>); 10] = [
        ("1", Some(1)),
        (" 42 ", Some(42)),
        ("0x55555555", Some(1431655765)),
        ("0X10", Some(16)),
        ("4294967295", Some(4294967295)),
        ("", None),
        ("zz", None),
        ("0x", None),
        ("0xZZ", None),
        ("-1", None),
    ];

    for (typed, expected) in cases {
        assert_eq!(
            PlaybackRules::try_seed(typed),
            expected,
            "{typed:?} reads as {expected:?}"
        );
    }
}

#[test]
fn the_rules_read_as_one_line() {
    let rules = PlaybackRules::default();

    assert_eq!(
        rules.describe(),
        "fixed tick 1/60 · cap default · seed freeze 1 · headless off"
    );

    assert_eq!(
        PlaybackRules {
            fixed_tick: false,
            cap: FpsCapMode::Unlimited,
            pin_seed: false,
            headless: true,
            ..rules
        }
        .describe(),
        "fixed tick off · cap unlimited · seed off · headless on"
    );

    assert_eq!(
        PlaybackRules {
            cap: FpsCapMode::Custom,
            custom_fps: 144,
            ..rules
        }
        .describe(),
        "fixed tick 1/60 · cap 144 fps · seed freeze 1 · headless off"
    );
}
