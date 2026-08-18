# Воспроизведение записанного прохождения (Record / Replay)

Дизайн-документ системы записи и воспроизведения игрового ввода для Metal Gear Rising: Revengeance.

> **Проверенные гипотезы и грабли (рантайм-верификация):** [REPLAY_FINDINGS.md](REPLAY_FINDINGS.md) — подтверждённые/опровергнутые гипотезы об устройстве ввода и технические ловушки (релокация базы, VirtualQuery/MEM_COMMIT, execute-протекты, call-скан, счётчики логов и т.д.). Обновлять при каждом рантайм-тесте.

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

**Маппинг клавиш (эмпирический, проверен в Этапе 0).** `m_aKeysDown` хранит **не VK-коды**, а игровые коды клавиш. Для обычных клавиш (ASCII-диапазон: буквы, цифры, Space):

```
code = VK ^ 0x1F        // инверсия младших 5 бит
index = code >> 5       // 0..5 — элемент m_aKeysDown
bit   = 1 << (code & 31)
```

| Клавиша | VK | Игровой код | Бит в `m_aKeysDown` |
|---------|-----|-------------|----------------------|
| W | `0x57` | `0x48` | `keys[2] |= 0x00000100` |
| A | `0x41` | `0x5E` | `keys[2] |= 0x40000000` |
| S | `0x53` | `0x4C` | `keys[2] |= 0x00001000` |
| D | `0x44` | `0x5B` | `keys[2] |= 0x08000000` |
| Space | `0x20` | `0x3F` | `keys[1] |= 0x80000000` |
| 1 | `0x31` | `0x2E` | `keys[1] |= 0x00004000` |

Спец-клавиши (модификаторы, Esc, стрелки, F, Numpad) кодируются **отдельным enum** в диапазоне `0x80+` — XOR-формула для них не работает:

| Клавиша | Игровой код | Бит в `m_aKeysDown` |
|---------|-------------|----------------------|
| Enter | `0x15` | `keys[0] |= 0x00200000` |
| Tab | `0x16` | `keys[0] |= 0x00400000` |
| Shift | `0x80` | `keys[4] |= 0x00000001` |
| Alt | `0x82` | `keys[4] |= 0x00000004` |
| Ctrl | `0x84` | `keys[4] |= 0x00000010` |
| Esc | `0x8E` | `keys[4] |= 0x00004000` |
| ↑ (Up) | `0x90` | `keys[4] |= 0x00010000` |
| → (Right) | `0x91` | `keys[4] |= 0x00020000` |
| ← (Left) | `0x92` | `keys[4] |= 0x00040000` |
| ↓ (Down) | `0x93` | `keys[4] |= 0x00080000` |
| F1…F12 | `0x9F…0x94` (`0xA0 − n`) | `keys[4]` |
| Numpad Enter | `0xAA` | `keys[5] |= 0x00000400` |
| Numpad 0…9 | `0xB9…0xB0` (`0xB9 − n`) | `keys[5]` |

Проверено: WASD, 1, Space, Enter, Tab, Shift, Ctrl, Alt, Esc, все стрелки, F1–F3, Numpad 0–2, Numpad Enter. F4–F12 и Numpad 3–9 — по экстраполяции (`Fn = 0xA0 − n`, `Numpad n = 0xB9 − n`); неизвестные коды в debug-панели показываются как `0xXX`. Для Record/Replay маппинг не нужен (битмаски копируются как есть), для конструирования ввода — см. `replay::vk_to_key_code` / `replay::key_code_name` (`src/tas/replay.rs`).

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
- `cInput::isKeybindDown(eSaveKeybind)` — `base + 0x61D280` (удержание keybind; hold-действия: blade mode).
- `cInput::isKeybindPressed(eSaveKeybind)` — `base + 0x61D2D0` (фронт keybind; toggle-действия: ripper).

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

### 2.5. Ripper / Blade Mode (обнаружено дизассемблированием, 2026-08-16)

Ripper и blade mode **не идут через `InputUnit`** — они активируются в `handleActions` через keybind-проверки, читающие сырой ввод напрямую из DirectInput (`GetDeviceState`). Поэтому override `InputUnit` их не воспроизводит.

- **Ripper** (toggle, клавиша R / L3+R3) — условие: `canActivateRipperMode() && isKeybindPressed(KEYBIND_RIPPERMODE=11)` (call site `enableRipperMode` 0x785190 → RVA 0x8106BD).
- **Blade mode** (hold, удержание) — через `isKeybindDown(KEYBIND_BLADEMODE=8)`.

