//! TAS Editor (Rust): the C# editor's functionality, on the `dear-app` + `dear-imgui-cte` stack.
//!
//! Проект вырос из спайка стека: сначала здесь были только докспейс, таблица в 20 000 строк и
//! редактор `dear-imgui-cte`. Теперь это редактор: рабочая папка со скриптами `.tas`, таблица
//! команд, текст скрипта и окно управления прогоном, которое говорит с модом по HTTP.
//!
//! Стек: `dear-app` (окно, докспейс, wgpu) + `dear-imgui-cte` (текстовый редактор).
//! Порт делался с C#-редактора (`../tas-editor-cs/`) — он же эталон по функциональности; формат
//! скриптов сверяется с его golden-файлами байт в байт (`tests/golden.rs`).
//!
//! Запуск: `cargo run`; `SPIKE_SECONDS=10 cargo run` останавливает цикл через 10 секунд.
//!
//! Модули живут в библиотеке (`src/lib.rs`) — там же их и тестируют, без окна; здесь только
//! само приложение: окно, цикл кадра и редактор текста.

use std::time::Instant;

use dear_app::{
    AppConfig, Application, DockingConfig, FrameContext, InitContext, PrepareFrameContext,
    RunError, Theme,
};
use dear_app::imgui;
use dear_imgui_cte::{
    AutocompleteConfig, AutocompleteRequest, Language, Palette, TextEditor, dejavu_font_source,
};

use tas_editor_rs::editor::shell::{self, ShellState};
use tas_editor_rs::editor::text_pane::autocomplete_tokens;
use tas_editor_rs::editor::{Editor, FrameActions, PendingFolder};

/// Раскладка докспейса и `.ini` рядом с exe: файл рабочий, а не пользовательский, и его
/// удобно удалить, чтобы вернуть дефолт.
const LAYOUT_INI: &str = "tas-editor-layout.ini";

/// Сколько раз в секунду холостой цикл: это потолок против простоя, а не частота экрана.
/// Без потолка цикл крутится на ~1500 fps и греет поток впустую; `SPIKE_FPS=0` его снимает —
/// только тогда виден настоящий потолок стека.
const FPS_CAP: f64 = 200.0;

/// Приложение редактора: держит редактор текста CTE (он создаётся в `configure_imgui` — это
/// единственный хук, дающий `&mut Context`) и замер кадра, которым стек проверялся.
struct Spike {
    /// Сам редактор: состояние, действия и опрос мода.
    editor: Editor,
    /// Состояние оболочки: какие панели открыты, какая раскладка подана, что загружено
    /// в текстовый редактор.
    shell: ShellState,
    /// Текстовый редактор CTE. Создаётся в `configure_imgui`, потому что `try_create` требует
    /// `&mut Context`.
    editor_text: Option<TextEditor>,
    /// Ошибка создания/настройки редактора, показывается вместо него.
    editor_error: Option<String>,
    /// Тема переключена и ждёт применения в `prepare_frame` (там есть стиль контекста).
    theme_dirty: bool,
    /// Моноширинный DejaVu из CTE, добавленный в атлас шрифтов: им рисуется редактор.
    font_id: Option<dear_app::imgui::FontId>,
    /// Потолок частоты кадров. `None` — без потолка (только для замера стека).
    fps_cap: Option<f64>,
    /// Когда началась отрисовка текущего кадра: из бюджета `fps_cap` вычитается ровно это
    /// время, поэтому пауза не зависит от того, сколько заняла отрисовка.
    draw_start: Option<Instant>,
    started: Instant,
    limit_secs: Option<f64>,
    last_frame: Option<Instant>,
    report_start: Instant,
    since_report: u64,
    report_ms: f64,
    frames: u64,
    total_ms: f64,
    min_ms: f64,
    max_ms: f64,
    /// Время внутри `frame()` на подачу UI — без `Present` и без ожидания кадра: и то и другое
    /// делает `dear-app` уже после возврата из колбэка, и из кадра их не видно.
    ui_total_ms: f64,
    ui_max_ms: f64,
}

