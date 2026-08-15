# Воспроизведение записанного прохождения (Record / Replay)

Дизайн-документ системы записи и воспроизведения игрового ввода для Metal Gear Rising: Revengeance.

> Все адреса и смещения ниже взяты из read-only референса `ref/mgr-plugin-sdk` (файлы `game/Hw.h`, `game/Pl0000.h`, `shared/Events.h`). Смещения, помеченные как «вычислено из SDK», требуют рантайм-верификации (см. Этап 0). Адреса вида `base + N` отсчитываются от модуля игры (`GetModuleHandleA(null)`), как и в остальном коде drmod-rs.

---

## 1. Цель и принятые решения

**Цель:** иметь возможность записать прохождение (ввод игрока) и затем детерминированно воспроизвести его в игре.

**Зафиксированные решения:**

| Вопрос | Решение |
|--------|---------|
| Устройство ввода | **Клавиатура + мышь** (проще геймпада). Геймпад — опциональное расширение (см. §3.4). |
| Механизм воспроизведения | **Сырой ввод без хуков** — прямая запись в `KeyInput` / `MouseInput`, игра сама нормализует через `updateInput()`. |
| Точность | **Чистый ввод** — проигрываем ввод как есть; дрейф траектории из-за физики (Havok) допускается. |
| Объём записи | **По сегменту/миссии** — интеграция с существующим `active_segment`, как уже сделано для ghost. |
| Хранилище | **SQLite** (`runs.db`), новые таблицы `replays` + `replay_frames`, bulk insert по паттерну `segment::finish_segment`. |

---

## 2. Исследование системы ввода (ответ на вопрос 1 — «что двигало персонажем»)

Игра имеет трёхуровневую модель ввода:

```
[устройство] → [сырой ввод в памяти] → [нормализованный ввод игрока] → [действия]
 DirectInput      KeyInput / MouseInput     Pl0000::m_CurrentInput      handleActions()
 XInput           ControllerState           m_nButton*, m_fInputDirection
```

Каждый кадр `Pl0000::updateInput()` (vtable-функция 241) читает сырой ввод и записывает его в нормализованные поля игрока; затем `Pl0000::handleActions()` (vtable-функция 242) читает эти поля и выполняет движение/атаки/прыжки.

### 2.1. Сырой ввод (источник — то, что реально нажато)

Все структуры находятся в namespace `cInput` (`ref/mgr-plugin-sdk/game/Hw.h`).

**Клавиатура** — `cInput::ms_KeyInput = base + 0x177B7C0` (тип `KeyInput`):

| Поле | Смещение | Тип | Назначение |
|------|----------|-----|------------|
| `m_aKeysDown[6]` | `+0x00` | `u32[6]` | зажатые клавиши (битовая маска) |
| `m_aKeysPressed[6]` | `+0x18` | `u32[6]` | однократное нажатие в этом кадре |
| `m_aKeysReleased[6]` | `+0x30` | `u32[6]` | отпущенные в этом кадре |
| `m_aKeysAlternated[6]` | `+0x48` | `u32[6]` | «перещёлкнутые» (alternate) |
| `m_aKeyHistory[6]` | `+0x60` | `u32[6]` | история нажатий |
| `m_nPressDelay` | `+0x78` | `i32` | задержка повтора |

Методы-помощники (для справки, при прямой записи не обязательны):
`isKeyDown(int vKey)` @ `0x9D93A0`, `isKeyPressed` @ `0x9D9400`, `isKeyReleased` @ `0x9D9460`,
`setKeyDown(int vKey, BOOL)` @ `0x9D9620`, `setKeyPressed(int vKey)` @ `0x9D9650`.

**Мышь** — `cInput::ms_MouseInput = base + 0x177B798` (тип `MouseInput`):

| Поле | Смещение | Тип |
|------|----------|-----|
| `m_nMouseButtons` | `+0x00` | `i32` |
| `m_nButtonsPressed` | `+0x04` | `i32` |
| `m_nButtonsReleased` | `+0x08` | `i32` |
| `m_nButtonsAlternated` | `+0x0C` | `i32` |
| `m_MousePosition` | `+0x10` | `cVec2` (2×`f32`) |
| `m_nRepeatCount` | `+0x1C` | `i32` |
| `m_LastMousePosition` | `+0x20` | `cVec2` |

