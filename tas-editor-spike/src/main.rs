//! Спайк: таблица на 20 000 строк + редактор кода на `dear-imgui-rs` / `dear-imgui-cte`.
//!
//! Проверяем четыре вещи, от которых зависит выбор стека для TAS-редактора:
//! 1. тянет ли `TableBuilder` + `ListClipper` таблицу на 20 000 кадров с данными и
//!    редактируемыми ячейками;
//! 2. годится ли `TextEditor` из `dear-imgui-cte` как редактор `.tas`-скрипта — вместе
//!    с моноширинным шрифтом (`dejavu_font_source`), автодополнением по токенам DSL
//!    и подсветкой (`Language::Python` как приближение: см. `README.md`);
//! 3. стоит ли рантайм `dear-app` своей цены;
//! 4. годится ли `dear-file-browser` для рабочей папки и списка скриптов.
//!
//! Таблица и текст редактора — проекции одного и того же сегмента скрипта
//! (`SEGMENT`), как в настоящем редакторе: таблица реализует текст в кадры.
//! ⚠️ Правка квадратика меняет данные строки и **не** переписывает текст: обратная
//! запись «кадры → текст» в спайке не делается.
//!
//! Спайк сам себя измеряет и завершает, чтобы прогон был воспроизводимым:
//! * `SPIKE_SECONDS=10` — выйти через 10 секунд, печатая раз в секунду и интервал
//!   между кадрами, и цену самой отрисовки;
//! * `SPIKE_NO_CLIPPER=1` — рисовать все 20 000 строк вместо видимого диапазона.
//!   Прогон с клиппером и без него — и есть проверка виртуализации.
//! * `SPIKE_FPS=200` — потолок частоты кадров (по умолчанию 200; `0` снимает потолок,
//!   и тогда интервал между кадрами показывает настоящий потолок стека).
//!
//! Запуск: `SPIKE_SECONDS=10 cargo run` и `SPIKE_SECONDS=10 SPIKE_NO_CLIPPER=1 cargo run`.

use std::path::PathBuf;
use std::time::Instant;

use dear_app::imgui::{
    Condition, DockLayout, DockLayoutApply, DockNodeFlags, DockSplit, FontId, ListClipper,
    TableColumnRef, TableFlags, TableSizingPolicy, Ui, WindowFlags,
};
use dear_app::{
    AppConfig, Application, DockingConfig, FrameContext, InitContext, PrepareFrameContext,
    RunError, Theme,
};
use dear_file_browser::{Backend, DialogMode, FileDialog, FileDialogError};
use dear_imgui_cte::{
    AutocompleteConfig, AutocompleteRequest, CteUiExt, Language, Palette, TextEditor,
    dejavu_font_source,
};

/// Колонки таблицы: `#` плюс токены DSL в порядке текста. Как в C#-редакторе —
/// одна колонка на токен, поэтому шапка служит легендой формата.
const COLUMNS: [&str; 30] = [
    "#", "ls", "lsx", "lsy", "rs", "rsx", "rsy", "a", "b", "x", "y", "by", "lt", "rt", "lb", "rb",
    "r", "lr", "ax", "du", "dd", "dl", "mu", "md", "ml", "mr", "ok", "esc", "cd", "wk",
];

/// Кадров в скрипте — верхняя граница, которую должен пережить редактор.
const ROWS: usize = 20_000;

/// Длина одного сегмента скрипта; он повторяется до конца `ROWS`.
const CYCLE: usize = 300;

/// Правдоподобный TAS-сегмент в форме `.tas`: кадр внутри цикла, токен, значение
/// (пусто — у кнопки значения нет), сколько кадров держать.
///
/// Набор токенов и их смысл — из `docs/SCRIPT_DSL.md` §3.1. Левый стик занят почти
/// каждый кадр (и это единственная из трёх его форм в кадре — §4), кнопки лежат
/// окнами поверх него: так выглядит настоящий TAS, а не решётка из пустых ячеек.
const SEGMENT: &[(usize, &str, &str, usize)] = &[
    // Левый стик: непрерывная намотка угла на весь цикл, вместе с точными осями.
    (0, "ls", "0", 25),
    (25, "ls", "45", 25),
    (50, "lsx", "-1000", 5),
    (55, "ls", "90", 40),
    (95, "ls", "135", 25),
    (120, "ls", "180", 40),
    (160, "ls", "225", 20),
    (180, "lsy", "-1000", 5),
    (185, "ls", "270", 45),
    (230, "ls", "315", 30),
    (260, "ls", "0", 40),
    // Кнопки: окна удержания, свободно перекрываются между колонками.
    (0, "a", "", 2),
    (10, "x", "", 6),
    (20, "x", "", 4),
    (30, "y", "", 18),
    (48, "lt", "", 55),
    (60, "by", "", 5),
    (70, "b", "", 8),
    (85, "du", "", 2),
    (95, "rt", "", 40),
    (100, "wk", "", 30),
    (135, "ax", "", 6),
    (140, "r", "", 1),
    (150, "rb", "", 1),
    (160, "lr", "", 50),
    (175, "dl", "", 3),
    (200, "lb", "", 12),
    (215, "ok", "", 3),
    (225, "esc", "", 1),
    (240, "cd", "", 1),
    // Камера (правый стик) — отдельная ось, с левым стиком не конфликтует.
    (250, "rsx", "800", 10),
    (255, "mu", "", 2),
    (265, "md", "", 2),
    (275, "ml", "", 2),
    (285, "mr", "", 2),
    (290, "dd", "", 1),
];