impl Spike {
    /// Дождаться конца бюджета кадра, если потолок задан.
    ///
    /// Вызывается в конце `frame()`, уже после отрисовки. Занятое время вычитаем из бюджета:
    /// спим только на остаток, иначе на медленном кадре цикл простаивал бы лишнее и уплывал
    /// ниже потолка. Потолок держим сном, а не vsync: при перемещении окна система уводит поток
    /// в модальный цикл, развёртка пропадает и `Present(1)` ждёт событие флипа, которого нет.
    fn wait_for_cap(&self) {
        let Some(budget_ms) = self.fps_cap.map(|fps| 1000.0 / fps) else {
            return;
        };
        let Some(draw_start) = self.draw_start else {
            return;
        };
        let remaining_ms = budget_ms - draw_start.elapsed().as_secs_f64() * 1000.0;
        if remaining_ms > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(remaining_ms / 1000.0));
        }
    }

    /// Интервал между кадрами — настоящая цена кадра, вместе с present.
    ///
    /// ⚠️ **Отсчёт окна начинается с первого кадра, а не с запуска процесса.** Иначе в первое
    /// окно попадает инициализация (`wgpu`, атлас шрифтов, создание окон) — это секунды, за
    /// которые не нарисовано ни одного кадра, — и `fps` выходит заниженным: не `1/интервал`, а
    /// `кадры / (старт + интервал)`. Замерено: окно 5 с печатало 113–127 fps при среднем интервале
    /// 7.1–8.0 ms (то есть 125–141 fps) — разница и была инициализацией.
    ///
    /// `report_ms` копится **на каждом** кадре, включая отчётный, поэтому `кадры × avg` в точности
    /// равно `span`: два числа в строке не могут разойтись.
    fn record_frame_time(&mut self) {
        let now = Instant::now();
        let Some(previous) = self.last_frame.replace(now) else {
            // Первый кадр: он задаёт начало отсчёта, а не измеряется — интервала до него нет.
            self.report_start = now;
            return;
        };
        let ms = (now - previous).as_secs_f64() * 1000.0;

        self.frames += 1;
        if ms > 100.0 {
            println!(
                "[editor] СТОП {ms:.0} ms перед кадром {} (t={:.2}s)",
                self.frames,
                self.started.elapsed().as_secs_f64()
            );
        }

        self.total_ms += ms;
        self.min_ms = self.min_ms.min(ms);
        self.max_ms = self.max_ms.max(ms);
        self.since_report += 1;
        self.report_ms += ms;

        let since = now - self.report_start;
        if since.as_secs_f64() >= 5.0 {
            let seconds = since.as_secs_f64();
            println!(
                "[editor] {} кадров за {:.1}s: {:.1} fps, кадр avg {:.2} ms \
                 (min {:.2}, max {:.2}), UI avg {:.2} ms (max {:.2})",
                self.since_report,
                seconds,
                self.since_report as f64 / seconds,
                self.report_ms / self.since_report as f64,
                self.min_ms,
                self.max_ms,
                self.ui_total_ms / self.since_report as f64,
                self.ui_max_ms,
            );
            self.report_start = now;
            self.since_report = 0;
            self.report_ms = 0.0;
            self.min_ms = f64::MAX;
            self.max_ms = 0.0;
            self.ui_total_ms = 0.0;
            self.ui_max_ms = 0.0;
        }
    }

    /// Применить то, что решил кадр: панели рисуются по `&`-заимствованиям состояния редактора,
    /// поэтому менять его внутри кадра нельзя — все действия собираются и применяются здесь,
    /// по одному разу на кадр.
    fn apply(
        &mut self,
        actions: FrameActions,
        context: &mut FrameContext<'_>,
    ) {
        let editor = &mut self.editor;

        // Рабочая папка.
        if actions.workspace.choose_folder {
            editor.pending_folder = PendingFolder::Workspace;
        }

        if let Some(index) = actions.workspace.select
            && let Some(script) = editor.listing.scripts.get(index)
        {
            editor.selected = Some(script.path.clone());
        }

        if actions.workspace.new_script {
            editor.new_script();
        }

        if actions.workspace.duplicate {
            editor.duplicate_script();
        }

        if actions.workspace.rename
            && let Some(script) = editor.selected().cloned()
        {
            self.shell.rename_field = script.name.clone();
            editor.pending_rename = Some(script);
        }

        if actions.workspace.delete
            && let Some(script) = editor.selected().cloned()
        {
            editor.pending_delete = Some(script);
        }

        // Мод.
        if actions.workspace.mod_choose_folder {
            editor.pending_folder = PendingFolder::GameFolder;
        }

        if actions.workspace.mod_detect {
            editor.detect_game_folder();
        }

        if actions.workspace.mod_install {
            editor.install_mod();
        }

        if actions.workspace.mod_uninstall {
            editor.pending_mod_remove = true;
        }

        // Диалоги.
        if actions.cancel_delete {
            editor.pending_delete = None;
        }

        if actions.confirm_delete {
            editor.delete_confirmed();
        }

        if actions.cancel_rename {
            editor.pending_rename = None;
        }

        if actions.confirm_rename {
            editor.rename_text = self.shell.rename_field.clone();
            editor.rename_confirmed();
        }

        if actions.cancel_mod_remove {
            editor.pending_mod_remove = false;
        }

        if actions.confirm_mod_remove {
            editor.remove_mod_confirmed();
        }

        // Правка текста: редактор отчитался, значит его текст и есть буфер выбранного скрипта.
        // Читается здесь, а не в кадре — `text()` требует `&mut` редактора, а кадр рисует
        // по `&`.
        if actions.script.text.changed
            && let Some(text_editor) = self.editor_text.as_mut()
            && let Ok(text) = text_editor.text()
        {
            editor.typed(text);
        }

        // Правила прогона: регион отдаёт их назад в поле оболочки, а здесь они становятся
        // настройками. Так поле ввода и значение не расходятся.
        if actions.script.controls.seed_changed {
            editor.set_seed_text(self.shell.seed_field.clone());
        }

        if actions.script.controls.rules_changed
            && let Some(rules) = self.shell.rules_out.take()
        {
            editor.set_rules(rules);
        }

        // Прогон.
        if actions.script.controls.save {
            editor.save_script();
        }

        if actions.script.controls.run {
            editor.run();
        }

        if actions.script.controls.apply {
            editor.apply_rules();
        }

        if actions.script.controls.cancel {
            editor.cancel_run();
        }

        if actions.quit {
            context.request_exit();
        }

        // Тема применяется в `prepare_frame`: здесь есть только `&Ui`, а смена темы требует
        // стиль контекста.
        if actions.toggle_theme {
            self.editor.settings.light_theme = !self.editor.settings.light_theme;
            self.editor.save_settings();
            self.theme_dirty = true;
        }
    }
}