**Не работает:** запись в `ms_KeyInput`/`ms_InputKeys` (перезаписываются DirectInput), `SendInput` (не читается `GetAsyncKeyState`).

**Работает:** прямой вызов `enableRipperMode()` @ 0x785190 / `disableRipperMode(false)` @ 0x7D9590 (`__thiscall`) — но **без условий и анимаций**; и ✅ **хук `isKeybindPressed` (0x61D2D0)** + **хук `isKeybindDown` (0x61D280)** — детур возвращает `1` для нужного keybind, `handleActions` запускает штатную цепочку (условия + анимации). Подробности и дизассемблер — [REPLAY_FINDINGS.md](REPLAY_FINDINGS.md).

**Интеграция в Record/Playback (2026-08-16).** Запись уже сохраняет результат ripper/blade в `ReplayFrame.state` (`ripper_enabled` @ 0x3184, `blade_mode_type` @ 0x40C8 — см. §4.2). При воспроизведении эти хуки подаются в playback-цикле `lib.rs` (debug) через ту же keybind-эмуляцию (`RIPPER_FRAMES`/`BLADE_HOLD`), что и NumPad7/8:

- **Ripper (toggle):** детектируется перепад `ripper_enabled` между кадрами записи (`frame[K] != frame[K-1]`) → `set_ripper_frames(1)` — один фронт `isKeybindPressed(11)`, игра сама тоглит вкл/выкл по текущему состоянию.
- **Blade mode (hold):** `blade_mode_type != 0` → `set_blade_hold(bool)` на каждом кадре — удержание `isKeybindDown(8)`.

Эмуляция задаётся в `render(K)` и применяется детуром на тике `K+1`, то есть синхронно с override `InputUnit` (тот же сдвиг +1 кадр, что и движение). Сброс — `replay::clear_keybind_emulation()` при остановке воспроизведения, старте записи и входе в loading (чтобы hold blade не «зависал»). Фронт/удержание логируются в `debug.log` (`playback: ripper edge …`, `playback: blade hold …`).

---

## 3. Способы подачи ввода (ответ на вопрос 2 — «как скормить ввод игре»)

> ⚠️ Ниже — исторический анализ способов подачи. Рантайм-верификация показала: движение подаётся через **хук `updateInputUnit` + `g_InputUnit0`** (не §3.1/§3.2), а ripper/blade — через **хуки `isKeybindPressed`/`isKeybindDown`** (см. §2.5 и `REPLAY_FINDINGS.md`).

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

### 4.1. Новый модуль `src/tas/replay.rs`

```rust
pub enum ReplayMode {
    Idle,
    Recording,
    Playback,
}

/// Один кадр записи: полный InputUnit (m_CurrentInput) + полное состояние
/// персонажа и камеры + номер кадра.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ReplayFrame {
    pub frame_index: u32,
    pub input: InputUnit,       // m_CurrentInput (0xCF8)
    pub state: PlayerState,     // позиция/поворот/скорость/HP/состояния
    pub camera: CameraState,    // позиция камеры + view-proj матрица
}
```

Размер кадра ≈ **216 байт** (`u32` + `InputUnit` 0x30 + `PlayerState` 0x58 + `CameraState` 0x4C). При 60 FPS одна минута ≈ 760 КБ.

**Источник записи** — `m_CurrentInput` (`Pl0000 + 0xCF8`) + полное состояние из `cached_player_obj_ptr` и камеры, читается **в `render()`** (после `read_game_state()`): глобальный `g_InputUnit0` в `Present` сброшен (`valid=0`), а `m_CurrentInput` — персистентная копия, стабильно читается в render. Захват симметричен для записи и воспроизведения.

**Воспроизведение** — обратно через `InputOverride` (полный `InputUnit`), детур пишет в `g_InputUnit0`; результат (состояние) также захватывается в `playback_log`.

**Ключевое:** записываем/воспроизводим **сырые значения** `InputUnit` целиком — маппинг битов для Replay не нужен (семантика нужна только debug-кнопкам). Неизвестные биты (блейд-мод, нинзяран, action, lock-on, под-оружие) воспроизводятся автоматически, т.к. копируется вся битмаска.

Константы адресов:

```rust
const UPDATE_INPUT_UNIT: usize = 0x9DAFE0;    // cInput::updateInputUnit (точка хука)
const GLOBAL_INPUT_UNIT0: usize = 0x177B850;  // g_InputUnit0 (источник входа игрока)
```