/// Высота строки таблицы: её же сообщаем клипперу, чтобы он считал видимый диапазон.
const ROW_HEIGHT: f32 = 18.0;

/// Сторона квадрата-флажка, px.
const FLAG_SIZE: f32 = 14.0;

/// Цвета флажка — сплошная заливка вместо галочки: нажато зелёный, отпущено красный.
const FLAG_ON: [f32; 4] = [0.16, 0.72, 0.24, 1.0];
const FLAG_OFF: [f32; 4] = [0.78, 0.18, 0.18, 1.0];

/// Чем рисуется флажок в колонках кнопок.
#[derive(Clone, Copy, PartialEq)]
enum FlagStyle {
    /// Маленький закрашенный квадрат по месту ячейки; остальное — как обычно.
    Square,
    /// Заливка всей ячейки цветом состояния: цветная «сетка» получается сама.
    Cell,
}

/// Ячейка строки: у кнопки значение — только факт нажатия, у стика — число.
#[derive(Clone)]
enum Cell {
    Empty,
    /// Кнопочный токен: рисуется квадратиком (чекбоксом) и правится кликом.
    Flag(bool),
    /// Токен стика: числовое значение, рисуется текстом.
    Value(String),
}

/// Одна строка таблицы: значение по каждой колонке.
type Row = [Cell; COLUMNS.len()];

/// Скрипт в рабочей папке: только то, что показывает список.
///
/// Имя файла не равно `name=` внутри скрипта (`docs/SCRIPT_DSL.md` §2) — список
/// показывает то, что лежит на диске.
struct ScriptFile {
    path: PathBuf,
    name: String,
    size: u64,
    lines: usize,
}

struct Spike {
    /// Редактор создаётся в `configure_imgui` — это единственный хук, дающий `&mut Context`.
    editor: Option<TextEditor>,
    /// Ошибка создания/настройки редактора, показывается вместо него.
    editor_error: Option<String>,
    /// Моноширинный DejaVu из CTE, добавленный в атлас шрифтов: им рисуется редактор.
    font_id: Option<FontId>,
    /// Чем включается автодополнение — для строки состояния.
    autocomplete: Option<String>,
    /// Сколько кадров редактор отчитался как изменённые.
    changed_frames: u64,
    /// Сколько квадратиков переключено в таблице за сессию.
    cell_edits: u64,
    /// Строка состояния под редактором (сколько символов вернулось из `text()`).
    status: String,
    /// Рабочая папка со скриптами (`None` — ещё не выбрана).
    folder: Option<PathBuf>,
    /// Скрипты в папке: список собирается по событию, а не в рендере.
    files: Vec<ScriptFile>,
    /// Выбранный в списке скрипт.
    selected: Option<usize>,
    /// Строка состояния рабочей папки: путь, счётчики или ошибка чтения.
    pane_message: String,
    /// Запрошен диалог выбора папки. Ставится кнопкой, исполняется в `prepare_frame`:
    /// диалог блокирующий, и открывать его посреди кадра ImGui нельзя.
    pending_open: bool,
    /// Сколько длился последний блокирующий диалог. Пока он открыт, цикл кадра стоит —
    /// это ожидаемая пауза, и её надо уметь отличить от настоящего зависания.
    dialog_ms: Option<f64>,
    /// Кадры для таблицы: построены один раз из `SEGMENT`.
    rows: Vec<Row>,
    /// Сколько строк непусты — для строки состояния над таблицей.
    rows_with_input: usize,
    /// Как рисуются флажки в колонках кнопок.
    flag_style: FlagStyle,
    /// Открыто ли каждое окно. Закрытие своего окна прячет часть UI без потери данных,
    /// поэтому состояние живёт здесь, а не внутри ImGui.
    window_open: [bool; 4],
    /// Подана ли уже объявленная раскладка докспейса. Строить её надо **один раз**,
    /// дальше размеры держит сам докспейс — иначе он ругается на повторную подачу.
    dock_layout_applied: bool,
    /// Считать видимый диапазон клиппером (`false` — рисовать все 20 000 строк).
    clipped: bool,
    /// Потолок частоты кадров: рендерить чаще, чем нужно, незачем. `None` — без потолка
    /// (только для замера настоящего потолка стека).
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
    /// Время, потраченное внутри `frame()` на подачу UI — без `Present` и без ожидания
    /// vsync: и то и другое делает `dear-app` уже после возврата из колбэка, и из кадра
    /// их не видно. С включённым vsync интервал между кадрами равен частоте экрана и о
    /// стоимости UI молчит — говорит как раз эта величина.
    ui_total_ms: f64,
    ui_max_ms: f64,
}

