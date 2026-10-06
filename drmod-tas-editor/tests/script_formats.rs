//! The script formats, as the editor's own tests.
//!
//! Three things are pinned here, and they are the three the editor's correctness rests on:
//!
//! * **the round trip** — `write(parse(text))` is the same text again, and `read(write(doc))` is
//!   the same document, which is what makes the three representations one script rather than three
//!   opinions about one;
//! * **the refusals** — an unknown token, an unknown rules attribute and a stick said twice are
//!   errors naming their line, because a typo that parses silently is a script the game runs wrong;
//! * **the key tables** — the two bit orders agree with the DSL's tokens, so a column, a JSON key
//!   and a token cannot drift apart unnoticed.

use drmod_tas_editor::script::dsl;
use drmod_tas_editor::script::frames;
use drmod_tas_editor::script::json;
use drmod_tas_editor::script::keys::{CommandKeys, FlagKeys};
use drmod_tas_editor::script::model::{RestartSpec, ScriptCommand, ScriptDocument, ScriptInput};
use drmod_tas_editor::script::projection;

/// The fixture the mod's own tests use, and the one the editor has to agree with byte for byte.
const ALL_INPUTS: &str = include_str!("fixtures/all_inputs.tas");

#[test]
fn text_round_trips_through_the_document() {
    let document = dsl::parse(ALL_INPUTS).expect("the fixture parses");
    let text = dsl::write(&document).expect("the fixture writes");
    let again = dsl::parse(&text).expect("the written text parses");

    assert_eq!(document, again, "write(parse(text)) is the same document");
    assert_eq!(
        text,
        dsl::write(&again).expect("the re-parse writes"),
        "writing is canonical: the second pass is the same text"
    );
}

#[test]
fn document_round_trips_through_the_api_json() {
    let document = dsl::parse(ALL_INPUTS).expect("the fixture parses");
    let body = json::write(&document).expect("the fixture writes as JSON");
    let again = json::read(&body).expect("the written JSON parses");

    assert_eq!(document, again, "read(write(doc)) is the same document");
}

#[test]
fn the_fixture_is_a_script_the_mod_would_take() {
    // The shared DTO is the authority on this, and the fixture is meant to be a script with every
    // input the format carries — so a failure here means the editor's own limits drifted from the
    // mod's.
    let document = dsl::parse(ALL_INPUTS).expect("the fixture parses");
    json::validate(&document).expect("the mod's own cross-field limits accept it");
}

#[test]
fn an_unknown_token_is_refused_with_its_line() {
    let refused = dsl::parse("! name=x\n0 a\n1 nosuchtoken\n").expect_err("the token is not one");
    let message = refused.message();

    assert!(message.contains("line 3"), "the refusal names the line: {message}");
    assert!(
        message.contains("nosuchtoken"),
        "the refusal names the token: {message}"
    );
}

#[test]
fn an_unknown_rule_is_refused_with_its_line() {
    let refused = dsl::parse("! name=x nosuchrule=y\n").expect_err("the attribute is not one");
    assert!(
        refused.message().contains("line 1"),
        "the rules line is line 1: {}",
        refused.message()
    );
}

#[test]
fn a_stick_said_twice_on_one_line_is_refused() {
    let refused = dsl::parse("! name=x\n0 ls:0 ls:90\n").expect_err("a line says one stick");
    let message = refused.message();

    assert!(message.contains("twice"), "the refusal says so: {message}");
    assert_eq!(refused.frame(), Some(0), "and carries the frame");
}

#[test]
fn an_angle_and_an_axis_on_one_line_are_refused() {
    let refused =
        dsl::parse("! name=x\n0 ls:0 lsx:500\n").expect_err("a line says one stick");
    assert!(
        refused.message().contains("two different sticks"),
        "the refusal explains: {}",
        refused.message()
    );
}

#[test]
fn a_stick_written_as_an_angle_reads_back_as_axes() {
    let document = dsl::parse("! name=x\n0 ls:90\n").expect("the angle parses");
    let input = document.commands[0].input;
    let axes = input.left_stick.expect("the angle becomes an explicit stick");

    // 90 is a full press to the right: +X, no Y.
    assert!((axes[0] - 1000.0).abs() < 0.01, "x is +1000, got {}", axes[0]);
    assert!(axes[1].abs() < 0.01, "y is 0, got {}", axes[1]);
}

