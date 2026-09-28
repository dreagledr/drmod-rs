//! The command table's view of a script text: one [`ScriptFrame`] per frame, showing the tokens
//! exactly as the lines write them.
//!
//! The table reads the text, not the parsed document. A document has already resolved
//! `ls:<angle>` into axis numbers and the movement flags into a stick, so the two spellings are
//! indistinguishable by the time it exists — but the table is meant to show what the script
//! *says*, and the line says `ls:0` or `lsx:500`, not a pair of numbers that happens to mean the
//! same thing. Reading the tokens keeps the table a picture of the text rather than a second
//! opinion about it.
//!
//! Frames run from 0 to the end of the last token held, so a row exists for every frame a `.tas`
//! could name; the frames no token touches are the table's blank rows, which is what the format
//! itself says by writing nothing.

use std::collections::BTreeMap;

use super::dsl;
use super::keys::{FlagKeys, flag_mask};

/// One frame of the command table, holding each stick exactly as the `.tas` text spells it.
///
/// A stick is written in one of two forms, and the frame keeps whichever the text used rather
/// than translating between them: `ls:<angle>` is a direction, `lsx`/`lsy` (and `rsx`/`rsy`) are
/// exact axis values. The table paints each into its own column, so a row reads as the line the
/// format would write for that frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScriptFrame {
    pub frame: u32,
    pub left: StickValue,
    pub right: StickValue,
    pub buttons: u32,
    pub held: bool,
}

impl ScriptFrame {
    /// Whether the input at `bit` — an index into [`FlagKeys::ALL`] — is held on this frame.
    pub fn holds(&self, bit: usize) -> bool {
        self.buttons & (1u32 << bit) != 0
    }
}

/// One stick of one frame, in the form the text wrote it.
///
/// The two forms are exclusive, because a line cannot say both (the parser refuses it) — so at
/// most one of the angle and the axes is present, and the other is not.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct StickValue {
    pub angle: Option<f64>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

impl StickValue {
    /// A stick the frame does not mention: both forms absent, which is the blank columns.
    pub const NONE: Self = Self {
        angle: None,
        x: None,
        y: None,
    };

    /// The `ls:<angle>` form.
    pub fn direction(angle: f64) -> Self {
        Self {
            angle: Some(angle),
            ..Self::NONE
        }
    }

    /// The `lsx` axis of the `lsx`/`lsy` form, added to whatever the line has said so far.
    pub fn with_x(self, x: f64) -> Self {
        Self { x: Some(x), ..self }
    }

    /// The `lsy` axis of the `lsx`/`lsy` form.
    pub fn with_y(self, y: f64) -> Self {
        Self { y: Some(y), ..self }
    }
}

/// The frames of a script text, or nothing when it does not parse — the text region carries the
/// parser's message, so the table stays silent rather than repeating it.
pub fn project(text: &str) -> Vec<ScriptFrame> {
    let mut rows: BTreeMap<u32, ScriptFrame> = BTreeMap::new();

    for line in dsl::lines(text).split('\n') {
        let Some((number, tokens)) = read_line(line) else {
            continue;
        };

        for token in tokens {
            hold(&mut rows, number, token);
        }
    }

    fill(rows)
}

/// One token of a frame line: what it does, and how many frames it holds for.
struct Token {
    duration: u32,
    apply: Box<dyn Fn(ScriptFrame) -> ScriptFrame>,
}

/// Fills frames `number` .. `number + token.duration - 1` with the column that token carries.
///
/// A token says one thing — a flag, or one stick — so each write fills one column for the whole
/// run. On a frame two tokens both touch, the longer run wins, which is the format's own "OR the
/// bits, keep the longest" rule; for a stick it means the later token's value stands, matching
/// the mod's last-wins assignment.
fn hold(rows: &mut BTreeMap<u32, ScriptFrame>, number: u32, token: Token) {
    for offset in 0..token.duration {
        let frame = number + offset;
        let row = rows.get(&frame).copied().unwrap_or(ScriptFrame {
            frame,
            left: StickValue::NONE,
            right: StickValue::NONE,
            buttons: 0,
            held: true,
        });
        rows.insert(frame, (token.apply)(row));
    }
}