impl Application for Spike {
    fn configure_imgui(&mut self, context: &mut InitContext<'_>) -> Result<(), RunError> {
        let imgui_context = context.imgui();

        match imgui_context.set_ini_filename(Some(std::path::PathBuf::from(LAYOUT_INI))) {
            Ok(()) => println!("[editor] раскладка сохраняется в {LAYOUT_INI}"),
            Err(error) => println!("[editor] раскладка не сохраняется: {error}"),
        }

        // Шрифт кладём в атлас здесь: это хук «до инициализации рендерера», а рендерер забирает
        // атлас себе уже после него.
        match dejavu_font_source(16.0) {
            Ok(source) => {
                self.font_id = Some(imgui_context.font_atlas().add_font(&[source]));
                println!("[editor] шрифт редактора: моноширинный DejaVu из CTE");
            }
            Err(error) => {
                println!("[editor] шрифт не добавлен: {error}");
                self.editor_error = Some(format!("dejavu_font_source: {error}"));
            }
        }

        match TextEditor::try_create(imgui_context) {
            Ok(mut editor_text) => {
                println!("[editor] редактор CTE создан");
                if let Err(error) = editor_text.set_palette(&Palette::dark()) {
                    self.editor_error = Some(format!("set_palette: {error}"));
                }

                // Свой язык зарегистрировать нельзя (у `Language` нет публичного конструктора
                // в C-API), а у Python маркер однострочного комментария — `#`, тот же, что
                // в `.tas` (`docs/SCRIPT_DSL.md` §1). Ни один токен DSL не совпадает с
                // ключевыми словами Python, поэтому подсветка идёт без ложных срабатываний.
                editor_text.set_language(Some(Language::Python));
                println!("[editor] язык редактора: Python — из-за маркера комментария `#`");

                // Подсказки — токены DSL, а не идентификаторы документа: у `.tas` роль словаря
                // играют имена токенов.
                let config = AutocompleteConfig::new();
                match editor_text.set_autocomplete(&config, |request: &mut AutocompleteRequest<'_>| {
                    let _ = request.set_suggestions(autocomplete_tokens());
                }) {
                    Ok(()) => println!(
                        "[editor] автодополнение: {} токенов DSL по Ctrl+Space",
                        autocomplete_tokens().len()
                    ),
                    Err(error) => self.editor_error = Some(format!("set_autocomplete: {error}")),
                }

                // Первый скрипт рабочей папки загружается сразу: редактор и таблица без текста
                // молчат, а пустой выбор в списке читается как «ничего не выбрано».
                let text = self.editor.selected_text();
                if let Err(error) = editor_text.set_text(&text) {
                    self.editor_error = Some(format!("set_text: {error}"));
                }

                self.editor_text = Some(editor_text);
                self.shell.loaded = self.editor.selected.clone();
            }
            Err(error) => {
                println!("[editor] редактор CTE не создан: {error}");
                self.editor_error = Some(format!("try_create: {error}"));
            }
        }