impl Spike {
    /// Дождаться конца бюджета кадра, если потолок задан.
    ///
    /// Вызывается в конце `frame()`, уже после отрисовки. Занятое время вычитаем из
    /// бюджета: спим только на остаток, иначе на медленном кадре цикл простаивал бы
    /// лишнее и уплывал ниже потолка.
    fn wait_for_cap(&self) {
        let Some(budget_ms) = self.fps_cap.map(|fps| 1000.0 / fps) else {
            return;
        };
        let Some(draw_start) = self.draw_start else {
            return;
        };
        let remaining_ms = budget_ms - draw_start.elapsed().as_secs_f64() * 1000.0;
        // Отрисовка уже съела бюджет — ждать нечего, кадр и так упёрся в потолок.
        if remaining_ms > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(remaining_ms / 1000.0));
        }
    }

    /// Интервал между кадрами — настоящая цена кадра, вместе с present.
    ///
    /// Пауза открытого диалога из статистики **исключается**: это время пользователя,
    /// а не стоимость кадра, и без исключения любой `max`/`avg` перестаёт что-либо значить.
    fn record_frame_time(&mut self) {
        let now = Instant::now();
        let Some(previous) = self.last_frame.replace(now) else {
            return;
        };
        let ms = (now - previous).as_secs_f64() * 1000.0;

        if let Some(dialog_ms) = self.dialog_ms.take() {
            println!(
                "[spike] пауза {ms:.0} ms — открыт диалог выбора папки (сам диалог {dialog_ms:.0} ms), из статистики исключена"
            );
            return;
        }

        self.frames += 1;
        if ms > 100.0 {
            println!(
                "[spike] СТОП {ms:.0} ms перед кадром {} (t={:.2}s) — ничем не объяснён",
                self.frames,
                self.started.elapsed().as_secs_f64()
            );
        }

        self.total_ms += ms;
        self.min_ms = if self.frames == 1 { ms } else { self.min_ms.min(ms) };
        self.max_ms = self.max_ms.max(ms);
        self.since_report += 1;
        self.report_ms += ms;

        let elapsed = self.report_start.elapsed().as_secs_f64();
        if elapsed >= 1.0 && self.since_report > 0 {
            let ui_avg = self.ui_total_ms / self.since_report as f64;
            println!(
                "[spike] clipped={} {:.1} fps, {:.2} ms/frame, UI {:.2} ms ({} frames in {:.2}s)",
                self.clipped,
                self.since_report as f64 / elapsed,
                self.report_ms / self.since_report as f64,
                ui_avg,
                self.since_report,
                elapsed
            );
            self.report_start = Instant::now();
            self.since_report = 0;
            self.report_ms = 0.0;
            self.ui_total_ms = 0.0;
        }
    }

    fn print_summary(&self) {
        if self.frames > 0 {
            println!(
                "[spike] SUMMARY clipped={} frames={} avg={:.2}ms min={:.2}ms max={:.2}ms, UI avg={:.2}ms max={:.2}ms",
                self.clipped,
                self.frames,
                self.total_ms / self.frames as f64,
                self.min_ms,
                self.max_ms,
                self.ui_total_ms / self.frames as f64,
                self.ui_max_ms
            );
        } else {
            println!("[spike] SUMMARY clipped={} — кадров не набралось", self.clipped);
        }
        // Итог прогона — в stdout, а не только в окно: из логов его видно сразу.
        println!(
            "[spike] ИТОГ: правок в таблице {}, правок в тексте {}",
            self.cell_edits, self.changed_frames
        );
    }

    /// Перечитать список `.tas` в рабочей папке.
    ///
    /// Диск читается по событию (открытие папки, «Обновить»), а не в рендере, и ничего
    /// не бросает: любая неудача возвращается строкой в `pane_message`.
    fn rescan(&mut self) {
        self.files.clear();
        self.selected = None;

        let Some(folder) = self.folder.clone() else {
            self.pane_message = "папка не выбрана".to_owned();
            return;
        };

        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                self.pane_message = format!("{}: {error}", folder.display());
                return;
            }
        };

        let mut files = Vec::new();
        let mut unreadable = 0usize;
        for entry in entries {
            let Ok(entry) = entry else {
                unreadable += 1;
                continue;
            };
            let path = entry.path();
            let is_tas = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("tas"));
            if !is_tas || !path.is_file() {
                continue;
            }

            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            // Файл, который не читается, остаётся в списке с нулём строк — как в C#-редакторе,
            // где такая строка сама сообщает о себе, а не исчезает.
            let lines = std::fs::read_to_string(&path)
                .map(|text| text.lines().count())
                .unwrap_or(0);
            files.push(ScriptFile {
                path,
                name,
                size,
                lines,
            });
        }
        files.sort_by(|left, right| left.name.cmp(&right.name));

        let skipped = if unreadable > 0 {
            format!(", записей не прочитано: {unreadable}")
        } else {
            String::new()
        };
        self.pane_message = format!("{}: скриптов {}{skipped}", folder.display(), files.len());
        self.folder = Some(folder);
        self.files = files;
    }

    /// Показать системный диалог выбора папки.
    ///
    /// Вызывается из `prepare_frame`, а не из рендера: диалог блокирующий, и держать
    /// открытым кадр ImGui на время его показа нельзя. Отмена — не ошибка, о ней
    /// `open_blocking` сообщает вариантом `Cancelled`, а не пустым `Ok`.
    fn pick_folder(&mut self) {
        let mut dialog = FileDialog::new(DialogMode::PickFolder).backend(Backend::Native);
        if let Some(folder) = &self.folder {
            dialog = dialog.directory(folder.clone());
        }

        let started = Instant::now();
        let result = dialog.open_blocking();
        // Цикл кадра стоит ровно столько, сколько диалог открыт: это ожидаемая пауза,
        // и её длительность мы запоминаем, чтобы подписать ею следующий интервал.
        self.dialog_ms = Some(started.elapsed().as_secs_f64() * 1000.0);

        match result {
            Ok(selection) => match selection.file_path_name() {
                Some(path) => {
                    self.folder = Some(path.to_path_buf());
                    self.rescan();
                }
                None => self.pane_message = "диалог вернул пустой выбор".to_owned(),
            },
            Err(FileDialogError::Cancelled) => {
                self.pane_message = "выбор папки отменён".to_owned();
            }
            Err(error) => self.pane_message = format!("диалог: {error}"),
        }
    }

    /// Загрузить текст скрипта в редактор CTE.
    fn load_into_editor(&mut self, index: usize) {
        let Some(file) = self.files.get(index) else {
            return;
        };
        let path = file.path.clone();
        let name = file.name.clone();

        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.selected = Some(index);
                self.pane_message = format!("{name}: {} символов", text.chars().count());
                match &mut self.editor {
                    Some(editor) => {
                        if let Err(error) = editor.set_text(&text) {
                            self.pane_message = format!("{name}: set_text: {error}");
                        }
                    }
                    None => self.pane_message = format!("{name}: редактор недоступен"),
                }
            }
            Err(error) => self.pane_message = format!("{name}: {error}"),
        }
    }
}