Методы (уже реализованы в `src/tas/replay.rs` / `HelloHud`):

```rust
fn read_global_input_unit(&self) -> InputUnit;   // чтение g_InputUnit0
fn set_input_override(ov: InputOverride);        // подача ввода (пишет в g_InputUnit0)
```

### 4.2. Схема SQLite (дополняет `runs.db`)

Полное состояние пишется в **отдельные таблицы** для записи и воспроизведения, под общим `replay_runs.id`:

```sql
CREATE TABLE replay_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,                 -- 'record' | 'playback'
    mission_id INTEGER NOT NULL,
    mission_name TEXT NOT NULL,
    started_at TEXT NOT NULL,
    frame_count INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    source_replay_id INTEGER            -- для playback: id исходной записи
);

CREATE TABLE replay_record_frames (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    replay_id INTEGER NOT NULL,
    frame_index INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    input_unit BLOB NOT NULL,           -- InputUnit (0x30)
    state BLOB NOT NULL,                -- PlayerState (0x58)
    camera BLOB NOT NULL,               -- CameraState (0x4C)
    FOREIGN KEY (replay_id) REFERENCES replay_runs(id) ON DELETE CASCADE
);

CREATE TABLE replay_playback_frames (
    -- та же форма, что у replay_record_frames
);

CREATE INDEX idx_replay_record_frames ON replay_record_frames(replay_id, frame_index);
CREATE INDEX idx_replay_playback_frames ON replay_playback_frames(replay_id, frame_index);
```

**Смещения `PlayerState` (из SDK, требуют рантайм-верификации):**

| Поле | Смещение | Источник |
|------|----------|----------|
| позиция | `0x50/54/58` | `cParts::m_vecTransPos` |
| поворот (Euler) | `0x90/94/98` | `cParts::m_vecRotation` |
| скорость (вертикальная) | `0x890/894/898` | `BehaviorAppBase::m_vecVelocity` — только Y; x/z всегда 0 |
| HP / rAnim | `0x870` / `0x618` | подтверждено |
| `m_CurrentInput` | `0xCF8` | подтверждено |
| ripper | `0x3184` | `Pl0000::m_bRipperModeEnabled` |
| blade mode type | `0x40C8` | `Pl0000::m_nBladeModeType` |

`CameraState`: позиция камеры `+0x1B0`, view-proj матрица `+0x200` (углы извлекаются офлайн из матрицы).

Создание таблиц — `replay::create_replay_tables()`, вызывается из `init_db()`. Флаш — `replay::flush_replay()` (bulk insert по паттерну `segment::finish_segment()`): буфер копится в памяти, в БД пишется одним `BEGIN`/`COMMIT` по завершении записи/воспроизведения.

> **Замечания по сохранению (2026-08-16):**
> - Покадровый `duration_ms` в `replay_*_frames` пишется как `frame_index * 1000 / 60` — это **фиктивная** длительность в предположении ровно 60 FPS. Реальный FPS в тестах ~52–58 (зависит от железа), поэтому покадровый `duration_ms` неточен и **не используется** при воспроизведении (подача идёт строго по `frame_index`). Он оставлен только для совместимости/диагностики.
> - `duration_ms` в `replay_runs` — честный wall-clock (`Instant::elapsed()` от старта до стопа), но отражает **частоту рендера**, а не число тиков симуляции.
> - BLOB `state` и `camera` **сохраняются**, но при воспроизведении подаётся только `input` (`InputUnit`); из `state` читаются лишь `ripper_enabled`/`blade_mode_type` для реконструкции ripper/blade. `camera` пока не используется вовсе — сохранён для офлайн-анализа и будущих фиксов (snap-коррекция, ресинк по `rAnim`).

### 4.3. Интеграция в `HelloHud` (`src/lib.rs`)

- Поле `replay_mode: ReplayMode`, буфер `replay_buffer: Vec<ReplayFrame>`, загруженные кадры `replay_frames: Vec<ReplayFrame>`, `replay_start: Instant`.
- **Запись:** внутри детура `updateInputUnit` (после вызова оригинала, когда override **выключен**) читать реальный `InputUnit` и пушить `ReplayFrame` с `frame_index` (счётчик кадров от старта). Читать в `Present` нельзя — unit сброшен.
- **Сохранение:** при `SegmentAction::End` — вставить запись в `replays`, затем bulk insert кадров в `replay_frames` (по аналогии с `finish_segment`).
- **Воспроизведение:** при старте сегмента (или по кнопке) загрузить кадры последней записи по `mission_id`, выставить `ReplayMode::Playback`.
- **Применение кадра:** каждый кадр строго по `frame_index` (1 кадр на тик), а не по `duration_ms`/`partition_point` — dt-сопоставление теряет однокадровые фронты `pressed`/`released` (прыжок/атака). Флаги `ms_bUpdateKeyboard`/`ms_bUpdateMouse` **не нужны** — override пишет в правильной фазе кадра.
- **Остановка:** по достижении конца записи (или по кнопке) — снять override, `ReplayMode::Idle`.