**Геймпад** — `cInput::ms_aControllers = base + 0x19D05F0` (массив `ControllerState[4]`, XInput):

| Поле | Смещение | Тип |
|------|----------|-----|
| `m_XInputState.dwPacketNumber` | `+0x00` | `u32` |
| `m_XInputState.Gamepad.wButtons` | `+0x04` | `u16` (битовая маска `eInputButton`) |
| `m_XInputState.Gamepad.bLeftTrigger` | `+0x06` | `u8` |
| `m_XInputState.Gamepad.bRightTrigger` | `+0x07` | `u8` |
| `m_XInputState.Gamepad.sThumbLX` | `+0x08` | `i16` |
| `m_XInputState.Gamepad.sThumbLY` | `+0x0A` | `i16` |
| `m_XInputState.Gamepad.sThumbRX` | `+0x0C` | `i16` |
| `m_XInputState.Gamepad.sThumbRY` | `+0x0E` | `i16` |
| `m_bAvailable` | `+0x10` | `i32` |

`eInputButton` (битовая маска геймпада): `DPAD_LEFT=1, DPAD_RIGHT=2, DPAD_DOWN=4, DPAD_UP=8, BUTTON_A=0x10, BUTTON_B=0x20, BUTTON_X=0x40, BUTTON_Y=0x80, BUTTON_START=0x100, BUTTON_BACK=0x200, LEFT_SHOULDER=0x400, LEFT_TRIGGER=0x800, LEFT_STICK=0x1000, RIGHT_SHOULDER=0x2000, RIGHT_TRIGGER=0x4000, RIGHT_STICK=0x8000`.

**Флаги автообновления** (важно для воспроизведения, см. §3.2):

| Флаг | Адрес | Тип |
|------|-------|-----|
| `ms_bUpdateKeyboard` | `base + 0x14CDDE8` | `bool` |
| `ms_bUpdateMouse` | `base + 0x19D07F8` | `bool` |
| `ms_bKeyboardAvailable` | `base + 0x19D06EC` | `bool` |
| `ms_bMouseAvailable` | `base + 0x19D0800` | `bool` |

Прочие глобальные инстансы `cInput` (справочно): `ms_InputDevice` (`base+0x19D06E4`, `LPDIRECTINPUT8`), `ms_PCInputDevice` (`base+0x19D06E8`), `ms_MouseDevice` (`base+0x19D06F4`), `ms_aControllerDevices` (`base+0x19D05A8`, `[4]`), `ms_InputKeys` (`base+0x19D06F8`, `char[256]` — сырое DirectInput-состояние клавиш), `ms_MouseStateInput` (`base+0x19D06D0`, `char[20]`), `ms_GlobalInput` (`base+0x19C1404`), `ms_aKeyMap` (`base+0x14CD838`, маппинг keybind→VK).

### 2.2. Нормализованный ввод игрока (`Pl0000`)

Объект игрока уже читается в drmod-rs: `static_ptr = *(base + 0x177B4A4)`. Внутри `Pl0000` (`ref/mgr-plugin-sdk/game/Pl0000.h`) находятся нормализованные поля ввода — **это и есть «что двигало персонажем» в игровой семантике**:

| Поле | Смещение* | Тип | Назначение |
|------|-----------|-----|------------|
| `m_CurrentInput` | `~0xCF8` | `InputUnit` (0x30) | кнопки Down/Pressed/Released/Alternated + стики + триггеры |
| `m_fInputMagnitudeSquared` | `~0xD28` | `f32` | квадрат магнитуды ввода |
| `m_fInputDirection` | `~0xD2C` | `f32` | направление ввода (уже спроецировано на камеру) |
| `m_fDesiredHeading` | `~0xD30` | `f32` | желаемый курс |
| `m_fAngleToCamera` | `~0xD34` | `f32` | угол к камере |
| `m_nButtonSwitchLockOn` | `~0xE08` | `i32` | lock-on |
| `m_nButtonJump` | `~0xE18` | `i32` | прыжок |
| `m_nButtonLightAttack` | `~0xE20` | `i32` | лёгкая атака |
| `m_nButtonHeavyAttack` | `~0xE24` | `i32` | тяжёлая атака |
| `m_nButtonAction` | `~0xE38` | `i32` | действие |
| `m_nButtonUseSubweapon` | `~0xE3C` | `i32` | под-оружие |
| `m_nButtonFireSubweapon` | `~0xE40` | `i32` | выстрел под-оружия |
| `m_nButtonNinjarun` | `~0xE48` | `i32` | ниндзя-бег |
| `m_nButtonBlademode` | `~0xE50` | `i32` | блейд-мод |
| `m_nButtonUseItem` | `~0xE58` | `i32` | предмет |

