//! The port's contract with the format: the same script in, the same text and the same JSON out,
//! byte for byte.
//!
//! These fixtures were the C# editor's own — copied from its `TasEditorCs.Tests/Fixtures/`, where
//! `ScriptGoldenTests.cs` asserted the same pairs. ⚠️ That editor has since been removed from the
//! repository; the fixtures here are now the only copy, and `tests/fixtures/golden/` is the
//! canonical location. That still makes this the strongest check the port has: not "the format reads
//! back as itself", which a private dialect would also pass, but "the text and the JSON written here
//! are exactly the ones the format was pinned to".
//!
//! The three assertions are the C# test's own, and they say exactly how much each view can lose:
//!
//! 1. **the JSON round-trips into its golden** — the JSON is the source of truth, so this is the
//!    strict one;
//! 2. **the text round-trips into its golden**, and reading a golden text back writes the same text
//!    again — a golden is what a user edits, and every round trip through the editor must leave the
//!    file alone;
//! 3. **the text and the JSON are the same script** — compared as the rules line plus the *frames*
//!    each expands to, and **not** as documents, because the text moves with the stick while the
//!    JSON moves with the direction flags. Both drive the character; the four movement bits are
//!    what the two forms are allowed to differ in.
//!
//! ⚠️ The goldens are the **reference, not generator output**: a format change means editing them by
//! hand alongside the fixtures. `drmod-script-gen` regenerates only the inputs (`*.json`) from the
//! shared DTOs (`drmod-script-gen/README.md`).

use drmod_tas_editor::script::frames;
use drmod_tas_editor::script::{dsl, json, ScriptDocument};

/// The fixture names, extension included — the C# `ScriptFixtures.Fixtures()` list.
const FIXTURES: [&str; 5] = [
    "all_inputs.json",
    "edge_inputs.json",
    "minimal.json",
    "rules_restart.json",
    "rules_ticks.json",
];

fn read(path: &str) -> String {
    let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden")
        .join(path);

    std::fs::read_to_string(&full)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", full.display()))
}

/// A fixture that may not be there — the goldens a fixture has no form for.
fn optionally(path: &str) -> Option<String> {
    let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden")
        .join(path);

    std::fs::read_to_string(full).ok()
}

fn golden_name(fixture: &str, extension: &str) -> String {
    let stem = fixture.strip_suffix(".json").unwrap_or(fixture);
    format!("{stem}.expected.{extension}")
}

/// The JSON is the source of truth, so it is what goes in: `read` applies the mod's own cross-field
/// limits, which is what makes a fixture that passes here a script the game would take.
fn document(fixture: &str) -> ScriptDocument {
    json::read(&read(fixture)).unwrap_or_else(|refused| {
        panic!(
            "{fixture}: the fixture is not a script: {}",
            refused.message()
        )
    })
}

/// Whether the text format can say this script at all — `raw_key`, `dik_key` and `when_enemy` have
/// no DSL spelling, so a fixture holding one has no text and must not pretend to have a golden.
fn expressible(document: &ScriptDocument) -> bool {
    dsl::write(document).is_ok()
}

#[test]
fn the_json_of_every_fixture_writes_back_into_its_golden() {
    for fixture in FIXTURES {
        let expected = read(&golden_name(fixture, "json"));
        let written = json::write(&document(fixture)).unwrap_or_else(|refused| {
            panic!("{fixture}: does not write as JSON: {}", refused.message())
        });

        assert_eq!(
            written, expected,
            "{fixture}: the JSON differs from the C# editor's"
        );
    }
}

#[test]
fn the_text_of_every_expressible_fixture_writes_into_its_golden() {
    for fixture in FIXTURES {
        let script = document(fixture);
        let golden = golden_name(fixture, "tas");

        if !expressible(&script) {
            assert!(
                optionally(&golden).is_none(),
                "{golden} exists, but the script has no text form"
            );
            continue;
        }

        let expected = read(&golden);
        let text = dsl::write(&script)
            .unwrap_or_else(|refused| panic!("{fixture}: does not write: {}", refused.message()));

        assert_eq!(text, expected, "{fixture}: the text differs from the C# editor's");

        // A golden is what a user edits: reading it back has to give the same text, or every round
        // trip through the editor would rewrite the file.
        let again = dsl::parse(&text)
            .unwrap_or_else(|refused| panic!("{fixture}: its own text does not parse: {}", refused.message()));
        assert_eq!(
            dsl::write(&again).expect("the re-parse writes"),
            text,
            "{fixture}: writing the parsed golden changed it"
        );
    }
}

