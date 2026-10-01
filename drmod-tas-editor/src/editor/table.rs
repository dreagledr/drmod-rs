//! The command table: one row per frame, one column per DSL token — the whole script as a matrix,
//! so a timing reads off the shape of the lit cells instead of a JSON command list.
//!
//! Read-only. The table visualizes the `.tas` text above it: a row reads as the line the format
//! would write for that frame, and editing is done in the text, in one place, rather than in two
//! representations that would have to be kept in step.
//!
//! The frames come from the text on screen, read token by token rather than from the parsed document
//! ([`crate::script::projection`]): a document has already resolved `ls:<angle>` into axis numbers,
//! so the two spellings are indistinguishable by then, and the table is meant to show what the
//! script says.

use dear_app::imgui::{ListClipper, TableColumnRef, TableFlags, TableSizingPolicy, Ui};

use crate::script::keys::FlagKeys;
use crate::script::projection::ScriptFrame;

/// The table's own row pitch, reported to the clipper so it can work out the visible range.
const ROW_HEIGHT: f32 = 20.0;

/// The columns, in the order the table draws them: the frame number, then the six stick tokens,
/// then the flags in the DSL's own token order.
///
/// The header of every column but `#` is a token of the format, so the table doubles as its legend.
/// `#` is not a token — the format spells a frame as the bare number at the start of the line, which
/// is what `#` says.
pub fn columns() -> Vec<&'static str> {
    let mut columns = vec!["#", "ls", "lsx", "lsy", "rs", "rsx", "rsy"];
    for flag in &FlagKeys::ALL {
        columns.push(flag.token);
    }

    columns
}

/// Whether a column is a flag — a lit cell rather than a value.
fn flag_column(column: &str) -> Option<usize> {
    FlagKeys::index_of(column)
}

/// One frame's cell in a given column, as the text wrote it. An empty answer is a blank cell — a
/// value the line did not write is not a zero.
fn cell_text(frame: &ScriptFrame, column: &str) -> String {
    match column {
        "#" => frame.frame.to_string(),
        "ls" => value(frame.left.angle),
        "lsx" => value(frame.left.x),
        "lsy" => value(frame.left.y),
        "rs" => value(frame.right.angle),
        "rsx" => value(frame.right.x),
        "rsy" => value(frame.right.y),
        _ => String::new(),
    }
}

/// A stick value the way the text wrote it, or blank when the line left it out.
fn value(number: Option<f64>) -> String {
    match number {
        Some(number) => format!("{number}"),
        None => String::new(),
    }
}

/// The whole table, from the frames [`super::text_cache`] already projected.
///
/// ⚠️ The frames are handed in rather than projected here: the projection is O(text) and allocates,
/// and the table is not the only panel that reads the script — the run controls' status line does
/// too. `table::draw(ui, &text)` used to parse and project the whole script once per frame, which is
/// what the cache exists to stop.
///
/// ⚠️ The table is drawn **whether or not there is anything to put in it**, and `empty_note` is what
/// its single body row says when there is not. A table with no selection used to skip the table
/// entirely, which took the header — the legend of all 29 columns — with it; the header is worth
/// reading precisely when the author is working out which column a token belongs to.
pub fn draw(ui: &Ui, frames: &[ScriptFrame], empty_note: &str) {
    let columns = columns();
    let mut builder = ui
        .table("command-table")
        .flags(
            TableFlags::BORDERS
                | TableFlags::ROW_BG
                | TableFlags::SCROLL_Y
                | TableFlags::SCROLL_X
                | TableFlags::RESIZABLE,
        )
        .sizing_policy(TableSizingPolicy::FixedFit)
        .outer_size([0.0, -1.0])
        // ⚠️ `headers(true)` only *submits* the header row — `freeze` is what keeps it pinned, and
        // without it the header scrolls away with the first screenful. The frame-number column is
        // pinned for the same reason horizontally: 29 columns do not fit one panel, and a row of lit
        // cells is meaningless without the frame it belongs to.
        .freeze(1, 1);

    for column in &columns {
        // Flag cells are square: the row height doubles as the width, so one of the two cannot
        // drift away from a matrix that reads as a matrix. The angle and axis columns are wider,
        // to hold "359.999" and "-1000".
        let width = match *column {
            "#" => 44.0,
            "ls" | "rs" => 48.0,
            "lsx" | "lsy" | "rsx" | "rsy" => 44.0,
            _ => ROW_HEIGHT,
        };
        builder = builder.column(*column).width(width).done();
    }

    builder.headers(true).build(|ui| {
        if frames.is_empty() {
            // One body row for the note: without it the header would sit over a table that ImGui
            // sizes to nothing, and a header alone is a strip that reads as a glitch. The note says
            // why there are no rows rather than the panel going blank.
            ui.table_next_row();
            ui.table_next_column();
            ui.text(empty_note);
            return;
        }

        // Only the visible rows are submitted — a 20 000-frame script is the case the clipper
        // exists for.
        for index in ListClipper::new(frames.len())
            .items_height(ROW_HEIGHT)
            .begin(ui)
            .iter()
        {
            draw_row(ui, &frames[index], &columns);
        }
    });
}

fn draw_row(ui: &Ui, frame: &ScriptFrame, columns: &[&str]) {
    ui.table_next_row();

    for column in columns {
        ui.table_next_column();

        if let Some(bit) = flag_column(column) {
            // A held flag is the accent fill; the surface is painted on every cell so the
            // checkerboard reads as a matrix rather than as gaps.
            if frame.holds(bit) {
                ui.table_set_cell_bg_color(
                    [0.20, 0.45, 0.72, 1.0],
                    TableColumnRef::Current,
                );
            }

            continue;
        }

        let text = cell_text(frame, column);
        if !text.is_empty() {
            ui.text(&text);
        }
    }
}