\* Смещения `m_*` вычислены из SDK-структуры `Pl0000` по якорям `m_bSwordHidden = 0xB74` и `m_SwordState = 0x13FC` (оба уже подтверждены в QWEN.md). Требуют рантайм-верификации (Этап 0).

Структура `InputUnit` (namespace `cInput`, `Hw.h`):

| Поле | Смещение | Тип |
|------|----------|-----|
| `m_nButtonsDown` | `+0x00` | `u32` |
| `m_nButtonsPressed` | `+0x04` | `u32` |
| `m_nButtonsReleased` | `+0x08` | `u32` |
| `m_nButtonsAlternated` | `+0x0C` | `u32` |
| `m_fLeftStick` | `+0x10` | `cVec2` |
| `m_fRightStick` | `+0x18` | `cVec2` |
| `m_fLeftTrigger` | `+0x20` | `f32` |
| `m_fRightTrigger` | `+0x24` | `f32` |
| `m_bValidInput` | `+0x28` | `i32` |
| `m_nRepeatCount` | `+0x2C` | `i32` |

### 2.3. Механизм обновления и события

- `Pl0000::updateInput()` — vtable-функция **241** игрока; читает сырой ввод → пишет `m_CurrentInput` / `m_nButton*` / `m_fInputDirection`.
- `Pl0000::handleActions()` — vtable-функция **242**; читает нормализованные поля → применяет действия.
- `Pl0000::setDefaultInput()` — `base + 0x779E20`.
- `cInput::updateInputUnit(InputUnit*, int userIndex)` — `base + 0x9DAFE0`.
- `cInput::updateControllerStateInput(ControllerState*, int)` — `base + 0x9DA900`.
- `cInput::setInputUnitButtons(InputUnit*, u32)` — `base + 0x9DA210`.
- `cInput::isKeybindDown(eSaveKeybind)` — `base + 0x61D280` (маппинг keybind→VK).

События SDK (`shared/Events.h`, для возможного hooking в будущем, в MVP не используются):

| Событие | Адрес инъекции | Семантика |
|---------|----------------|-----------|
| `OnUpdateEvent` | `0x6526A2` | каждый non-game тик (до геймплея) |
| `OnTickEvent` | `0x64D411` | каждый in-game тик |
| `OnPresent` | `0xB9807A` | рендер (аналог текущего hudhook-хука) |
| `OnEndScene` | `0x652651` | рендер |

### 2.4. Итог по вопросу 1

Для записи «что двигало персонажем» есть два корректных источника:

1. **Сырой ввод** (`ms_KeyInput` + `ms_MouseInput` + `ms_aControllers`) — что реально нажато. Полный, не зависит от игровой логики, симметрично воспроизводится. **Выбран для MVP.**
2. **Нормализованный ввод** (`Pl0000::m_CurrentInput` + `m_nButton*` + `m_fInputDirection`) — что игра поняла. Удобен для отладки, но для воспроизведения требует хука `updateInput`.

Позиция/скорость (`base+0x50/54/58`, `BehaviorAppBase::m_vecVelocity`, `rAnim`) — это **результат** движения, а не ввод; подходит для ghost-трека, но не воспроизводит атаки/QTE/блейд-мод.

---

## 3. Способы подачи ввода (ответ на вопрос 2 — «как скормить ввод игре»)

### 3.1. Прямая запись в сырой ввод (рекомендовано)

Пишем те же байты, что были записаны, обратно в `ms_KeyInput` / `ms_MouseInput`. Игра сама нормализует их через `updateInput()` → `m_CurrentInput` → движение.

**Плюсы:** не требует фокуса окна, работает в фоне, детерминировано, не зависит от раскладки/устройства, нет системной очереди сообщений.