impl Application for Spike {
    fn configure_imgui(&mut self, context: &mut InitContext<'_>) -> Result<(), RunError> {
        let imgui_context = context.imgui();

        // Сохранение раскладки: без `.ini` ImGui не помнит ни позиций окон, ни размеров
        // докспейса, и раскладка теряется при закрытии. Кладём рядом с exe — файл рабочий,
        // а не пользовательский, и его удобно удалить, чтобы вернуть дефолт.
        match imgui_context.set_ini_filename(Some(std::path::PathBuf::from("spike-layout.ini"))) {
            Ok(()) => println!("[spike] раскладка сохраняется в spike-layout.ini"),
            Err(error) => println!("[spike] раскладка не сохраняется: {error}"),
        }

        // Шрифт кладём в атлас здесь: это хук «до инициализации рендерера», а рендерер
        // забирает атлас себе уже после него.
        match dejavu_font_source(16.0) {
            Ok(source) => {
                self.font_id = Some(imgui_context.font_atlas().add_font(&[source]));
                println!("[spike] шрифт редактора: моноширинный DejaVu из CTE добавлен в атлас");
            }
            Err(error) => {
                println!("[spike] шрифт не добавлен: {error}");
                self.editor_error = Some(format!("dejavu_font_source: {error}"));
            }
        }

        match TextEditor::try_create(imgui_context) {
            Ok(mut editor) => {
                println!("[spike] редактор CTE создан");
                if let Err(error) = editor.set_text(&script_text()) {
                    self.editor_error = Some(format!("set_text: {error}"));
                }
                if let Err(error) = editor.set_palette(&Palette::dark()) {
                    self.editor_error = Some(format!("set_palette: {error}"));
                }
                // Свой язык зарегистрировать нельзя (у `Language` нет публичного конструктора
                // в C-API), а у Python маркер однострочного комментария — `#`, тот же, что
                // в `.tas` (`docs/SCRIPT_DSL.md` §1). Ни один токен DSL не совпадает с
                // ключевыми словами Python, поэтому подсветка идёт без ложных срабатываний.
                editor.set_language(Some(Language::Python));
                println!("[spike] язык редактора: Python — из-за маркера комментария `#`");
                // Свой список подсказок вместо встроенного три: три собирает идентификаторы
                // из документа, а у `.tas` роль словаря играют имена токенов (§3.1).
                let config = AutocompleteConfig::new();
                match editor.set_autocomplete(&config, |request: &mut AutocompleteRequest<'_>| {
                    let tokens = COLUMNS.iter().copied().filter(|token| *token != "#");
                    let _ = request.set_suggestions(tokens);
                }) {
                    Ok(()) => {
                        self.autocomplete =
                            Some(format!("{} токенов DSL, Ctrl+Space", COLUMNS.len() - 1));
                        println!(
                            "[spike] автодополнение: {} токенов DSL по Ctrl+Space",
                            COLUMNS.len() - 1
                        );
                    }
                    Err(error) => {
                        println!("[spike] автодополнение не встало: {error}");
                        self.editor_error = Some(format!("set_autocomplete: {error}"));
                    }
                }
                self.editor = Some(editor);
            }
            Err(error) => {
                println!("[spike] редактор CTE не создан: {error}");
                self.editor_error = Some(format!("try_create: {error}"));
            }
        }