#[test]
fn movement_flags_are_written_as_the_stick_they_stand_for() {
    // The DSL moves with the stick, so a document whose movement is the flags writes them out as
    // the stick those flags mean — the round trip is by value, not by spelling. Forward is a full
    // press at compass 0, which is the `ls:0` angle: the writer prefers the angle form for a full
    // press, and the parser reads it back as the axes `(0, −1000)`.
    let document = ScriptDocument {
        name: "x".to_owned(),
        trigger: None,
        restart: None,
        commands: vec![ScriptCommand {
            t: 0,
            duration: 5,
            input: ScriptInput {
                forward: true,
                ..ScriptInput::default()
            },
            when_enemy: None,
        }],
    };

    let text = dsl::write(&document).expect("the document writes");
    assert!(
        text.contains("ls:0:5"),
        "forward is a full press on the compass, 0 being forward: {text}"
    );

    let again = dsl::parse(&text).expect("the text parses");
    let axes = again.commands[0]
        .input
        .left_stick
        .expect("and reads back as the stick");
    assert!(axes[0].abs() < 0.01, "no X, got {}", axes[0]);
    assert!((axes[1] + 1000.0).abs() < 0.01, "−1000 Y, got {}", axes[1]);
}

#[test]
fn a_full_press_is_written_as_an_angle() {
    let document = ScriptDocument {
        name: "x".to_owned(),
        trigger: None,
        restart: None,
        commands: vec![ScriptCommand {
            t: 0,
            duration: 1,
            input: ScriptInput {
                left_stick: Some([1000.0, 0.0]),
                ..ScriptInput::default()
            },
            when_enemy: None,
        }],
    };

    let text = dsl::write(&document).expect("the document writes");
    assert!(text.contains("ls:90"), "a full press is an angle: {text}");
}

#[test]
fn when_enemy_has_no_text_spelling_and_is_refused_rather_than_lost() {
    let document = ScriptDocument {
        name: "x".to_owned(),
        trigger: None,
        restart: None,
        commands: vec![ScriptCommand {
            t: 0,
            duration: 1,
            input: ScriptInput {
                jump: true,
                ..ScriptInput::default()
            },
            when_enemy: Some(drmod_tas_editor::script::model::EnemyCondition {
                anim: vec![19],
                ..Default::default()
            }),
        }],
    };

    let refused = dsl::write(&document).expect_err("the DSL cannot say it");
    assert!(
        refused.message().contains("when_enemy"),
        "the refusal names the field: {}",
        refused.message()
    );

    // The JSON carries it, which is why the JSON is the source of truth.
    let body = json::write(&document).expect("the JSON carries it");
    assert!(body.contains("when_enemy"), "and writes it: {body}");
}

#[test]
fn restart_writes_only_the_parameters_that_differ_from_the_defaults() {
    let document = ScriptDocument {
        name: "x".to_owned(),
        trigger: None,
        restart: Some(RestartSpec::default()),
        commands: vec![ScriptCommand {
            t: 0,
            duration: 1,
            input: ScriptInput {
                jump: true,
                ..ScriptInput::default()
            },
            when_enemy: None,
        }],
    };

    let text = dsl::write(&document).expect("the document writes");
    assert!(
        text.contains("! name=x restart\n"),
        "a default restart is the bare word: {text}"
    );

    let changed = ScriptDocument {
        restart: Some(RestartSpec {
            ups: Some(2),
            ..RestartSpec::default()
        }),
        ..document
    };
    let text = dsl::write(&changed).expect("the document writes");
    assert!(
        text.contains("restart:ups=2"),
        "a changed parameter is spelled out: {text}"
    );
}

#[test]
fn the_frame_projection_shows_the_tokens_the_text_wrote() {
    let frames = projection::project("! name=x\n4 ls:0:3 lt:2\n");

    assert_eq!(frames.len(), 7, "four frames to the end of the three-frame run");

    // The angle fills the `ls` column for the whole run of its own duration, and the axes stay
    // blank — the table shows what the line says, not what the angle resolves to.
    for frame in &frames[4..7] {
        assert_eq!(frame.left.angle, Some(0.0), "frame {} carries the angle", frame.frame);
        assert_eq!(frame.left.x, None, "and no axis the line did not write");
    }

    // The flag has its own, shorter run: it lights frames 4 and 5 and stops — the table keeps the
    // duration the text gave each token, not a single run for the whole line.
    let lt = FlagKeys::index_of("lt").expect("lt is a column");
    assert!(frames[4].holds(lt) && frames[5].holds(lt), "lt holds for its two frames");
    assert!(!frames[6].holds(lt), "and no longer than that");

    // Frames 0..3 are the blank rows the text left: they exist, and hold nothing.
    for frame in &frames[..4] {
        assert!(!frame.held, "frame {} is a blank row", frame.frame);
    }
}

#[test]
fn a_duration_lights_every_frame_of_its_run() {
    let frames = projection::project("! name=x\n0 lt:3\n");
    let lt = FlagKeys::index_of("lt").expect("lt is a column");

    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|frame| frame.holds(lt)), "all three are lit");
}

