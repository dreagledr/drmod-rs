//! The inspector: the run controls, the rules they apply, and what the game is doing.
//!
//! The rules are **not part of the script**: they are the mod's state for one run, so they live in
//! the editor's settings and no `.tas` file is rewritten to hold them. What the panel shows about
//! the game is read back from the mod rather than remembered from the click.

use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{
    Button, ButtonVariant, Checkbox, ControlSize, Kit, SegmentedControl, Switch, TextInput,
};

use crate::api::{FpsCapMode, GameStatus};
use crate::app::App;
use crate::settings::{cap_from_index, cap_index};

pub fn panel(app: &App) -> El {
    let k = &app.kit;

    let content = col()
        .key("run-content")
        .w_full()
        .padding(10.0)
        .gap(10.0)
        .child(heading(k, "ПРОГОН"))
        .child(buttons(app, k))
        .child(status_line(app, k))
        .child(divider(k))
        .child(heading(k, "ПРАВИЛА"))
        .child(rules(app, k))
        .child(divider(k))
        .child(heading(k, "ИГРА"))
        .child(game(app, k))
        .child(divider(k))
        .child(heading(k, "МОД"))
        .child(
            text(
                "Установка мода перенесена не полностью: этот эксперимент умеет работать с уже \
                 внедрённым модом, но не кладёт DLL в папку игры.",
            )
            .font(k.ty.small)
            .color(k.c.text_faint)
            .wrap()
            .w_full()
            .max_w(300.0),
        )
        .child(
            Button::new("Подробнее…")
                .control_size(ControlSize::Small)
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| s.mod_open = true)
                .build(k),
        );

    col()
        .key("run-panel")
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}

fn heading(k: &Kit, label: &str) -> El {
    text(label).font(k.ty.small_medium).color(k.c.text_faint)
}

fn divider(k: &Kit) -> El {
    rune::el()
        .w_full()
        .h(1.0)
        .fill(k.c.border_muted)
        .opacity(0.6)
}

fn buttons(app: &App, k: &Kit) -> El {
    let can_run = app.can_run();
    let can_apply = app.game.online && !app.preparing && !app.worker.busy();
    let can_cancel = app.game.script_active() && !app.worker.busy();

    row()
        .w_full()
        .gap(4.0)
        .child(
            Button::new("Сохранить")
                .control_size(ControlSize::Small)
                .disabled(!app.dirty())
                .on_press(|s: &mut App| s.save())
                .build(k)
                .key("run-save")
                .grow(1.0),
        )
        .child(
            Button::new("Запуск")
                .control_size(ControlSize::Small)
                .variant(ButtonVariant::Submit)
                .disabled(!can_run)
                .on_press(|s: &mut App| s.run())
                .build(k)
                .key("run-start")
                .grow(1.0),
        )
        .child(
            Button::new("Правила")
                .control_size(ControlSize::Small)
                .variant(ButtonVariant::Ghost)
                .disabled(!can_apply)
                .on_press(|s: &mut App| s.apply_rules())
                .build(k)
                .key("run-apply")
                .grow(1.0),
        )
        .child(
            Button::new("Стоп")
                .control_size(ControlSize::Small)
                .variant(ButtonVariant::Danger)
                .disabled(!can_cancel)
                .on_press(|s: &mut App| s.cancel_run())
                .build(k)
                .key("run-cancel")
                .grow(1.0),
        )
}

fn status_line(app: &App, k: &Kit) -> El {
    let (line, color) = if app.preparing || app.worker.busy() {
        ("Работаю…".to_owned(), k.c.text_accent)
    } else if let Some(error) = &app.run_error {
        (error.clone(), k.c.danger.fg)
    } else if let Some(message) = &app.run_message {
        (message.clone(), k.c.success.fg)
    } else if let Some(message) = &app.message {
        (message.clone(), k.c.text_muted)
    } else if app.dirty() {
        ("Есть несохранённые изменения".to_owned(), k.c.text_faint)
    } else {
        ("Готов".to_owned(), k.c.text_faint)
    };

    text(line).font(k.ty.small).color(color).wrap().w_full()
}