        Ok(())
    }

    /// Единственное место, где открывается блокирующий системный диалог: `dear-app`
    /// зовёт этот хук **до** того, как ImGui откроет следующий кадр, поэтому окно
    /// на время показа диалога не держит незакрытый кадр.
    fn prepare_frame(&mut self, _context: &mut PrepareFrameContext<'_>) -> Result<(), RunError> {
        if self.pending_open {
            self.pending_open = false;
            self.pick_folder();
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
            self.print_summary();
            context.request_exit();
            return Ok(());
        }

        let ui = context.ui();

        // **Главное окно — хост докспейса**, а панели докаются в него: `TAS Editor Spike`
        // это одно окно со своим меню и содержимым, а не пустая полоса меню рядом с
        // набором панелей. Докспейс подаётся внутри этого окна на всё его место.
        //
        // Окно занимает весь вьюпорт и **не может** быть сдвинуто, растянуто или свёрнуто:
        // оно и есть рабочая область приложения. Размер берётся у вьюпорта каждый кадр, так
        // что растягивание системного окна ведёт за собой и панели внутри.
        //
        // ⚠️ Раскладка подаётся один раз при первом кадре, сам докспейс — каждый кадр:
        // повторная подача раскладки даёт `dockspace ... was already submitted`.
        let viewport = ui.main_viewport();
        ui.set_next_window_viewport(viewport.id());
        let dock_id = ui.get_id(DOCKSPACE_ID);
        ui.window(WINDOW_TITLE)
            .flags(
                WindowFlags::MENU_BAR
                    | WindowFlags::NO_TITLE_BAR
                    | WindowFlags::NO_MOVE
                    | WindowFlags::NO_RESIZE
                    | WindowFlags::NO_COLLAPSE
                    | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS
                    | WindowFlags::NO_NAV_FOCUS,
            )
            .position(viewport.pos(), Condition::Always)
            .size(viewport.size(), Condition::Always)
            .build(|| {
                let _ = ui.menu_bar(|| {
                    ui.menu("Вид", || {
                        for (index, name) in WINDOW_NAMES.iter().enumerate() {
                            ui.menu_item_toggle(
                                name,
                                None::<&str>,
                                &mut self.window_open[index],
                                true,
                            );
                        }
                    });
                });

                // Размер докспейса — остаток окна под полосой меню.
                let available = ui.content_region_avail();
                let dockspace = ui
                    .dockspace()
                    .current_window(available)
                    .root_id(dock_id)
                    .flags(DockNodeFlags::PASSTHRU_CENTRAL_NODE);
                let outcome = if self.dock_layout_applied {
                    dockspace.build()
                } else {
                    dockspace
                        .layout(&spike_dock_layout(), DockLayoutApply::IfMissing)
                        .build()
                };
                match outcome {
                    Ok(_) => self.dock_layout_applied = true,
                    Err(error) if !self.dock_layout_applied => {
                        self.dock_layout_applied = true;
                        println!("[spike] докспейс не разложился: {error}");
                    }
                    Err(_) => {}
                }
            });

        // Состояние окон живёт в `self.window_open`, и каждая функция окна берёт его
        // по ссылке прямо в `.opened(..)` — тогда `false` от крестика никуда не теряется,
        // а внешней копии флага не нужно.
        self.window_workspace(ui);
        self.window_table(ui);
        self.window_text(ui);
        self.window_state(ui);

        // Меряем ровно подачу UI: `Present` и ожидание развёртки `dear-app` делает уже
        // после возврата из этого колбэка, поэтому в цифру не попадают.
        let ui_ms = ui_started.elapsed().as_secs_f64() * 1000.0;
        self.ui_total_ms += ui_ms;
        self.ui_max_ms = self.ui_max_ms.max(ui_ms);

        // Потолок держим сном, а не vsync (см. `fps_cap` в `main`).
        self.wait_for_cap();

        Ok(())
    }
}

impl Spike {
    /// Стабильный ключ окна по его индексу в [`WINDOW_NAMES`].
    ///
    /// `WindowKey` держит идентичность отдельно от подписи, поэтому окно и его место
    /// в докспейсе ссылаются на одну и ту же панель, даже если подпись поменяется.
    fn key(slot: usize) -> dear_app::imgui::WindowKey {
        const IDS: [&str; 4] = ["workspace", "table", "text", "state"];
        dear_app::imgui::WindowKey::new(IDS[slot], WINDOW_NAMES[slot])
            .expect("идентификаторы окон заданы без разделителя и не пусты")
    }

    /// Окно рабочей папки и списка скриптов.
    fn window_workspace(&mut self, ui: &Ui) {
        if !self.window_open[WINDOW_WORKSPACE] {
            return;
        }
        // Локальная копия нужна только из-за заимствования: `.opened(&mut ..)` держит
        // `self.window_open` занятым всё время `build`, а тело окна тоже берёт `self`.
        // Запись обратно — после `build`, когда заимствование кончилось.
        let mut opened = self.window_open[WINDOW_WORKSPACE];
        ui.window(Self::key(WINDOW_WORKSPACE).label(WINDOW_NAMES[WINDOW_WORKSPACE]))
            .opened(&mut opened)
            .size([420.0, 320.0], Condition::FirstUseEver)
            .build(|| {
                if ui.button("Открыть папку…") {
                    self.pending_open = true;
                }
                if ui.button("Обновить список") {
                    self.rescan();
                }
                ui.text(&self.pane_message);

                if self.files.is_empty() {
                    return;
                }
                // Клик запоминаем, а применяем после обхода: список взят по `&self.files`,
                // а загрузка скрипта требует `&mut self`.
                let mut clicked = None;
                for (index, file) in self.files.iter().enumerate() {
                    let label = format!("{} — {} строк, {} Б", file.name, file.lines, file.size);
                    let selected = self.selected == Some(index);
                    if ui.selectable_config(label.as_str()).selected(selected).build() {
                        clicked = Some(index);
                    }
                }
                if let Some(index) = clicked {
                    self.load_into_editor(index);
                }
            });
        self.window_open[WINDOW_WORKSPACE] = opened;
    }

