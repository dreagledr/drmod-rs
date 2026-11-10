//! The command table: one virtualised row per frame of the script text.
//!
//! Rows come from [`drmod_script::projection`], read token by token from the text rather than from
//! the parsed document — the table is meant to show what the script *says*, including whether a
//! stick was written as an angle or as axes. Only the rows on screen are built, so a 20 000-frame
//! script costs what the visible thirty do.

use std::borrow::Cow;

use drmod_script::keys::FlagKeys;
use drmod_script::projection::{ScriptFrame, StickValue};
use rune::{El, col, row, text};
use rune_kit::{Cell, Table, TableColumn};

use crate::app::App;

pub fn panel(app: &App, height: f32) -> El {
    let k = &app.kit;
    let frames = app.frames.len();

    let header = row()
        .w_full()
        .h(28.0)
        .px(10.0)
        .center_y()
        .gap(8.0)
        .child(text("КАДРЫ").font(k.ty.small_medium).color(k.c.text_faint))
        .child(
            text(format!("{frames}"))
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
        .child(rune::spacer())
        .child(
            text("прокрутка колесом · колонки тянутся за заголовок")
                .font(k.ty.small)
                .color(k.c.text_faint),
        );

    let table = Table::new(
        |s: &App| &s.table,
        |s: &mut App| &mut s.table,
        |s: &App| s.frames.len(),
        |s: &App, i| Cow::Owned(s.frames[i].frame.to_string()),
    )
    .name("Кадры скрипта")
    .column(TableColumn::new("frame", "#").width(74.0).resizable(false))
    .column(TableColumn::new("left", "Левый стик").flex(1.0).min(120.0))
    .column(
        TableColumn::new("right", "Правый стик")
            .flex(1.0)
            .min(120.0),
    )
    .column(TableColumn::new("held", "Кнопки").flex(2.0).min(180.0))
    .cell(|s: &App, i, column| match column {
        "frame" => Cell::text(s.frames[i].frame.to_string()).mono(),
        "left" => Cell::text(stick(s.frames[i].left)).mono().muted(),
        "right" => Cell::text(stick(s.frames[i].right)).mono().muted(),
        "held" => Cell::text(held(&s.frames[i])).mono(),
        _ => Cell::text(""),
    })
    .build(app, k)
    .key("frames-table");

    let body = if frames == 0 {
        col()
            .key("frames-empty")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .center()
            .child(
                text(match &app.status.error {
                    Some(_) => "Таблица молчит, пока текст не разбирается.",
                    None => "В скрипте нет кадров.",
                })
                .font(k.ty.small)
                .color(k.c.text_faint),
            )
    } else {
        col()
            .key("frames-body")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .child(table.h_full())
    };

    col()
        .key("frames-panel")
        .w_full()
        .h(height)
        .min_h(120.0)
        .fill(k.c.layer_01)
        .child(header)
        .child(body)
}

/// A stick the way the text wrote it: an angle, or the axes it named. Blank when the line said
/// nothing about it.
fn stick(stick: StickValue) -> String {
    if let Some(angle) = stick.angle {
        return format!("{angle}");
    }

    match (stick.x, stick.y) {
        (Some(x), Some(y)) => format!("x {x}  y {y}"),
        (Some(x), None) => format!("x {x}"),
        (None, Some(y)) => format!("y {y}"),
        (None, None) => String::new(),
    }
}

/// The tokens a frame holds, in the DSL's own token order — the table's compact stand-in for one
/// lit column per button.
fn held(frame: &ScriptFrame) -> String {
    FlagKeys::ALL
        .iter()
        .enumerate()
        .filter(|(bit, _)| frame.holds(*bit))
        .map(|(_, key)| key.token)
        .collect::<Vec<_>>()
        .join(" ")
}