---

## 5. Этапы реализации

### Этап 0 — PoC чтения ввода ✅ выполнено (2026-08-15)

1. Добавить в `HelloHud` поля-адреса `ms_KeyInput` / `ms_MouseInput` и методы `read_keys()` / `read_mouse()`. ✅
2. В debug-панели (только `#[cfg(debug_assertions)]`) вывести:
   - `m_aKeysDown[0..6]` и `m_aKeysPressed[0..6]` как hex-битмаски; ✅
   - `m_nMouseButtons`, `m_MousePosition`. ✅
3. **Проверка:** нажимать реальные клавиши/мышь — биты должны совпадать с VK-кодами. Сверить, что `m_aKeysDown`/`m_aKeysPressed` ведут себя как ожидается (Down — удержание, Pressed — фронт). ✅ **Результат:** биты НЕ совпадают с VK — игра хранит игровые коды клавиш. Маппинг найден эмпирически и задокументирован в §2.1 (`code = VK ^ 0x1F` для ASCII-клавиш + отдельный enum спец-клавиш). Down/Pressed/Released ведут себя корректно.
4. Опционально: вывести `m_CurrentInput` (InputUnit) игрока по смещению `~0xCF8`, чтобы верифицировать смещения §2.2 (при нажатии — биты в `m_nButtonsDown`/`m_nButtonsPressed`). ✅ выведено в debug-панели; смещение требует сверки бит при нажатии (открытый вопрос).

### Этап 1 — Подача ввода ✅ выполнено (2026-08-15)

**Рабочий механизм:** override глобального `InputUnit[0]` (`base + 0x177B850`) в хуке `cInput::updateInputUnit` (0x9DAFE0, MinHook).

Цепочка ввода:
```
DirectInput → updateInputUnit (заполняет 4 глобальных InputUnit) → Pl0000::updateInput
  (копирует g_InputUnit0 → m_CurrentInput 0xCF8 → m_nButton*) → handleActions (движение/атаки)
```

Прямая запись в `Present` не работает (поздно — после `handleActions`); прямая запись в сырые кэши (`ms_KeyInput`/`ms_InputKeys`/`ms_MouseStateInput`) не работает (игрок их не читает). Подробности — в [REPLAY_FINDINGS.md](REPLAY_FINDINGS.md).

**Реализация:** `InputOverride{active, buttons_down, buttons_pressed, left_stick, right_stick}` → `replay::set_input_override` → детур `update_input_unit_detour` вызывает оригинал, затем для `user_index == 0` перезаписывает `g_InputUnit0` нашими значениями. Debug-кнопки («Зажать W», «Прыжок», «Лёгкая атака», «Тяжёлая атака», «Крутить камеру») + скрипт NumPad4 (бег → прыжок → удар → поворот камеры).

**Биты действий в `InputUnit.buttons_down`/`buttons_pressed` (эмпирически, сверено с сырыми клавишами/мышью):**

| Действие | Клавиша | Бит |
|----------|---------|-----|
| Вперёд | W | `0x400000` (бит 22) + `left_stick = [0, -1000]` |
| Прыжок | Space | `0x1` (бит 0) ⚠️ |
| Лёгкая атака | ЛКМ (`mouse=1`) | `0x40` (бит 6) |
| Тяжёлая атака | ПКМ (`mouse=2`) | `0x80` (бит 7) |
| Камера | мышь | `right_stick` (дельта мыши, до ~±2000) |

Константы — `replay::input_bits` (`src/tas/replay.rs`).

> ⚠️ **2026-08-18:** бит `0x1` (прыжок) требует перепроверки — при подаче через API-скрипт
> вместо прыжка открывается меню выбора оружия (`SelectWeaponMenu`), персонаж замирает.
> Подробности — `docs/API.md` §10.1.

**Проверено:** персонаж бежит/прыгает/атакует/камера крутится без реального ввода; после снятия override реальный ввод возвращается.

