//! Сворачиваемая полоса свойств: `name`, `trigger`, `restart`, кадр, условные команды.

use crate::editor::{Editor, Msg, WhenField};
use crate::matrix::NumberColumn;
use crate::model::WhenEnemy;
use windows_reactor::*;

/// Высота полосы свойств: выше — она распирает окно и выдавливает таймлайн и JSON.
const MAX_HEIGHT: f64 = 240.0;

pub(crate) fn render(editor: &Editor, context: &mut ViewContext<Editor>) -> View {
    let trigger_pos = editor
        .script
        .trigger
        .as_ref()
        .and_then(|trigger| trigger.pos);
    let trigger_ticks = editor
        .script
        .trigger
        .as_ref()
        .and_then(|trigger| trigger.ticks);

    let common = Grid::new()
        .row_spacing(4.0)
        .column_spacing(8.0)
        .columns([GridLength::Pixel(160.0), GridLength::Star(1.0)])
        .rows([GridLength::Auto; 4])
        .children((
            label("name").grid_row(0),
            TextBox::new()
                .text(editor.script.name.clone())
                .on_text_changed(context.callback(Msg::NameChanged))
                .grid_row(0)
                .grid_column(1),
            label("trigger: позиция").grid_row(1),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .grid_row(1)
                .grid_column(1)
                .children((
                    CheckBox::new()
                        .is_checked(trigger_pos.is_some())
                        .on_is_checked_changed(context.callback(Msg::TriggerPos)),
                    axis_box(trigger_pos.map(|pos| f64::from(pos[0])), context, 0),
                    axis_box(trigger_pos.map(|pos| f64::from(pos[1])), context, 1),
                    axis_box(trigger_pos.map(|pos| f64::from(pos[2])), context, 2),
                )),
            label("trigger: тики").grid_row(2),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .grid_row(2)
                .grid_column(1)
                .children((
                    CheckBox::new()
                        .is_checked(trigger_ticks.is_some())
                        .on_is_checked_changed(context.callback(Msg::TriggerTicks)),
                    NumberBox::new()
                        .minimum(0.0)
                        .value(trigger_ticks.map(f64::from))
                        .on_value_changed(context.callback(Msg::TriggerTicksValue)),
                )),
            label("restart миссии").grid_row(3),
            CheckBox::new()
                .is_checked(editor.script.restart.is_some())
                .on_is_checked_changed(context.callback(Msg::RestartEnabled))
                .grid_row(3)
                .grid_column(1),
        ));

    let frame = frame_grid(editor, context);
    let conditional = conditional_grid(editor, context, editor.selected_when_enemy());

    let content = StackPanel::new()
        .spacing(4.0)
        .children((common, frame, conditional));

    Expander::new()
        .is_expanded(editor.properties_open)
        .on_is_expanded_changed(context.callback(Msg::ToggleProperties))
        .slots([
            SlotView::new(
                ExpanderSlot::Header,
                TextBlock::new().text("Свойства скрипта"),
            ),
            SlotView::new(
                ExpanderSlot::Content,
                ScrollViewer::new()
                    .height(MAX_HEIGHT)
                    .vertical_scroll_bar_visibility(ScrollBarVisibility::Auto)
                    .content(Border::new().padding(2.0).content(content)),
            ),
        ])
}

fn frame_grid(editor: &Editor, context: &mut ViewContext<Editor>) -> View {
    let Some(frame) = editor.selected_frame else {
        return Border::new()
            .padding(6.0)
            .content(TextBlock::new().text(
                "кадр не выбран: кликните строку таймлайна, чтобы править камеру и стик",
            ));
    };

    Grid::new()
        .row_spacing(4.0)
        .column_spacing(8.0)
        .columns([GridLength::Pixel(160.0), GridLength::Star(1.0)])
        .rows([GridLength::Auto; 3])
        .children((
            TextBlock::new()
                .text(format!("кадр {frame}"))
                .grid_row(0)
                .grid_column_span(2),
            label("camera dx / dy").grid_row(1),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .grid_row(1)
                .grid_column(1)
                .children((
                    number_box(
                        editor
                            .matrix
                            .number(frame, NumberColumn::CameraX)
                            .map(f64::from),
                        true,
                        context,
                        move |value| Msg::SetNumber {
                            frame,
                            column: NumberColumn::CameraX,
                            value,
                        },
                    ),
                    number_box(
                        editor
                            .matrix
                            .number(frame, NumberColumn::CameraY)
                            .map(f64::from),
                        true,
                        context,
                        move |value| Msg::SetNumber {
                            frame,
                            column: NumberColumn::CameraY,
                            value,
                        },
                    ),
                )),
            label("left_stick x / y").grid_row(2),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .grid_row(2)
                .grid_column(1)
                .children((
                    number_box(
                        editor
                            .matrix
                            .number(frame, NumberColumn::StickX)
                            .map(f64::from),
                        true,
                        context,
                        move |value| Msg::SetNumber {
                            frame,
                            column: NumberColumn::StickX,
                            value,
                        },
                    ),
                    number_box(
                        editor
                            .matrix
                            .number(frame, NumberColumn::StickY)
                            .map(f64::from),
                        true,
                        context,
                        move |value| Msg::SetNumber {
                            frame,
                            column: NumberColumn::StickY,
                            value,
                        },
                    ),
                )),
        ))
}