**Нюанс (гипотеза, проверить в Этапе 1):** игра каждый кадр обновляет `ms_KeyInput`/`ms_MouseInput` из DirectInput-устройства. Чтобы наша запись не затиралась, перед воспроизведением выставляем флаги:

```
*(bool*)(base + 0x14CDDE8) = false;  // ms_bUpdateKeyboard
*(bool*)(base + 0x19D07F8) = false;  // ms_bUpdateMouse
```

Если флаг работает как ожидается, игра перестаёт перечитывать реальное устройство и принимает наши значения. После остановки воспроизведения возвращаем `true`.

### 3.2. Прямая запись в нормализованный ввод (альтернатива)

Писать готовый `InputUnit` / `m_nButton*` в `Pl0000`. Требует перехвата `updateInput()` (vtable 241), потому что иначе игра перезапишет наши значения в том же кадре. Сложнее (нужен hooking: `VirtualProtect` + detour), но детерминированнее. **Не используется в MVP** (пользователь выбрал «без хуков»).

### 3.3. WinAPI `PostMessage` / `SendInput`

Посылать `WM_KEYDOWN`/`WM_KEYUP` в окно игры. Работает для клавиатуры/мыши, но:
- требует фокуса окна (не работает в фоне);
- идёт через системную очередь — возможны задержки и «съедание» событий другими приложениями;
- не эмулирует геймпад;
- менее детерминировано.

Для DLL-инжекта прямая запись в память (§3.1) надёжнее. `SendInput` можно использовать как запасной вариант, если прямая запись не сработает.

### 3.4. Эмуляция геймпада

**Ключевой вывод:** эмуляция геймпада ≠ эмуляция XInput-устройства.

- Настоящий XInput-геймпад эмулировать из DLL нельзя: `XInputGetState()` читает из системного драйвера, а не из памяти процесса. Для «виртуального устройства» нужен драйвер (`ViGEmBus`, `vJoy`).
- **Для воспроизведения это не нужно.** Геймпад эмулируется на уровне игрового состояния, а не устройства:
  - писать в `ms_aControllers[i].m_XInputState` (кэш XInput в памяти игры), предварительно выставив `m_bAvailable = 1` и отключив обновление контроллера (аналог `ms_bUpdateKeyboard`);
  - либо писать в нормализованный `InputUnit` (`m_fLeftStick`, `m_fRightStick`, `m_fLeftTrigger`, `m_fRightTrigger`, `m_nButtonsDown`) — этот путь вообще не зависит от XInput.

Итог: для MVP берём клавиатуру+мышь; геймпад добавляется позже через те же структуры без драйвера.

---

## 4. Архитектура решения

### 4.1. Новый модуль `src/replay.rs`

```rust
pub enum ReplayMode {
    Idle,
    Recording,
    Playback,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ReplayFrame {
    pub duration_ms: i64,
    pub keys_down: [u32; 6],
    pub keys_pressed: [u32; 6],
    pub mouse_buttons: i32,
    pub mouse_pressed: i32,
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_last_x: f32,
    pub mouse_last_y: f32,
}
```

Размер кадра ≈ **80 байт**. При 60 FPS одна минута ≈ 288 КБ — приемлемо для SQLite.

Константы адресов:

```rust
const KEY_INPUT: usize = 0x177B7C0;      // cInput::ms_KeyInput
const MOUSE_INPUT: usize = 0x177B798;    // cInput::ms_MouseInput
const UPDATE_KEYBOARD: usize = 0x14CDDE8; // ms_bUpdateKeyboard
const UPDATE_MOUSE: usize = 0x19D07F8;    // ms_bUpdateMouse
```

Методы (в `HelloHud`, по паттерну существующего `read_game_state()`):

```rust
fn read_keys(&self) -> ([u32; 6], [u32; 6]);   // (down, pressed)
fn write_keys(&self, down: [u32; 6], pressed: [u32; 6]);
fn read_mouse(&self) -> MouseState;
fn write_mouse(&self, m: &MouseState);
fn set_input_autoupdate(&self, enabled: bool); // ms_bUpdateKeyboard/ms_bUpdateMouse
```

### 4.2. Схема SQLite (дополняет `runs.db`)