    /// Окно таблицы кадров.
    fn window_table(&mut self, ui: &Ui) {
        if !self.window_open[WINDOW_TABLE] {
            return;
        }
        let mut opened = self.window_open[WINDOW_TABLE];
        ui.window(Self::key(WINDOW_TABLE).label(WINDOW_NAMES[WINDOW_TABLE]))
            .opened(&mut opened)
            .size([1100.0, 340.0], Condition::FirstUseEver)
            .build(|| {
                let edits = frames_table(ui, self.clipped, &mut self.rows, self.flag_style);
                self.cell_edits += u64::from(edits);
            });
        self.window_open[WINDOW_TABLE] = opened;
    }

    /// Окно текста скрипта с редактором `dear-imgui-cte`.
    fn window_text(&mut self, ui: &Ui) {
        if !self.window_open[WINDOW_TEXT] {
            return;
        }
        let mut opened = self.window_open[WINDOW_TEXT];
        ui.window(Self::key(WINDOW_TEXT).label(WINDOW_NAMES[WINDOW_TEXT]))
            .opened(&mut opened)
            .size([820.0, 380.0], Condition::FirstUseEver)
            .build(|| {
                match &mut self.editor {
                    Some(editor) => {
                        // Моноширинный шрифт пушим только вокруг редактора: таблица и
                        // подписи остаются шрифтом по умолчанию.
                        let font_token = self.font_id.map(|id| ui.push_font(id));
                        match ui.text_editor(editor, "Source").size([-1.0, -1.0]).build() {
                            Ok(true) => {
                                self.changed_frames += 1;
                                self.status = match editor.text() {
                                    Ok(text) => {
                                        format!("после правки: {} символов", text.chars().count())
                                    }
                                    Err(error) => format!("ошибка чтения: {error}"),
                                };
                            }
                            Ok(false) => {}
                            Err(error) => ui.text(format!("ошибка редактора: {error}")),
                        }
                        drop(font_token);
                    }
                    None => {
                        ui.text("редактор недоступен");
                    }
                }
                if !self.status.is_empty() {
                    ui.text(&self.status);
                }
            });
        self.window_open[WINDOW_TEXT] = opened;
    }

    /// Окно состояния: что именно встало — шрифт, язык, автодополнение, флажки.
    fn window_state(&mut self, ui: &Ui) {
        if !self.window_open[WINDOW_STATE] {
            return;
        }
        let mut opened = self.window_open[WINDOW_STATE];
        ui.window(Self::key(WINDOW_STATE).label(WINDOW_NAMES[WINDOW_STATE]))
            .opened(&mut opened)
            .size([560.0, 200.0], Condition::FirstUseEver)
            .build(|| {
                ui.text(format!(
                    "{ROWS} кадров × {} колонок — с вводом {} строк",
                    COLUMNS.len(),
                    self.rows_with_input
                ));
                if let Some(error) = &self.editor_error {
                    ui.text(format!("ошибка редактора: {error}"));
                }
                ui.text(format!(
                    "шрифт: {}; язык: Python (приближение для .tas)",
                    if self.font_id.is_some() {
                        "DejaVu Mono (CTE)"
                    } else {
                        "по умолчанию"
                    }
                ));
                ui.text(format!(
                    "автодополнение: {}; флажки: {}",
                    self.autocomplete.as_deref().unwrap_or("выключено"),
                    match self.flag_style {
                        FlagStyle::Square => "квадрат в ячейке",
                        FlagStyle::Cell => "заливка всей ячейки",
                    }
                ));
                ui.text(format!(
                    "правок: {} в таблице, {} в тексте",
                    self.cell_edits, self.changed_frames
                ));
            });
        self.window_open[WINDOW_STATE] = opened;
    }
}

/// Индексы окон в [`Spike::window_open`]: их порядок и есть порядок отрисовки.
const WINDOW_WORKSPACE: usize = 0;
const WINDOW_TABLE: usize = 1;
const WINDOW_TEXT: usize = 2;
const WINDOW_STATE: usize = 3;

/// Раскладка панелей деревом: сплиттеры ImGui между соседями.
///
/// Слева колонка — «Состояние» над «Скриптами»; справа «Кадры» над «Текст скрипта».
/// Число у `split` — доля, которую занимает первая сторона. Дальше размеры держит сам
/// докспейс: потянув разделитель, пользователь меняет две соседние панели разом, и это
/// сохраняется в `.ini` ImGui.
///
/// Ключи окон — те же [`Spike::key`], что и у самих окон: докспейс адресует окна по
/// стабильному идентификатору, а подпись может меняться независимо.
fn spike_dock_layout() -> DockLayout {
    let left = DockLayout::split(
        DockSplit::Up,
        0.14,
        DockLayout::tabs([Spike::key(WINDOW_STATE)]),
        DockLayout::tabs([Spike::key(WINDOW_WORKSPACE)]),
    );
    let right = DockLayout::split(
        DockSplit::Up,
        0.58,
        DockLayout::tabs([Spike::key(WINDOW_TABLE)]),
        DockLayout::tabs([Spike::key(WINDOW_TEXT)]),
    );
    DockLayout::split(DockSplit::Left, 0.24, left, right)
}

