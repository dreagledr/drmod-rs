//! Colouring a `.tas` text: a small lexer of the DSL's own tokens, turned into Rune [`Span`]s.
//!
//! `.tas` is not one of the languages Rune's built-in highlighter knows, and it does not need a
//! general one: its vocabulary is a first token, whitespace-separated pad tokens, and a `#`
//! comment. The spans are rebuilt only when the text changes (the app keeps the cache).

use std::collections::HashSet;
use std::sync::OnceLock;

use drmod_script::dsl;
use rune::Span;
use rune_kit::syntax::{Syntax, SyntaxTheme};

/// The spans of one text, in byte offsets. `theme` colours the kinds; the text itself is drawn by
/// the editor's mono style.
pub fn spans(text: &str, theme: &SyntaxTheme) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut line_start = 0usize;

    for line in text.split('\n') {
        colour_line(line, line_start, theme, &mut spans);
        line_start += line.len() + 1;
    }

    spans
}

fn colour_line(line: &str, offset: usize, theme: &SyntaxTheme, out: &mut Vec<Span>) {
    let (body, comment) = match line.find('#') {
        Some(at) => (&line[..at], Some(at)),
        None => (line, None),
    };

    let rules_line = body.trim_start().starts_with('!');
    let mut first = true;

    for (start, end) in words(body) {
        let word = &body[start..end];
        let kind = if rules_line {
            rule_kind(word, first)
        } else if first {
            Syntax::Number
        } else {
            token_kind(word)
        };

        // The `head:value` form: the head is the word's own kind, the argument reads as a number.
        if let Some(colon) = word.find(':') {
            let head_end = start + colon;
            out.push(paint(offset + start, offset + head_end, theme, kind));
            let value_start = head_end + 1;
            if value_start < end {
                out.push(paint(
                    offset + value_start,
                    offset + end,
                    theme,
                    Syntax::Number,
                ));
            }
        } else if rules_line && let Some(eq) = word.find('=') {
            let head_end = start + eq;
            out.push(paint(offset + start, offset + head_end, theme, kind));
            let value_start = head_end + 1;
            if value_start < end {
                out.push(paint(
                    offset + value_start,
                    offset + end,
                    theme,
                    Syntax::String,
                ));
            }
        } else {
            out.push(paint(offset + start, offset + end, theme, kind));
        }

        first = false;
    }

    if let Some(at) = comment {
        out.push(paint(
            offset + at,
            offset + line.len(),
            theme,
            Syntax::Comment,
        ));
    }
}

fn paint(start: usize, end: usize, theme: &SyntaxTheme, kind: Syntax) -> Span {
    let mut span = Span::new(start, end).color(theme.color(kind));
    if let Some(weight) = theme.weight(kind) {
        span = span.weight(weight);
    }
    span
}

fn rule_kind(word: &str, first: bool) -> Syntax {
    if first {
        return Syntax::Keyword;
    }

    let head = word.split(['=', ':']).next().unwrap_or(word);
    match head {
        "name" | "trig" | "restart" => Syntax::Property,
        _ => Syntax::Variable,
    }
}

fn token_kind(word: &str) -> Syntax {
    let head = word.split(':').next().unwrap_or(word);
    match head.to_ascii_lowercase().as_str() {
        "ls" | "lsx" | "lsy" | "rs" | "rsx" | "rsy" => Syntax::Type,
        "by" | "dr" => Syntax::Function,
        "wk" => Syntax::Attribute,
        other => {
            if vocabulary().contains(other) {
                Syntax::Function
            } else {
                Syntax::Variable
            }
        }
    }
}

/// The DSL's own button tokens. Built once; the text region colours a token per frame and the set
/// is small.
fn vocabulary() -> &'static HashSet<&'static str> {
    static VOCABULARY: OnceLock<HashSet<&'static str>> = OnceLock::new();
    VOCABULARY.get_or_init(|| {
        dsl::vocabulary()
            .into_iter()
            .map(|(token, _)| token)
            .collect()
    })
}

/// The spans of the whitespace-separated words of `line`, as byte ranges inside it.
fn words(line: &str) -> Vec<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut words = Vec::new();
    let mut at = 0usize;

    while at < bytes.len() {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= bytes.len() {
            break;
        }

        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        words.push((start, at));
    }

    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(usize, usize, Syntax)> {
        let theme = SyntaxTheme::neutral(true);
        spans(text, &theme)
            .into_iter()
            .map(|span| {
                let kind = [
                    Syntax::Keyword,
                    Syntax::String,
                    Syntax::Number,
                    Syntax::Comment,
                    Syntax::Type,
                    Syntax::Function,
                    Syntax::Property,
                    Syntax::Variable,
                ]
                .into_iter()
                .find(|kind| theme.color(*kind) == span.color.unwrap_or(rune::Color::TRANSPARENT))
                .unwrap_or(Syntax::Variable);
                (span.start as usize, span.end as usize, kind)
            })
            .collect()
    }

    #[test]
    fn a_comment_runs_to_the_end_of_its_line() {
        let spans = kinds("0 a  # why\n1 b");
        assert!(
            spans
                .iter()
                .any(|(start, end, kind)| *kind == Syntax::Comment
                    && &"0 a  # why\n1 b"[*start..*end] == "# why")
        );
    }

    #[test]
    fn the_rules_line_and_both_stick_forms_are_distinct() {
        let text = "! name=x trig=ticks:0\n0 lsx:500 lsy:-1000 a:2";
        let spans = kinds(text);

        let find = |needle: &str| {
            spans
                .iter()
                .find(|(start, end, _)| &text[*start..*end] == needle)
                .map(|(_, _, kind)| *kind)
                .unwrap()
        };

        assert_eq!(find("!"), Syntax::Keyword);
        assert_eq!(find("name"), Syntax::Property);
        assert_eq!(find("lsx"), Syntax::Type);
        assert_eq!(find("a"), Syntax::Function);
        assert_eq!(find("500"), Syntax::Number);
        assert_eq!(find("x"), Syntax::String);
    }

    #[test]
    fn a_byte_offset_is_the_authored_one() {
        // Cyrillic is two bytes: the lexer must not report character offsets.
        let text = "# привет\n0 a";
        let spans = kinds(text);
        let comment = spans
            .iter()
            .find(|(_, _, kind)| *kind == Syntax::Comment)
            .unwrap();
        assert_eq!(comment.0, 0);
        assert_eq!(comment.1, "# привет".len());
        assert_eq!(&text[comment.0..comment.1], "# привет");
    }
}