/// One row per frame from 0 to the last one a token touched, so a gap the text leaves is a blank
/// row rather than a missing one.
fn fill(rows: BTreeMap<u32, ScriptFrame>) -> Vec<ScriptFrame> {
    let total = rows.keys().map(|frame| frame + 1).max().unwrap_or(0);

    (0..total)
        .map(|frame| {
            rows.get(&frame).copied().unwrap_or(ScriptFrame {
                frame,
                left: StickValue::NONE,
                right: StickValue::NONE,
                buttons: 0,
                held: false,
            })
        })
        .collect()
}

/// One frame line: its number, and the tokens that follow it. A blank line, a comment and the
/// rules line are not frames.
fn read_line(line: &str) -> Option<(u32, Vec<Token>)> {
    let text = strip_comment(line).trim();
    if text.is_empty() || text.starts_with('!') {
        return None;
    }

    let parts: Vec<&str> = text.split(' ').filter(|part| !part.is_empty()).collect();
    let number = parts.first()?.parse::<u32>().ok()?;

    let mut tokens = Vec::new();
    for part in &parts[1..] {
        if let Some(token) = read(part) {
            tokens.push(token);
        }
    }

    Some((number, tokens))
}

/// One token as the write it performs. An unknown token, a stick without a value and a
/// malformed one are skipped — the table shows what it can read, and the parse refuses the line
/// separately (which is what empties the table anyway).
fn read(token: &str) -> Option<Token> {
    let separator = token.find(':');
    let key = match separator {
        Some(at) => token[..at].to_ascii_lowercase(),
        None => token.to_ascii_lowercase(),
    };
    let arguments: Vec<&str> = match separator {
        Some(at) => token[at + 1..].split(':').collect(),
        None => Vec::new(),
    };

    // A stick carries its value first, so its duration is the second argument.
    match key.as_str() {
        "ls" => return read_stick(&arguments, read_duration(&arguments, 1), true, true, -1),
        "rs" => return read_stick(&arguments, read_duration(&arguments, 1), false, true, -1),
        "lsx" => return read_stick(&arguments, read_duration(&arguments, 1), true, false, 0),
        "lsy" => return read_stick(&arguments, read_duration(&arguments, 1), true, false, 1),
        "rsx" => return read_stick(&arguments, read_duration(&arguments, 1), false, false, 0),
        "rsy" => return read_stick(&arguments, read_duration(&arguments, 1), false, false, 1),
        _ => {}
    }

    // A flag has no value of its own, so its single argument is the duration when there is one.
    let bit = FlagKeys::index_of(&key)?;
    Some(Token {
        duration: read_duration(&arguments, 0),
        apply: Box::new(move |row| ScriptFrame {
            buttons: row.buttons | flag_mask(&[FlagKeys::ALL[bit].token]),
            ..row
        }),
    })
}

/// A stick token as its write. The angle form is the whole value; an axis form sets one axis and
/// leaves the other as the line left it, so a `lsx` with no `lsy` shows a blank Y.
fn read_stick(
    arguments: &[&str],
    duration: u32,
    left: bool,
    angle: bool,
    axis: i32,
) -> Option<Token> {
    let value = arguments.first()?.parse::<f64>().ok()?;

    if angle {
        let direction = StickValue::direction(value);
        return Some(Token {
            duration,
            apply: Box::new(move |row| {
                if left {
                    ScriptFrame {
                        left: direction,
                        ..row
                    }
                } else {
                    ScriptFrame {
                        right: direction,
                        ..row
                    }
                }
            }),
        });
    }

    Some(Token {
        duration,
        apply: Box::new(move |row| {
            let stick = if left { row.left } else { row.right };
            let stick = if axis == 0 {
                stick.with_x(value)
            } else {
                stick.with_y(value)
            };
            if left {
                ScriptFrame { left: stick, ..row }
            } else {
                ScriptFrame { right: stick, ..row }
            }
        }),
    })
}

/// The duration a token holds for, taken from the argument at `at`: `1` when it is absent or
/// unreadable — the DSL's own default for a token that names none.
fn read_duration(arguments: &[&str], at: usize) -> u32 {
    arguments
        .get(at)
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|frames| *frames >= 1)
        .unwrap_or(1)
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(at) => &line[..at],
        None => line,
    }
}