/// Подписи окон для меню «Вид» — по тем же индексам.
const WINDOW_NAMES: [&str; 4] = ["Скрипты", "Кадры", "Текст скрипта", "Состояние"];

/// Заголовок главного окна — оно же хост докспейса, в который докаются панели.
/// Имя только ASCII: не-ASCII в именах окон путает поиск и докинг.
const WINDOW_TITLE: &str = "TAS Editor Spike";

/// Строковый идентификатор докспейса (хэшируется в `Id`). Он же ключ дерева в `.ini`.
const DOCKSPACE_ID: &str = "SpikeDockspace";

/// Таблица кадров: заголовок, фиксированные колонки, горизонтальный и вертикальный скролл.
///
/// С `clipped` рисуется только видимый диапазон (`ListClipper`); без него —
/// все `ROWS` строк. Разница и есть цена виртуализации. Возвращает число
/// переключённых за кадр квадратиков.
fn frames_table(ui: &Ui, clipped: bool, rows: &mut [Row], style: FlagStyle) -> u32 {
    let mut toggles = 0;

    let mut builder = ui
        .table("frames")
        .flags(
            TableFlags::BORDERS
                | TableFlags::ROW_BG
                | TableFlags::SCROLL_Y
                | TableFlags::SCROLL_X
                | TableFlags::RESIZABLE,
        )
        .sizing_policy(TableSizingPolicy::FixedFit)
        // `-1.0` по высоте — «до низа доступного места»: таблица занимает всю высоту окна,
        // а не обрезается на константе. Ширина `0.0` — по ширине окна.
        .outer_size([0.0, -1.0]);

    // Колонки узкие: значений в них мало (число или квадрат), а колонок тридцать —
    // широкие колонки заставляют таблицу скроллиться по горизонтали без нужды.
    for column in COLUMNS {
        builder = builder
            .column(column)
            .width(if column == "#" { 44.0 } else { 26.0 })
            .done();
    }

    builder.headers(true).build(|ui| {
        if clipped {
            for index in ListClipper::new(rows.len())
                .items_height(ROW_HEIGHT)
                .begin(ui)
                .iter()
            {
                toggles += table_row(ui, index, &mut rows[index], style);
            }
        } else {
            for (index, row) in rows.iter_mut().enumerate() {
                toggles += table_row(ui, index, row, style);
            }
        }
    });

    toggles
}

/// Одна строка таблицы: колонка `#`, затем колонка на каждый токен.
///
/// Стик рисуется текстом, кнопка — закрашенным квадратом (галочки нет). Идентификатор
/// квадрата несёт номер кадра: иначе ячейки разных строк делили бы одно состояние в ImGui.
fn table_row(ui: &Ui, frame: usize, cells: &mut Row, style: FlagStyle) -> u32 {
    let mut toggles = 0;
    ui.table_next_row();

    for (index, cell) in cells.iter_mut().enumerate() {
        ui.table_next_column();
        match cell {
            Cell::Empty => {}
            Cell::Value(text) => ui.text(&*text),
            Cell::Flag(pressed) => {
                let id = format!("##{frame}-{}", COLUMNS[index]);
                let rgba = if *pressed { FLAG_ON } else { FLAG_OFF };

                if style == FlagStyle::Cell {
                    // Заливка всей ячейки: цвет и есть «сетка».
                    ui.table_set_cell_bg_color(rgba, TableColumnRef::Current);
                }

                // Позицию берём до кнопки: кнопка сдвигает курсор.
                let top_left = ui.cursor_screen_pos();
                if ui.invisible_button(id, [FLAG_SIZE, FLAG_SIZE]) {
                    *pressed = !*pressed;
                    toggles += 1;
                }

                if style == FlagStyle::Square {
                    ui.get_window_draw_list()
                        .add_rect(
                            top_left,
                            [top_left[0] + FLAG_SIZE, top_left[1] + FLAG_SIZE],
                            rgba,
                        )
                        .filled(true)
                        .build();
                }
            }
        }
    }

    toggles
}

/// Проекция `SEGMENT` в кадры: токен с длительностью `N` занимает свою колонку
/// на `N` кадров — так же, как текст `.tas` читает таблица редактора.
fn build_rows() -> (Vec<Row>, usize) {
    let mut rows: Vec<Row> = vec![std::array::from_fn(|_| Cell::Empty); ROWS];
    for (frame, row) in rows.iter_mut().enumerate() {
        row[0] = Cell::Value(frame.to_string());
    }

    let mut cycles = 0;
    while cycles * CYCLE < ROWS {
        let base = cycles * CYCLE;
        for (offset, token, value, duration) in SEGMENT {
            let Some(column) = column_index(token) else {
                continue;
            };
            let start = base + offset;
            let cell = if value.is_empty() {
                Cell::Flag(true)
            } else {
                Cell::Value((*value).to_string())
            };
            for frame in start..(start + duration).min(ROWS) {
                rows[frame][column] = cell.clone();
            }
        }
        cycles += 1;
    }

    let with_input = rows
        .iter()
        .filter(|row| row[1..].iter().any(|cell| !matches!(cell, Cell::Empty)))
        .count();
    (rows, with_input)
}