### Этап 1.5 — Smoke-тест Record→Playback (проверка механизма InputUnit) ✅ выполнено (2026-08-15)

**Назначение:** проверить саму цепочку `InputUnit` — запись реального ввода в детуре (при override=off) → воспроизведение через `set_input_override` (override=on) — **без привязки к сегментам и без SQLite**. Короткий фрагмент ввода записывается в память и сразу проигрывается.

**Клавиши (debug-сборка):**

| Клавиша | Действие |
|---------|----------|
| NumPad5 | запись: arm → авто-старт по триггеру → стоп (повторное нажатие в arm — отмена) |
| NumPad6 | воспроизведение: arm → авто-старт по триггеру → стоп / авто-стоп в конце |

**Отложенный старт:** NumPad5/6 взводит arm; запись/воспроизведение стартуют автоматически, когда игрок попадает в триггерную зону спавна R-01 beach (`-24.7, 12.14, 120.7`, допуск ±0.1 XY / ±1.0 Y). Триггер — хардкод `BARE_START_TRIGGER` в `src/lib.rs` (зеркалит `segment::START_CONDITIONS[0x0118]`). Это убирает ручной тайминг/дрейф: запись и воспроизведение стартуют в одной точке пространства.

**Реализация:**
1. В `src/tas/replay.rs` — буфер короткой записи `BareRecording { active, frames }` (static `Mutex`, независим от БД): `start_bare_recording()` / `stop_bare_recording()` / `is_bare_recording()` / `bare_recording_frame_count()`, приватный `record_bare_frame()`. Кадры пишутся с порядковым номером `frame_index` (`st.frames.len() as u32`), а не с `duration_ms`.
2. `InputOverride` расширен до полного `InputUnit`: `{ active, input }`, детур делает `*unit = guard.input` (полная запись).
3. В детуре `update_input_unit_detour` запись читается **до** вызова оригинала (unit ещё содержит реальный ввод — оригинал сбрасывает его в ноль), override пишется **после** оригинала.
4. В `src/lib.rs` (`HelloHud`) — поля `bare_playback` / `bare_playback_frames` / `bare_playback_frame_idx` / `bare_record_armed` / `bare_playback_armed`; методы `toggle_bare_record` / `toggle_bare_playback` (arm/стоп/отмена) / `update_bare_deferred_start` / `stop_bare_playback`; в `render()` сначала `update_bare_deferred_start` (arm → триггер), затем применение кадра строго по индексу (`bare_playback_frame_idx` растёт на 1 за кадр, без dt). `update_input_injection` делает early-return при `bare_playback`.
5. В `src/ui.rs` (debug-панель) — подсказка клавиш и статус `short record` / `short playback`.

**Как проверить:** в игре (debug-сборка) NumPad5 (arm) → рестарт R-01 → игрок на спавне попадает в триггер → запись авто-стартует; подвигаться/прыгнуть/атаковать ~2–3 с → NumPad5 (стоп). NumPad6 (arm) → рестарт → авто-старт воспроизведения — персонаж повторяет ввод; NumPad6 — стоп. Запись ловит только реальный ввод (override выключен), буфер без авто-лимита.

### Этап 2 — Модуль записи (Record)

> **Частично выполнено (2026-08-16):** полное покадровое логирование состояния
> реализовано на механизме NumPad5/6 (не через сегменты). Ввод читается из
> `m_CurrentInput` (0xCF8) в `render()` вместе с полным состоянием (`PlayerState`)
> и камерой (`CameraState`); буфер в памяти флашится в `replay_runs` +
> `replay_record_frames`/`replay_playback_frames` по завершении. Пункты 4–5 ниже
> (сегментная интеграция `SegmentAction::End`) — не сделаны.

1. Добавить в `src/tas/replay.rs`: `ReplayMode`, `ReplayFrame { frame_index, input: InputUnit }` (dt не воспроизводит геймплей — см. Этап 1.5).
2. **Расширить `InputOverride` до полного `InputUnit`** (сейчас только `buttons_down`/`pressed`/`left_stick`/`right_stick`; добавить `buttons_released`, `buttons_alternated`, `left_trigger`, `right_trigger`, `valid_input`, `repeat_count`) — нужно для точной записи и воспроизведения.
3. **Точка записи:** в детуре `update_input_unit_detour` (после вызова оригинала, при **выключенном** override) читать реальный `InputUnit` и пушить `ReplayFrame` с `frame_index` (счётчик кадров от старта). В `Present` читать нельзя — unit сброшен (`valid=0`).
4. Добавить таблицы `replays`/`replay_frames` (`input_unit BLOB`, 48 байт). WAL уже включён в `init_db`.
5. При `SegmentAction::End` — bulk insert кадров в `replay_frames` + строка в `replays` (паттерн `segment::finish_segment`).
6. **Проверка:** завершить сегмент → в БД появились `replays` и `replay_frames` с корректным числом кадров; кадры содержат ненулевые `buttons_down`/стики при реальном вводе.