        Ok(())
    }

    /// Единственное место, где открывается блокирующий системный диалог: `dear-app` зовёт этот
    /// хук **до** того, как ImGui откроет следующий кадр, поэтому окно на время показа диалога
    /// не держит незакрытый кадр.
    ///
    /// Здесь же применяется тема: `theme.apply_to_context` требует `&mut Context`, а этот хук
    /// даёт стиль контекста — в теле кадра есть только `&Ui`. Тема переписывается целиком, когда
    /// её переключили, и не трогается иначе: у ImGui стиль один на контекст, и подавать его
    /// каждый кадр значило бы затирать всё, что автор настроил в `.ini`.
    fn prepare_frame(&mut self, context: &mut PrepareFrameContext<'_>) -> Result<(), RunError> {
        self.editor.run_pending_dialog();

        if std::mem::take(&mut self.theme_dirty) {
            let preset = if self.editor.settings.light_theme {
                imgui::ThemePreset::Light
            } else {
                imgui::ThemePreset::Dark
            };
            imgui::Theme {
                preset,
                ..Default::default()
            }
            .apply_to_style(context.style_mut());
            println!("[editor] тема: {}", if self.editor.settings.light_theme { "светлая" } else { "тёмная" });
        }

        Ok(())
    }

    fn frame(&mut self, context: &mut FrameContext<'_>) -> Result<(), RunError> {
        self.record_frame_time();
        let ui_started = Instant::now();
        self.draw_start = Some(ui_started);

        if let Some(limit) = self.limit_secs
            && self.started.elapsed().as_secs_f64() >= limit
        {
            println!(
                "[editor] остановка по SPIKE_SECONDS: {} кадров, avg {:.2} ms",
                self.frames,
                self.total_ms / self.frames.max(1) as f64
            );
            context.request_exit();
            return Ok(());
        }

        let ui = context.ui();

        // Опрос игры: работа уходит на отдельный поток, сюда возвращается только последний
        // ответ. Делается до отрисовки, чтобы панель показывала состояние этого кадра.
        self.editor.poll();

        if let Some(outcome) = self.editor.take_run_outcome() {
            self.editor.absorb(outcome);
        }

        // Подготовка панелей к кадру: поля ввода живут в состоянии оболочки, а правила,
        // которые регион правит, возвращаются в `rules_out`.
        self.shell.seed_field = self.editor.settings.seed_text.clone();
        self.shell.rules_out = None;

        let mut actions = FrameActions::default();
        let text_editor = self
            .editor_text
            .as_mut()
            .expect("редактор CTE создан в configure_imgui");

        shell::frame(
            ui,
            &self.editor,
            &mut self.shell,
            text_editor,
            &mut actions,
        );
        self.apply(actions, context);

        let ui_ms = ui_started.elapsed().as_secs_f64() * 1000.0;
        self.ui_total_ms += ui_ms;
        self.ui_max_ms = self.ui_max_ms.max(ui_ms);

        self.wait_for_cap();

        Ok(())
    }
}

fn main() -> Result<(), RunError> {
    let limit_secs = std::env::var("SPIKE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok());

    let fps_cap = match std::env::var("SPIKE_FPS").ok().as_deref() {
        Some("0") => None,
        Some(value) => value.parse::<f64>().ok().filter(|fps| *fps > 0.0),
        None => Some(FPS_CAP),
    };

    let editor = Editor::new();
    println!(
        "[editor] рабочая папка: {}",
        editor
            .folder
            .as_ref()
            .map(|folder| folder.display().to_string())
            .unwrap_or_else(|| "не выбрана".to_owned())
    );
    println!(
        "[editor] скриптов в папке: {}, игровая папка: {}",
        editor.listing.scripts.len(),
        editor
            .game_folder
            .as_ref()
            .map(|folder| folder.display().to_string())
            .unwrap_or_else(|| "не найдена".to_owned())
    );
    println!(
        "[editor] payload вшит: {} KiB мода + {} KiB загрузчика",
        editor.payload.asi_kib(),
        editor.payload.loader_kib()
    );

    let config = AppConfig {
        window_title: "TAS Editor".to_owned(),
        window_size: (1440.0, 900.0),
        // Докинг включён, но хост рисуем **мы** — внутри главного окна. Свой хост `dear-app`
        // здесь был бы вторым докспейсом и ломал бы ввод (проверено).
        docking: DockingConfig::application_managed(),
        theme: Some(Theme::Dark),
        // Без vsync: частоту держит наш сон по бюджету кадра (`fps_cap`).
        present_mode: dear_app::wgpu::PresentMode::AutoNoVsync,
        ..AppConfig::default()
    };

    let now = Instant::now();
    let spike = Spike {
        editor,
        // Состояние оболочки: все панели открыты, раскладка подаётся при первом кадре.
        shell: ShellState::default(),
        editor_text: None,
        editor_error: None,
        theme_dirty: false,
        font_id: None,
        fps_cap,
        draw_start: None,
        started: now,
        limit_secs,
        last_frame: None,
        report_start: now,
        since_report: 0,
        report_ms: 0.0,
        frames: 0,
        total_ms: 0.0,
        min_ms: f64::MAX,
        max_ms: 0.0,
        ui_total_ms: 0.0,
        ui_max_ms: 0.0,
    };

    dear_app::run(config, spike)
}