#[test]
fn the_text_of_a_fixture_is_the_same_script_as_its_json() {
    for fixture in FIXTURES {
        let script = document(fixture);
        if !expressible(&script) {
            continue;
        }

        let text = dsl::write(&script).expect("it is expressible");
        let from_text = dsl::parse(&text).unwrap_or_else(|refused| {
            panic!("{fixture}: its own text does not parse: {}", refused.message())
        });

        // The rules line is a lossless spelling of name/trigger/restart, so comparing the written
        // rules compares the fields themselves.
        assert_eq!(
            rules_line(&script),
            rules_line(&from_text),
            "{fixture}: the rules differ"
        );

        // The frames are what the two forms must agree on, movement aside.
        assert_same_frames(fixture, &frames::expand(&script), &frames::expand(&from_text));
    }
}

#[test]
fn the_table_of_the_input_fixture_covers_every_command() {
    // The last command starts at frame 45 and lasts five frames, so the table is 50 rows.
    assert_eq!(frames::expand(&document("all_inputs.json")).len(), 50);
}

/// The rules line of a document's text — everything the text says that is not a frame.
fn rules_line(document: &ScriptDocument) -> String {
    dsl::write(document)
        .expect("the caller checked expressibility")
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The two frame lists agree, except in the four movement bits.
///
/// ⚠️ The movement bits are the one thing the two forms are *allowed* to differ in, and the
/// exception is not a loophole: the text has no token for them at all (movement is the stick), so a
/// `forward: true` in the JSON has nowhere to go in the text but the stick it means. Both drive the
/// character (`docs/API.md` §10.2), which is what makes the pair the same script.
fn assert_same_frames(
    fixture: &str,
    json_frames: &[frames::CommandRow],
    text_frames: &[frames::CommandRow],
) {
    assert_eq!(
        json_frames.len(),
        text_frames.len(),
        "{fixture}: the two forms run for different lengths"
    );

    let movement = drmod_tas_editor::script::keys::command_mask(&[
        "forward",
        "backward",
        "left",
        "right",
    ]);

    for (index, (json_frame, text_frame)) in json_frames.iter().zip(text_frames).enumerate() {
        assert_eq!(
            json_frame.buttons & !movement,
            text_frame.buttons & !movement,
            "{fixture}: frame {index} differs outside the movement bits"
        );

        // The stick itself is compared, and the angles compare through the axes they stand for:
        // a text stick is written as an angle or as axes, and both come back to the same numbers.
        assert_axes(
            fixture,
            index,
            "left",
            (json_frame.left_stick_angle, json_frame.left_stick_amount),
            (text_frame.left_stick_angle, text_frame.left_stick_amount),
        );
        assert_axes(
            fixture,
            index,
            "right",
            (json_frame.right_stick_angle, json_frame.right_stick_amount),
            (text_frame.right_stick_angle, text_frame.right_stick_amount),
        );
    }
}

/// A stick compared by the axes it stands for, within the thousandth the text keeps.
fn assert_axes(
    fixture: &str,
    frame: usize,
    side: &str,
    expected: (f64, f64),
    actual: (f64, f64),
) {
    let vector = |(angle, amount): (f64, f64)| {
        let radians = angle * std::f64::consts::PI / 180.0;
        [
            1000.0 * amount * radians.cos(),
            1000.0 * amount * radians.sin(),
        ]
    };

    let (left, right) = (vector(expected), vector(actual));
    for axis in 0..2 {
        assert!(
            (left[axis] - right[axis]).abs() < 1.0,
            "{fixture}: frame {frame} {side} axis {axis} differs: {} vs {}",
            left[axis],
            right[axis]
        );
    }
}