fn conditional_grid(
    editor: &Editor,
    context: &mut ViewContext<Editor>,
    when: Option<&WhenEnemy>,
) -> View {
    let labels: Vec<String> = editor
        .matrix
        .conditional()
        .iter()
        .map(|command| {
            let anim = command
                .when_enemy
                .as_ref()
                .and_then(|when| when.anim.as_ref())
                .map(|anim| {
                    anim.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .unwrap_or_else(|| "—".to_string());
            format!("t={} dur={} anim={anim}", command.t, command.duration)
        })
        .collect();

    let anim_text = when
        .and_then(|when| when.anim.as_ref())
        .map(|anim| {
            anim.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let enabled = when.is_some();

    Grid::new()
        .row_spacing(4.0)
        .column_spacing(8.0)
        .columns([GridLength::Pixel(160.0), GridLength::Star(1.0)])
        .rows([GridLength::Auto; 7])
        .children((
            label("условная команда").grid_row(0),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .grid_row(0)
                .grid_column(1)
                .children((
                    ComboBox::new()
                        .items_source(labels)
                        .selected_index(editor.conditional_index)
                        .is_enabled(!editor.matrix.conditional().is_empty())
                        .on_selection_changed(context.callback(Msg::ConditionalSelected)),
                    Button::new()
                        .is_enabled(enabled)
                        .on_click(context.message(Msg::ConditionalDelete))
                        .content("Удалить"),
                )),
            label("anim (через запятую)").grid_row(1),
            TextBox::new()
                .text(anim_text)
                .is_enabled(enabled)
                .placeholder_text("65545, 19")
                .on_text_changed(context.callback(Msg::ConditionalAnim))
                .grid_row(1)
                .grid_column(1),
            label("frame_min / frame_max").grid_row(2),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .grid_row(2)
                .grid_column(1)
                .children((
                    number_box(
                        when.and_then(|when| when.frame_min).map(f64::from),
                        enabled,
                        context,
                        |value| Msg::ConditionalNumber {
                            field: WhenField::FrameMin,
                            value,
                        },
                    ),
                    number_box(
                        when.and_then(|when| when.frame_max).map(f64::from),
                        enabled,
                        context,
                        |value| Msg::ConditionalNumber {
                            field: WhenField::FrameMax,
                            value,
                        },
                    ),
                )),
            label("dist_max / blade_dy_min").grid_row(3),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .grid_row(3)
                .grid_column(1)
                .children((
                    number_box(
                        when.and_then(|when| when.dist_max).map(f64::from),
                        enabled,
                        context,
                        |value| Msg::ConditionalNumber {
                            field: WhenField::DistMax,
                            value,
                        },
                    ),
                    number_box(
                        when.and_then(|when| when.blade_dy_min).map(f64::from),
                        enabled,
                        context,
                        |value| Msg::ConditionalNumber {
                            field: WhenField::BladeDyMin,
                            value,
                        },
                    ),
                )),
            label("player_y_min / player_y_max").grid_row(4),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .grid_row(4)
                .grid_column(1)
                .children((
                    number_box(
                        when.and_then(|when| when.player_y_min).map(f64::from),
                        enabled,
                        context,
                        |value| Msg::ConditionalNumber {
                            field: WhenField::PlayerYMin,
                            value,
                        },
                    ),
                    number_box(
                        when.and_then(|when| when.player_y_max).map(f64::from),
                        enabled,
                        context,
                        |value| Msg::ConditionalNumber {
                            field: WhenField::PlayerYMax,
                            value,
                        },
                    ),
                )),
            label("player_vy_max").grid_row(5),
            number_box(
                when.and_then(|when| when.player_vy_max).map(f64::from),
                enabled,
                context,
                |value| Msg::ConditionalNumber {
                    field: WhenField::PlayerVyMax,
                    value,
                },
            )
            .grid_row(5)
            .grid_column(1),
            label("repeat").grid_row(6),
            CheckBox::new()
                .is_checked(when.is_some_and(|when| when.repeat))
                .is_enabled(enabled)
                .on_is_checked_changed(context.callback(Msg::ConditionalRepeat))
                .grid_row(6)
                .grid_column(1),
        ))
}

fn label(text: &str) -> TextBlock {
    TextBlock::new().text(text)
}

fn axis_box(value: Option<f64>, context: &mut ViewContext<Editor>, axis: usize) -> NumberBox {
    NumberBox::new()
        .value(value)
        .on_value_changed(
            context.callback(move |value: Option<f64>| Msg::TriggerPosValue { axis, value }),
        )
}

fn number_box(
    value: Option<f64>,
    enabled: bool,
    context: &mut ViewContext<Editor>,
    map: impl Fn(Option<f64>) -> Msg + 'static,
) -> NumberBox {
    NumberBox::new()
        .value(value)
        .is_enabled(enabled)
        .on_value_changed(context.callback(map))
}