fn rules(app: &App, k: &Kit) -> El {
    let mut column = col().w_full().gap(8.0).child(
        Checkbox::new(app.rules.fixed_tick)
            .label("Фиксированный тик 1/60")
            .on_toggle(|s: &mut App| {
                s.rules.fixed_tick = !s.rules.fixed_tick;
                s.save_settings();
            })
            .build(k),
    );

    column = column.child(
        SegmentedControl::new(
            &["кап игры", "без лимита", "свой"],
            Some(cap_index(app.rules.cap)),
        )
        .label("Кап кадров")
        .width(None)
        .on_pick(|s: &mut App, index, _tx| {
            s.set_cap(cap_from_index(index));
        })
        .build(k),
    );

    if app.rules.cap == FpsCapMode::Custom {
        column = column.child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .child(text("fps").font(k.ty.small).color(k.c.text_muted))
                .child(
                    TextInput::new(&app.fps)
                        .label("Кадров в секунду")
                        .width(Some(90.0))
                        .on_edit(|s: &mut App| &mut s.fps)
                        .on_change(|s: &mut App, _tx| {
                            if let Ok(fps) = s.fps.text.trim().parse::<u32>() {
                                s.rules.custom_fps = fps.clamp(
                                    crate::api::PlaybackRules::MIN_FPS,
                                    crate::api::PlaybackRules::MAX_FPS,
                                );
                                s.save_settings();
                            }
                        })
                        .build(k),
                ),
        );
    }

    column = column.child(
        Checkbox::new(app.rules.pin_seed)
            .label("Заморозить сид")
            .on_toggle(|s: &mut App| {
                s.rules.pin_seed = !s.rules.pin_seed;
                s.save_settings();
            })
            .build(k),
    );

    if app.rules.pin_seed {
        column = column.child(
            TextInput::new(&app.seed)
                .label("Сид")
                .width(None)
                .placeholder("0x55555555")
                .on_edit(|s: &mut App| &mut s.seed)
                .on_change(|s: &mut App, _tx| {
                    if let Some(seed) = crate::api::PlaybackRules::try_seed(&s.seed.text) {
                        s.rules.seed = seed;
                        s.save_settings();
                    }
                })
                .build(k),
        );
    }

    column.child(
        Switch::new(app.rules.headless)
            .label("Headless (без окна)")
            .on_toggle(|s: &mut App| {
                s.rules.headless = !s.rules.headless;
                s.save_settings();
            })
            .build(k),
    )
}

fn game(app: &App, k: &Kit) -> El {
    let mut column = col().w_full().gap(4.0);

    if !app.game.online {
        return column.child(
            text("Мод не отвечает. Редактор работает, запуск — нет.")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .wrap()
                .w_full(),
        );
    }

    let menu = if app.game.menu_status.is_empty() {
        "—".to_owned()
    } else {
        app.game.menu_status.clone()
    };

    column = column.child(field(k, "Меню", &menu)).child(field(
        k,
        "FPS",
        &format!("{:.0}", app.game.fps),
    ));

    if !app.game.mission_name.is_empty() {
        column = column.child(field(
            k,
            "Миссия",
            &format!("{} ({})", app.game.mission_name, app.game.mission_id),
        ));
    }

    if let Some(script) = &app.game.script {
        column = column.child(field(k, "Скрипт", &script.name)).child(field(
            k,
            "Фаза",
            &format!(
                "{} · {}/{}",
                script.phase.word(),
                script.frame,
                script.total_frames
            ),
        ));
    }

    if app.game.script_active() {
        column = column.child(
            text("Слот мода занят")
                .font(k.ty.small)
                .color(k.c.text_accent),
        );
    }

    column
}

fn field(k: &Kit, label: &str, value: &str) -> El {
    row()
        .w_full()
        .gap(6.0)
        .child(text(label).font(k.ty.small).color(k.c.text_faint).w(64.0))
        .child(
            text(value)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .grow(1.0)
                .truncate(),
        )
}

/// Keeps [`GameStatus`] in the import list: the inspector's model is the API's own status type.
#[allow(unused)]
fn _status(_: &GameStatus) {}