```sql
CREATE TABLE replays (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id INTEGER NOT NULL,
    mission_name TEXT NOT NULL,
    started_at TEXT NOT NULL,
    frame_count INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL
);

CREATE TABLE replay_frames (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    replay_id INTEGER NOT NULL,
    frame_index INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    keys_down BLOB NOT NULL,     -- 24 байта [u32; 6]
    keys_pressed BLOB NOT NULL,  -- 24 байта [u32; 6]
    mouse_buttons INTEGER NOT NULL,
    mouse_pressed INTEGER NOT NULL,
    mouse_x REAL NOT NULL,
    mouse_y REAL NOT NULL,
    mouse_last_x REAL NOT NULL,
    mouse_last_y REAL NOT NULL,
    FOREIGN KEY (replay_id) REFERENCES replays(id) ON DELETE CASCADE
);

CREATE INDEX idx_replay_frames_replay ON replay_frames(replay_id, frame_index);
```

Создание таблиц — в `segment::create_segment_tables()` (или отдельной `replay::create_replay_tables()`). Bulk insert по паттерну `segment::finish_segment()`.

### 4.3. Интеграция в `HelloHud` (`src/lib.rs`)

- Поле `replay_mode: ReplayMode`, буфер `replay_buffer: Vec<ReplayFrame>`, загруженные кадры `replay_frames: Vec<ReplayFrame>`, `replay_start: Instant`.
- **Запись:** при `active_segment.is_some()` в `render()` (параллельно `position_buffer`) каждый кадр читать сырой ввод и пушить `ReplayFrame` с `duration_ms` от `seg.start_instant`.
- **Сохранение:** при `SegmentAction::End` — вставить запись в `replays`, затем bulk insert кадров в `replay_frames` (по аналогии с `finish_segment`).
- **Воспроизведение:** при старте сегмента (или по кнопке) загрузить кадры последней записи по `mission_id`, выставить `ReplayMode::Playback`, вызвать `set_input_autoupdate(false)`.
- **Применение кадра:** каждый кадр по `duration_ms` через `partition_point` (как ghost) — найти кадр, чей `duration_ms <= elapsed`, и записать его в `ms_KeyInput`/`ms_MouseInput`.
- **Остановка:** по достижении конца записи (или по кнопке) — `set_input_autoupdate(true)`, `ReplayMode::Idle`.

---

## 5. Этапы реализации

### Этап 0 — PoC чтения ввода

1. Добавить в `HelloHud` поля-адреса `ms_KeyInput` / `ms_MouseInput` и методы `read_keys()` / `read_mouse()`.
2. В debug-панели (только `#[cfg(debug_assertions)]`) вывести:
   - `m_aKeysDown[0..6]` и `m_aKeysPressed[0..6]` как hex-битмаски;
   - `m_nMouseButtons`, `m_MousePosition`.
3. **Проверка:** нажимать реальные клавиши/мышь — биты должны совпадать с VK-кодами. Сверить, что `m_aKeysDown`/`m_aKeysPressed` ведут себя как ожидается (Down — удержание, Pressed — фронт).
4. Опционально: вывести `m_CurrentInput` (InputUnit) игрока по смещению `~0xCF8`, чтобы верифицировать смещения §2.2 (при нажатии — биты в `m_nButtonsDown`/`m_nButtonsPressed`).

### Этап 1 — PoC записи ввода в память

1. Проверить **гипотезу флага**: выставить `ms_bUpdateKeyboard = false` (`base+0x14CDDE8`), затем записать бит клавиши «вперёд» (W) в `m_aKeysDown[0]` — проверить, что персонаж движется.
2. Если гипотеза неверна (игра всё равно перечитывает устройство) — исследовать порядок обновления:
   - писать после обновления игры (через `OnUpdateEvent` AddBefore, если допустим hooking);
   - либо писать в `ms_InputKeys` (сырое DirectInput-состояние, `char[256]`);
   - либо перейти к нормализованному вводу + хук `updateInput` (§3.2).
3. Debug-кнопки для ручного теста: «зажать W», «прыжок» (через `m_aKeysPressed`), «повернуть камеру» (через `m_MousePosition`).
4. **Проверка:** персонаж бежит/прыгает/камера крутится без реального ввода; после `ms_bUpdateKeyboard = true` реальный ввод возвращается.

### Этап 2 — Модуль записи (Record)