/// Текст `.tas` из того же `SEGMENT`: таблица и редактор показывают один скрипт.
///
/// Формат — `docs/SCRIPT_DSL.md` §2–4: строка правил первой, затем кадр и токены;
/// длительность 1 не пишется, у стика третье поле — длительность.
fn script_text() -> String {
    let mut text = String::from(
        "! name=spike-segment trig=ticks:0\n\
         # Сегмент повторяется до конца скрипта\n\
         # Токен с длительностью N занимает свою колонку N кадров\n",
    );

    let mut cycles = 0;
    while cycles * CYCLE < ROWS {
        let base = cycles * CYCLE;
        text.push_str(&format!(
            "# сегмент {cycles}: кадры {base}—{}\n",
            base + CYCLE - 1
        ));
        for (offset, token, value, duration) in SEGMENT {
            let frame = base + offset;
            if frame >= ROWS {
                break;
            }
            let command = match (*value, *duration) {
                ("", 1) => (*token).to_string(),
                ("", n) => format!("{token}:{n}"),
                (value, 1) => format!("{token}:{value}"),
                (value, n) => format!("{token}:{value}:{n}"),
            };
            text.push_str(&format!("{frame} {command}\n"));
        }
        cycles += 1;
    }

    text
}

/// Индекс колонки для токена DSL.
fn column_index(token: &str) -> Option<usize> {
    COLUMNS.iter().position(|column| *column == token)
}

fn main() -> Result<(), RunError> {
    let clipped = std::env::var("SPIKE_NO_CLIPPER").is_err();
    let limit_secs = std::env::var("SPIKE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok());

    // Потолок частоты держим сами — сном по бюджету кадра, а не vsync. Замерено
    // 2026-09-28: с `AutoVsync` при перемещении окна система уводит поток в модальный
    // цикл (тот же, что при перетаскивании мышью), композитор снимает развёртку и
    // `Present(1)` ждёт событие флипа, которого нет — кадры проседают до 13–20 fps
    // (интервалы 50–79 ms) при цене UI 0.20 ms. Сон живёт в нашем потоке и от
    // композитора не зависит.
    //
    // По умолчанию 200: это не «частота экрана», а потолок против холостого хода.
    // Без потолка цикл крутился на ~1540 fps и грел поток впустую; 200 оставляет
    // запас и на 144-герцовый экран, и на редактор. `SPIKE_FPS=0` снимает потолок
    // совсем — только тогда виден настоящий потолок стека.
    let fps_cap = match std::env::var("SPIKE_FPS").ok().as_deref() {
        Some("0") => None,
        Some(value) => value.parse::<f64>().ok().filter(|fps| *fps > 0.0),
        None => Some(200.0),
    };

    let (rows, rows_with_input) = build_rows();

    // Как рисуются флажки — переключается переменной, чтобы померить и сравнить оба вида.
    let flag_style = match std::env::var("SPIKE_FLAGS").ok().as_deref() {
        Some("cell") => FlagStyle::Cell,
        _ => FlagStyle::Square,
    };

    println!(
        "[spike] данные: {ROWS} строк × {} колонок, непустых строк {rows_with_input}",
        COLUMNS.len()
    );
    println!(
        "[spike] флажки: {}",
        match flag_style {
            FlagStyle::Square => "квадрат в ячейке",
            FlagStyle::Cell => "заливка всей ячейки",
        }
    );
    println!(
        "[spike] потолок частоты: {}",
        match fps_cap {
            Some(fps) => format!("{fps:.0} fps (сон по бюджету кадра)"),
            None => "снят (SPIKE_FPS=0) — виден настоящий потолок стека".to_owned(),
        }
    );

    let config = AppConfig {
        window_title: "TAS Editor Spike".to_owned(),
        window_size: (1440.0, 900.0),
        // Докинг включён, но хост рисуем **мы** — внутри главного окна. Свой хост `dear-app`
        // здесь был бы вторым докспейсом и ломал бы ввод (проверено).
        docking: DockingConfig::application_managed(),
        theme: Some(Theme::Dark),
        // Без vsync: частоту держит наш сон по бюджету кадра (`fps_cap`). С `AutoVsync`
        // перемещение окна уводит поток в модальный цикл, развёртка пропадает и
        // `Present(1)` ждёт её событие — кадры проседают до 13–20 fps.
        present_mode: dear_app::wgpu::PresentMode::AutoNoVsync,
        ..AppConfig::default()
    };

    let now = Instant::now();
    let mut spike = Spike {
        editor: None,
        editor_error: None,
        font_id: None,
        autocomplete: None,
        changed_frames: 0,
        cell_edits: 0,
        status: String::new(),
        // Папку можно задать заранее: так список проверяется без кликов по диалогу.
        folder: std::env::var_os("SPIKE_FOLDER").map(PathBuf::from),
        files: Vec::new(),
        selected: None,
        pane_message: String::new(),
        pending_open: false,
        dialog_ms: None,
        rows,
        rows_with_input,
        flag_style,
        window_open: [true; 4],
        dock_layout_applied: false,
        clipped,
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

    // Список собираем до запуска цикла и печатаем — это и есть проверка отображения
    // без кликов по диалогу.
    spike.rescan();
    println!("[spike] {}", spike.pane_message);
    for file in &spike.files {
        println!(
            "[spike]   {} — {} строк, {} Б",
            file.name, file.lines, file.size
        );
    }

    dear_app::run(config, spike)
}