### Этап 3 — Модуль воспроизведения (Playback)

1. Загрузка кадров последней записи по `mission_id` (`ORDER BY id DESC LIMIT 1`, затем `SELECT ... WHERE replay_id = ? ORDER BY frame_index`).
2. Каждый кадр строго по `frame_index` (1 кадр на тик), а не по `duration_ms`/`partition_point` — dt теряет однокадровые фронты `pressed`/`released`. Записывать его **полный `InputUnit`** через `set_input_override`. Флаги `ms_bUpdateKeyboard`/`ms_bUpdateMouse` **не нужны** — override пишет в правильной фазе кадра.
3. Остановка по достижении конца записи — снять override, `ReplayMode::Idle`.
4. **Проверка:** воспроизвести сегмент — персонаж повторяет движения/атаки/камеру (визуально, рядом с ghost-цилиндром).

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

1. Обновить `QWEN.md`: новая секция «Record/Replay» с адресами ввода (`ms_KeyInput`, `ms_MouseInput`, `ms_aControllers`, `ms_bUpdateKeyboard/ms_bUpdateMouse`), новый модуль `src/tas/replay.rs`, новые таблицы БД `replays`/`replay_frames`.

---

## 6. Риски и открытые вопросы

| Риск | Описание | Митигация |
|------|----------|-----------|
| Перезапись `ms_KeyInput` из DirectInput | Игра может каждый кадр перезаписывать сырой ввод из устройства, затирая наши кадры. | Гипотеза `ms_bUpdateKeyboard=false` (Этап 1); fallback — `ms_InputKeys` или нормализованный ввод + хук. |
| Недетерминизм физики | Чистый ввод в Havok может дрейфовать из-за вариации `dt`/RNG. | ✅ Уточнено (2026-08-16): **физика использует фиксированный `dt`** (детерминирована — вертикальная скорость `vel.y` совпадает побайтово), а **анимация — реальную дельту времени**. Дрейф даёт именно анимация (и кинематическое горизонтальное движение), см. ниже. |
| Дрейф/бифуркация из-за дельты анимации | Анимация берёт дельту времени; на слабом железе (FPS < 60) дельта «гуляет» → позиция/`rAnim` дрейфуют и на критических событиях (climb, выход из blade) разветвляются. Подтверждено: 3 playback одной записи расходятся между собой. | Митигация: (b) подменять дельту времени анимации на 1/60; (c) snap-коррекция позиции/курса к записи; (d) ресинк по `rAnim`. |
| Опаздывание blade-мода | Blade mode не идёт через `InputUnit`; эмуляция реконструирует его из `blade_mode_type` (0x40C8), где не виден момент отпускания кнопки → выход из blade опаздывает на +2…+11 кадров (ripper и blade ON — точно, +1 кадр). | Записывать при записи сам факт удержания blade-кнопки (keybind 8), а не результирующий `blade_mode_type`. |
| Тайминг однократных `Pressed` | Прыжок/атака зависят от фронта нажатия; dt-сопоставление теряет 1-кадровые фронты. | ✅ Решено: подача строго по `frame_index` (1 кадр/тик), не по `dt` (см. FINDINGS №14). |
| Креш при рестарте (loading) | `static_ptr`/`cached_player_obj_ptr` указывает на освобождённую память (dangling, не `null`) → access violation. | ✅ Решено: читать объект игрока только при `player_readable`, в loading обнулять `cached_player_obj_ptr` (см. FINDINGS №13). |
| Креш при быстром рестарте (без loading) | `menu_status` не переходит в loading (остаётся `InGame`), но объект игрока/`rAnim`/`PlayerManager` пересоздаются; кэшированные в `new()` указатели `r_anim_ptr`/`player_manager_addr` становятся dangling → access violation. | ✅ Решено: `rAnim` читается из `cached_player_obj_ptr + 0x618` (лежит в `Pl0000`), `PlayerManagerImplement` перечитывается каждый кадр; `cached_player_obj_ptr` обнуляется по `VirtualQuery` (страница не committed/не читаема) — см. FINDINGS №15. |
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