1. Создать `src/replay.rs`: `ReplayMode`, `ReplayFrame`, `MouseState`, функции чтения/записи.
2. Подключить `mod replay;` в `src/lib.rs`; добавить поля в `HelloHud` (`replay_mode`, `replay_buffer`).
3. Каждый кадр при `active_segment.is_some()` пушить `ReplayFrame` (duration_ms + сырой ввод).
4. Добавить таблицы `replays`/`replay_frames` (WAL уже включён в `init_db`).
5. При `SegmentAction::End` — bulk insert кадров в `replay_frames` + строка в `replays`.
6. **Проверка:** завершить сегмент → в БД появились `replays` и `replay_frames` с корректным числом кадров.

### Этап 3 — Модуль воспроизведения (Playback)

1. Загрузка кадров последней записи по `mission_id` (`ORDER BY id DESC LIMIT 1`, затем `SELECT ... WHERE replay_id = ? ORDER BY frame_index`).
2. Применение кадра по `duration_ms` через `partition_point` (паттерн ghost в `lib.rs::render_3d`).
3. На время воспроизведения `set_input_autoupdate(false)`; в конце — `true`.
4. **Проверка:** воспроизвести сегмент — персонаж повторяет движения (визуально, рядом с ghost-цилиндром).

### Этап 4 — UI

1. Переключатель режима (Record/Replay/Idle) и индикатор статуса в окне Settings (или отдельном окне), на русском.
2. Горячие клавиши для старта/остановки записи и воспроизведения (debug-сборка).
3. **Проверка:** переключение режимов меняет поведение, статус отображается.

### Этап 5 — Тестирование

1. Записать короткий сегмент → воспроизвести → сравнить траекторию с ghost (визуально).
2. Проверить однократные действия: прыжок, лёгкая/тяжёлая атака (фронт `Pressed`).
3. Проверить камеру (мышь) при воспроизведении.
4. Проверить границы: пустая запись, остановка посередине, отсутствие записи для миссии.
5. **Проверка:** `cargo build --release` (i686) + `cargo clippy` без предупреждений.

### Этап 6 — Документация

1. Обновить `QWEN.md`: новая секция «Record/Replay» с адресами ввода (`ms_KeyInput`, `ms_MouseInput`, `ms_aControllers`, `ms_bUpdateKeyboard/ms_bUpdateMouse`), новый модуль `src/replay.rs`, новые таблицы БД `replays`/`replay_frames`.

---

## 6. Риски и открытые вопросы

| Риск | Описание | Митигация |
|------|----------|-----------|
| Перезапись `ms_KeyInput` из DirectInput | Игра может каждый кадр перезаписывать сырой ввод из устройства, затирая наши кадры. | Гипотеза `ms_bUpdateKeyboard=false` (Этап 1); fallback — `ms_InputKeys` или нормализованный ввод + хук. |
| Недетерминизм физики | Чистый ввод в Havok может дрейфовать из-за вариации `dt`/RNG. | Принято (чистый ввод). Возможное расширение — гибрид «ввод + коррекция позиции». |
| Тайминг однократных `Pressed` | Прыжок/атака зависят от фронта нажатия; пропуск кадра может потерять событие. | Записывать и воспроизводить `Pressed` и `Down` отдельно; проверить в Этапе 5. |
| Точность смещений `m_*` в `Pl0000` | Смещения нормализованного ввода вычислены, не подтверждены. | Верифицировать в Этапе 0; для MVP они не критичны (используем сырой ввод). |
| Объём данных | Длинные сегменты → много кадров. | bulk insert + WAL (уже есть); при необходимости — сжатие или отдельный файл. |

---

## 7. План тестирования (сквозной)

1. **Запись:** запустить миссию → активировать запись → пробежать участок → завершить сегмент.
2. **Проверка данных:** в БД `replays`/`replay_frames` присутствует запись с ожидаемым числом кадров.
3. **Воспроизведение:** активировать playback → персонаж повторяет маршрут без реального ввода.
4. **Сравнение:** наложить ghost (уже реализован) и визуально сверить траектории записи и воспроизведения.
5. **Действия:** убедиться, что прыжки/атаки/блейд-мод воспроизводятся (не только перемещение).
6. **Камера:** убедиться, что повороты камеры (мышь) воспроизводятся.