#[test]
fn the_two_bit_orders_agree_with_their_own_tables() {
    // The converter's order: every boolean of `input`, each lighting its own bit.
    for (bit, key) in CommandKeys::ALL.iter().enumerate() {
        let input = (key.set)(ScriptInput::default());
        let re_read = CommandKeys::ALL
            .iter()
            .enumerate()
            .filter(|(_, other)| (other.get)(&input))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();

        assert_eq!(re_read, vec![bit], "{} lights only its own bit", key.key);
    }

    // The table's order: only the flags a line can spell, each its own column.
    for flag in &FlagKeys::ALL {
        assert!(
            dsl::vocabulary().iter().any(|(token, _)| *token == flag.token),
            "the column {} is a token the parser accepts",
            flag.token
        );

        assert!(
            CommandKeys::index_of(flag.key).is_some(),
            "the column {} names a real input",
            flag.token
        );
    }
}

#[test]
fn the_frame_converter_collapses_a_run_into_one_command() {
    let document = dsl::parse("! name=x\n2 lt:4\n").expect("the fixture parses");
    let rows = frames::expand(&document);

    assert_eq!(rows.len(), 6, "the run ends at frame 6");
    assert!(rows[0].buttons == 0, "frame 0 is empty");
    assert!(rows[2].holds(CommandKeys::index_of("blade").expect("blade is an input")));

    let collapsed = frames::collapse(&rows);
    let blade = collapsed
        .iter()
        .find(|command| command.input.blade)
        .expect("the run becomes a command");

    assert_eq!(blade.t, 2);
    assert_eq!(blade.duration, 4);
}

#[test]
fn a_comment_and_a_blank_line_are_not_frames() {
    let document = dsl::parse("! name=x\n# a comment\n\n0 a\n").expect("the fixture parses");
    assert_eq!(document.commands.len(), 1);
    assert_eq!(document.commands[0].t, 0);
}

#[test]
fn a_line_separator_never_changes_what_a_text_says() {
    let unix = "! name=x\n0 a\n1 lt\n";
    let windows = "! name=x\r\n0 a\r\n1 lt\r\n";

    // `\r\n` halves trim to `\n`, so a file edited elsewhere reads as the script it is — which is
    // exactly what the workspace's listing relies on.
    assert_eq!(dsl::lines(windows), unix);
    assert_eq!(
        dsl::parse(windows).expect("windows lines parse"),
        dsl::parse(unix).expect("unix lines parse")
    );
}

#[test]
fn a_script_the_mod_would_refuse_is_refused_here_too() {
    // Empty input is the mod's own `input is empty` refusal (`parse_script`).
    let document = ScriptDocument {
        name: "x".to_owned(),
        trigger: None,
        restart: None,
        commands: vec![ScriptCommand {
            t: 0,
            duration: 1,
            input: ScriptInput::default(),
            when_enemy: None,
        }],
    };

    let refused = json::validate(&document).expect_err("the mod refuses it");
    assert!(
        refused.message().contains("input is empty"),
        "the refusal is the mod's own: {}",
        refused.message()
    );
}

#[test]
fn a_relative_frame_counts_from_the_previous_line() {
    // The first line is absolute and every `+N` is a delta from the line before it, so the two
    // forms mix: `100`, then `110`, then an absolute `200` that the next delta counts from.
    let document = dsl::parse("! name=x\n100 a\n+10 x\n+5 y\n200 lt\n+2 b\n")
        .expect("the relative text parses");

    let frames: Vec<u32> = document.commands.iter().map(|command| command.t).collect();
    assert_eq!(frames, vec![100, 110, 115, 200, 202]);
}

#[test]
fn the_first_relative_line_counts_from_zero() {
    let document = dsl::parse("! name=x\n+7 a\n").expect("the relative text parses");
    assert_eq!(document.commands[0].t, 7);
}

#[test]
fn a_relative_text_round_trips_through_the_relative_writer() {
    let text = "! name=x\n100 a\n+10 x\n+5 y\n";
    let document = dsl::parse(text).expect("the relative text parses");

    // The writer keeps the shape the author tuned: the first frame absolute, the rest deltas.
    assert_eq!(dsl::write_relative(&document).expect("it writes"), text);
    assert_eq!(
        dsl::parse(&dsl::write_relative(&document).expect("it writes")).expect("it parses"),
        document
    );

    // The absolute spelling of the same document is the canonical one.
    assert_eq!(dsl::write(&document).expect("it writes"), "! name=x\n100 a\n110 x\n115 y\n");
}

#[test]
fn a_bare_plus_is_refused_with_its_line() {
    let refused = dsl::parse("! name=x\n+ a\n").expect_err("`+` has no number");
    assert!(
        refused.message().contains("line 2"),
        "the refusal names the line: {}",
        refused.message()
    );
}

#[test]
fn the_table_resolves_relative_frames() {
    let frames = projection::project("! name=x\n100 a\n+10 x\n");

    assert_eq!(frames.len(), 111, "the last frame is 110");
    assert!(frames[100].held, "the absolute line lights frame 100");
    assert!(!frames[105].held, "the frames between the lines are blank");
    assert!(frames[110].held, "the delta line lights frame 110");
}
