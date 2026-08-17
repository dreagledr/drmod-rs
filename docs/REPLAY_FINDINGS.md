# Проверенные гипотезы и грабли (Record/Replay)

> Живой документ: обновляется по мере рантайм-верификации. Все адреса — `base + offset`, base игры в тестах = `0x00370000` (релокация, см. грабли №1).

## Рабочий механизм подачи ввода (✅ проверено)

Игрок читает ввод из **глобального `InputUnit[0]` (`base + 0x177B850`)**, а не из `Pl0000::m_CurrentInput` и не из сырых кэшей (`ms_KeyInput`/`ms_InputKeys`/`ms_MouseStateInput`).

Цепочка:
```
DirectInput → cInput::updateInputUnit (0x9DAFE0, заполняет 4 глобальных unit)
  → Pl0000::updateInput (копирует g_InputUnit0 → m_CurrentInput 0xCF8 → m_nButton*)
  → handleActions (движение/атаки)
```

**Подача:** хук `updateInputUnit` (MinHook) → вызвать оригинал → перезаписать `g_InputUnit0` (только `user_index == 0`) нашими значениями. Прямая запись в `Present` не работает (поздно — после `handleActions`); прямая запись в поля `Pl0000` (включая `m_CurrentInput`) не работает по той же причине.

**Биты `InputUnit.buttons_down`/`buttons_pressed` (эмпирически, сверено с сырыми клавишами/мышью):**

| Действие | Клавиша | Бит |
|----------|---------|-----|
| Вперёд | W | `0x400000` (бит 22) + `left_stick = [0, -1000]` |
| Прыжок | Space | `0x1` (бит 0) |
| Лёгкая атака | ЛКМ (`mouse=1`) | `0x40` (бит 6) |
| Тяжёлая атака | ПКМ (`mouse=2`) | `0x80` (бит 7) |
| Камера | мышь | `right_stick` (дельта мыши, до ~±2000+) |

Замечания:
- `left_stick` хранит масштаб 1000 (W → `(0, -1000)`), а не `[-1, 1]` — это не «мусор», а нормальный формат.
- `m_nButtonJump` (0xE18) меняется при прыжке (`=1`), но это **следствие** (состояние), а не источник: прыжок подаётся битом `0x1` в `buttons`.
- `m_nButtonLightAttack=64`, `HeavyAttack=128`, `Action=32`, `Ninjarun=16384` — **константы** (не флаги): атаки идут через `buttons`, а не через эти поля.

## Полезные оффсеты

### Глобальные (cInput)

| Адрес | Что | Тип |
|-------|-----|-----|
| `base + 0x177B850` | `g_InputUnit0` — реальный источник входа игрока | `InputUnit` (0x30) |
| `base + 0x177B880` / `0x177B8B0` / `0x177B8E0` | `g_InputUnit1..3` (шаг 0x30, меню/UI) | `InputUnit` |
| `base + 0x9DAFE0` | `cInput::updateInputUnit(InputUnit*, int userIndex)` — заполняет unit из DirectInput, точка хука | cdecl |
| `base + 0x61D280` | `cInput::isKeybindDown(eSaveKeybind)` — удержание keybind (blade mode) | cdecl |
| `base + 0x61D2D0` | `cInput::isKeybindPressed(eSaveKeybind)` — фронт keybind (ripper) | cdecl |
| `base + 0x785190` | `Pl0000::enableRipperMode()` — включение ripper (обход ввода, без условий) | thiscall |
| `base + 0x7D9590` | `Pl0000::disableRipperMode(bool)` — выключение ripper | thiscall |
| `base + 0xD4AFC0` | `resetInputUnit` — сбрасывает unit (вызывается из 0x98D900) | |
| `base + 0x61D900` (= 0x98D900) | глобальный апдейт ввода: reset всех 4 unit + `0xD4A270` | |
| `base + 0x177B7C0` | `ms_KeyInput` (сырой кэш клавиш — вспомогательный, игроком не читается) | `KeyInput` |
| `base + 0x177B798` | `ms_MouseInput` (сырой кэш мыши — вспомогательный) | `MouseInput` |

### Pl0000 (объект игрока, `*(base + 0x177B4A4)`)

| Смещение | Поле | Тип |
|----------|------|-----|
| `+0xB74` | `m_bSwordHidden` (якорь раскладки) | i32 |
| `+0xCF8` | `m_CurrentInput` — копия `g_InputUnit0` (смещение **верное**) | `InputUnit` |
| `+0xD28` | `m_fInputMagnitudeSquared` | f32 |
| `+0xD2C` | `m_fInputDirection` (спроецировано на камеру) | f32 |
| `+0xD30` | `m_fDesiredHeading` | f32 |
| `+0xD34` | `m_fAngleToCamera` | f32 |
| `+0xE08` | `m_nButtonSwitchLockOn` | i32 |
| `+0xE18` | `m_nButtonJump` (=1 при прыжке) | i32 |
| `+0xE20` / `+0xE24` | `m_nButtonLightAttack` / `m_nButtonHeavyAttack` (константы 64/128) | i32 |
| `+0xE38` | `m_nButtonAction` (константа 32) | i32 |
| `+0xE48` | `m_nButtonNinjarun` (константа 16384) | i32 |
| `+0xE50` / `+0xE58` | `m_nButtonBlademode` / `m_nButtonUseItem` | i32 |
| `+0x13FC` | `m_SwordState` (якорь раскладки) | i32 |

Размер `Pl0000` = `0x5400` (SDK `VALIDATE_SIZE`).

### Скорость и поворот (подтверждено рантаймом 2026-08-16)

| Смещение | Поле | Назначение |
|----------|------|------------|
| `+0x50/54/58` | `cParts::m_vecTransPos` | позиция (x/y/z) |
| `+0x90/94/98` | `cParts::m_vecRotation` | поворот (Euler); меняется только Y = yaw = heading |
| `+0x890/894/898` | `BehaviorAppBase::m_vecVelocity` | **только вертикальная скорость** (vel.y при прыжке: вверх→пик→вниз); x/z всегда 0 — горизонтального поля скорости нет, движение по земле кинематическое (пишется прямо в позицию) |
| `+0x8E0` | `field_8E0` | всегда 0 |
| `+0x8F0` | `field_8F0` | копия поворота (== `m_vecRotation`) |
| `+0x900` | `field_900` | предыдущая позиция (лаг ~1 кадр); дельта `pos − field_900` даёт скорость за кадр |
| `+0x3184` | `m_bRipperModeEnabled` | ripper: 1 вкл / 0 выкл |
| `+0x40C8` | `m_nBladeModeType` | blade mode: 2 вкл / 0 выкл |

### `InputUnit` (0x30 байт)

| Смещение | Поле |
|----------|------|
| `+0x00` | `buttons_down` (u32) |
| `+0x04` | `buttons_pressed` (u32) |
| `+0x08` | `buttons_released` (u32) |
| `+0x0C` | `buttons_alternated` (u32) |
| `+0x10` | `left_stick` (cVec2, f32×2) |
| `+0x18` | `right_stick` (cVec2, f32×2) |
| `+0x20` | `left_trigger` (f32) |
| `+0x24` | `right_trigger` (f32) |
| `+0x28` | `valid_input` (i32) |
| `+0x2C` | `repeat_count` (i32) |

## Ripper / Blade Mode (✅ решено, 2026-08-16, дизассемблирование)

Ripper и blade mode активируются **не через `InputUnit`**, а через keybind-проверки в `handleActions`, которые читают сырой ввод **напрямую из DirectInput** (`GetDeviceState`), а не из кэшей `ms_KeyInput`/`ms_InputKeys` и не из системной очереди (`GetAsyncKeyState`).

**Две функции-близнеца** (обе `__cdecl`, `(eSaveKeybind) -> BOOL`, читают `ms_aKeyMap` @ `base+0x14CD838` → `ms_KeyInput` @ `base+0x177B7C0`):

| Функция | Адрес | Читает | Назначение |
|---------|-------|--------|------------|
| `isKeybindDown` | `base + 0x61D280` | `isKeyDown` @ 0x9D93A0 (удержание) | hold-действия: **blade mode** (`KEYBIND_BLADEMODE`=8) |
| `isKeybindPressed` | `base + 0x61D2D0` | `isKeyPressed` @ 0x9D9400 (фронт) | toggle-действия: **ripper** (`KEYBIND_RIPPERMODE`=11) |

**Условие активации ripper (дизассемблер, call site `enableRipperMode` 0x785190 → RVA 0x8106BD):**
```asm
mov  edx, [eax+0x344]   ; vtable[209] = canActivateRipperMode()
call edx
test eax, eax
jz   skip               ; !canActivateRipperMode() → skip
push 0x0B               ; KEYBIND_RIPPERMODE = 11
call 0x61D2D0           ; isKeybindPressed(11)
test eax, eax
jz   skip               ; фронт R не нажат → skip
call 0x785190           ; enableRipperMode()
```
То есть `canActivateRipperMode() && isKeybindPressed(11)`. Плюс второй безусловный путь (`[base+0x1B5A094] & 0x800`) — скриптовая активация.

**Что НЕ работает** (проверено рантаймом):
- запись в `ms_KeyInput`/`ms_InputKeys` (NumPad7 v1) — кэши перезаписываются DirectInput;
- `SendInput` (системная очередь) — `isKeybindDown/Pressed` не читают `GetAsyncKeyState`.

**Что работает:**
- прямой вызов `enableRipperMode()` @ `0x785190` / `disableRipperMode(false)` @ `0x7D9590` (`__thiscall`) — но **без условий и анимаций** (мгновенно, без fade) — только fallback;
- ✅ **хук `isKeybindPressed` (0x61D2D0)** для ripper + **хук `isKeybindDown` (0x61D280)** для blade — детур возвращает `1` для нужного keybind → `handleActions` запускает штатную цепочку (условия + анимации).

**Реализация** (`src/replay.rs`, debug): `is_keybind_pressed_detour` (RIPPERMODE, счётчик `RIPPER_FRAMES`) + `is_keybind_down_detour` (BLADEMODE, флаг `BLADE_HOLD`). NumPad7 = фронт ripper, NumPad8 = toggle blade-удержания.

## Опровергнутые гипотезы (не тратить время повторно)

| # | Гипотеза | Результат |
|---|----------|-----------|
| 1 | Запись в `ms_KeyInput.m_aKeysDown` при `ms_bUpdateKeyboard=false` двигает персонажа | ❌ Игра глохнет к клавиатуре, персонаж не реагирует — игрок не читает этот кэш |
| 2 | Запись в `ms_InputKeys` (DIK-сканкоды, 0x80=нажата) при флаге false | ❌ Кэш замерзает, наши значения видны, но персонаж не реагирует |
| 3 | `ms_MouseStateInput` (DIMOUSESTATE2) — путь мыши игрока | ❌ dx/dy всегда 0, кэш не используется игроком |
| 4 | `m_CurrentInput` по 0xCF8 — «мусор, смещение неверно» | ❌ **ошибка интерпретации**: 0xCF8 верно; `L=(0,-1000)` при W — нормальный left_stick (масштаб 1000). «valid чередуется» — из-за чтения в Present в неправильной фазе |
| 5 | Хук `updateInputUnit` + перезапись стика двигает игрока | ❌ (в раннем тесте) — нужны **правильные значения** (биты `buttons_down` + стики), а не только стик; писать на `user_index == 0` |
| 6 | Хук `isKeybindDown` (0x61D2D0) — точка движения | ❌ 0x61D2D0 — это **`isKeybindPressed`** (фронт), а не `isKeybindDown`. Вызывается с `k=11` (RIPPERMODE); FORWARD (k=0) не запрашивается — это не путь движения |
| 7 | 0xDCE1B0 — keybind-проверка движения | ❌ Принимает указатель; вызывается 3 раза за сессию — не путь движения |
| 8 | vtable 241 Pl0000 (0xB804B0) — это `updateInput` | ❌ Это switch по rAnim (`mov eax,[ecx+0x618]; cmp eax,0x141; jmp [table]`, 321 кейс); SDK vtable-индексы неточны |

## Грабли (технические ловушки)

1. **Релокация базы.** Игра грузится по `base=0x00370000` (не 0x400000). В коде адреса глобалов — absolute (base+offset), но встречается и чистый offset (`[reg+offset]`). Сканеры паттернов ищут **оба варианта** (`target` и `target+base`).
2. **`VirtualQuery` + MEM_RESERVE.** Для зарезервированных регионов `Protect = 0` — это НЕ `PAGE_NOACCESS` (0x01). Фильтр читаемости: `State & MEM_COMMIT (0x1000) != 0`, иначе чтение даёт ACCESS_VIOLATION.
3. **`PAGE_GUARD` (0x100)** — комбинированный флаг; чтение страницы с guard → `STATUS_GUARD_PAGE_VIOLATION`. Исключать: `prot & 0x100 == 0`.
4. **Execute-протекты — НЕ битовые комбинации**: `PAGE_EXECUTE=0x10`, `EXECUTE_READ=0x20`, `EXECUTE_READWRITE=0x40`, `EXECUTE_WRITECOPY=0x80`. Фильтр «код»: `prot & 0xF0 != 0` (а НЕ `prot & 0x10` — тот ловит только PAGE_EXECUTE и даёт пустой скан).
5. **Размер модуля** — из PE-заголовка: `e_lfanew = *(u32*)(base+0x3C)`, сигнатура `0x00004550`, `SizeOfImage = *(u32*)(nt+0x50)`. Не «64 МБ вслепую».
6. **Чтение функции по фиксированной длине** может выйти за конец региона → AV. Использовать clamp до конца committed-региона.
7. **`call rel32` (E8)** в x86: абсолютный адрес цели НЕ появляется в байтах инструкции (только относительное смещение). Паттерн-скан по imm32 вызовы НЕ найдёт — нужен `find_calls_to` (E8 + вычисление target).
8. **Общий счётчик логов в детурах.** Если функция вызывается 240/сек, а редкая — 1-2 раза/сек, лог «каждые N вызовов» почти не срабатывает для редкой. Нужен отдельный счётчик на детур + лог при смене аргумента.
9. **Захуканная функция в дампе начинается с `E9` (JMP на детур)** — это MinHook-патч, не оригинальный код.
10. **thiscall на Rust (i686)** — `extern "C"` не кладёт `this` в ECX; хук thiscall-методов без naked-asm опасен. Предпочитать хуки cdecl-целей.
11. **`[u8; 256]` не реализует `Default`** (массивы > 32) — реализовывать вручную.
12. **Однократные фронты (pressed) в логе.** Периодический frame-лог (раз в N кадров) пропускает нажатия длиной 1 кадр. Логировать при **изменении** кнопок (`down<<32 | pressed`), а не по таймеру.
13. **Dangling-указатель объекта игрока при рестарте/loading (креш).** В loading `static_ptr` (`base+0x177B4A4`) может указывать на освобождённую память (не `null`), и разыменование `cached_player_obj_ptr` даёт ACCESS_VIOLATION. В логе выглядит как обрыв через ~0.9 c после попадания в спавн-триггер. **Фикс:** читать объект игрока только при `player_readable` (`menu_status_valid && !is_loading()`), в loading обнулять `cached_player_obj_ptr` (`null_mut()`). Тот же гейт уже был у `gStr`/`rAnim`.
14. **Воспроизведение по `dt` не работает.** Поиск кадра через `partition_point(duration_ms <= elapsed)` теряет однокадровые фронты `pressed`/`released` (квантование `as_millis()` + фазовый сдвиг «запись в детуре / подача в render») — прыжки/атаки пропадают. **Фикс:** `ReplayFrame{frame_index, input}` + подача строго по индексу (1 кадр на вызов render). Игра целится в 60 FPS, но на слабом железе реальный FPS ~52–58 (см. «Десинк Record→Playback» ниже); подача всё равно по индексу, т.к. фронты важнее dt.
15. **Кэшированные указатели `rAnim` / `PlayerManager` при рестарте (креш).** `r_anim_ptr` (цепочка `base+0x019C14C4 → +0x788 → +0x618`) и `player_manager_addr` (`*(base+0x17EA100)`) вычислялись ОДИН РАЗ в `HelloHud::new()` и больше не пересчитывались. При **быстром** рестарте `menu_status` остаётся `InGame` (не проходит через loading), но объект игрока и `PlayerManagerImplement` пересоздаются → кэшированные указатели указывают на освобождённую память → ACCESS_VIOLATION. В логе: `access=0x35351A98` = старый `player(0x35351480) + 0x618` (rAnim). **Фикс:** `rAnim` читать из `cached_player_obj_ptr + 0x618` (rAnim лежит в `Pl0000` — подтверждено disasm `mov eax,[ecx+0x618]`, FINDINGS №8); `PlayerManagerImplement` deref каждый кадр; плюс `VirtualQuery`-гейт `is_readable_ptr` на адрес игрока (освобождённая страница → `Protect=0` → пропуск чтения).
16. **`log_line` в hot-path детуре при рестарте → рекурсия access violation (креш).** Детур `updateInputUnit` вызывается игрой несколько раз за кадр и содержал `log_line` (chrono `Local::now()` + файловый I/O). При рестарте (статусы `InGame(1) → PauseMenu(3) → None(16)` и теардаун миссии) `log_line` сам падает (`access=0x77F6D3C8`, детерминированный системный адрес), VEH-обработчик вызывает `log_line` снова → рекурсия диспетчера исключений. В логе — **~сотни одинаковых `EXCEPTION`** за ~15 мс. **Фикс:** (a) детур `update_input_unit_detour` переписан на **чистые атомики/запись** — без `log_line` и диагностических чтений `*unit`; (b) `log_line` получил re-entrancy guard (`LOG_REENTRY`) — после первого фолта деградирует в no-op; (c) VEH-обработчик получил re-entrancy guard (`IN_VEH`) — не логирует при повторном входе; (d) `is_readable_ptr` теперь исключает `PAGE_GUARD` (см. №3), `pm_ptr` (PlayerManager) гейтится через `is_readable_ptr`.

## Десинк Record→Playback и замечания по сохранению (2026-08-16)

**dt: физика — fixed, анимация — variable.** Физика интегрируется с фиксированным шагом (вертикальная скорость `vel.y` совпадает побайтово между record и playback), а анимация (и кинематическое горизонтальное движение) берёт реальную дельту времени. Поэтому дрейфуют позиция/`rAnim`, а физика — нет. Подтверждено рантаймом и пользователем.

**Источник десинка — нестабильный кадр на слабом железе:**
- Реальный FPS ~52–58 (не 60): ранний прогон record=51.7, playback=52.6–56.2; после оптимизации ~58.5 (record/playback 58.3–58.6). `duration_ms` в БД — wall-clock, отражает частоту рендера, а не тики симуляции.
- **Воспроизводимость проверена:** три playback одной записи (ids 9/10/11) расходятся между собой до ~21 м — недетерминизм в самой игре (дельта анимации), а не record-vs-playback. При ~58.5 FPS разброс playback↔playback падает до ~1.63 м (~13×), но record↔playback всё ещё до ~12 м с бифуркацией на критическом событии (`rAnim 12→151`, climb-подобное, i≈540–600).
- Дрейф record↔playback накапливается: >0.01 м на i=58, >0.5 м i=103, >2 м i=174, >5 м i=246, >10 м i=417. Курс `rot.y` расходится первым (i≈50–55), позиция следует; `input_direction` совпадает 0/899 (направление верное, дрейфует интеграция движения/курса).

**Опаздывание blade-мода (вторичная причина):**
- Ввод `InputUnit` почти идеален: 14/899 расхождений, все — бит `0x800` в `buttons_down` (маркер blade, появляется только при активном blade-моде).
- Ripper (toggle) и blade ON воспроизводятся точно (+1 кадр), но **blade OFF опаздывает на +2…+11 кадров** (record 518→playback 520; record 545→playback 556). Причина: эмуляция реконструирует hold из `blade_mode_type` (0x40C8), который не хранит **момент отпускания кнопки** (только «клинок вынут/убран»). Эмуляция держит кнопку всё окно `blade_mode_type==2`, и игра заново проигрывает анимацию убирания. **Фикс:** писать в запись сам факт удержания blade-кнопки (keybind 8), а не результирующий `blade_mode_type`.

**Замечание по сохранению:** `flush_replay()` пишет покадровый `duration_ms = frame_index * 1000/60` (предполагает 60 FPS — неверно при реальном FPS ~52–58). Воспроизведение идёт по `frame_index`, поэтому покадровый `duration_ms` — только информационный. BLOB `state`/`camera` сохраняются, но при воспроизведении подаётся только `input` (+ реконструкция ripper/blade из `state`); `camera` не используется.

## Логи отладки

- `%LOCALAPPDATA%\drmod\debug.log` — детуры, override, frame-дампы (cur_in/g_unit0/pos, плюс `mouse=`/`space=`/`w=` для сопоставления битов).
- `%LOCALAPPDATA%\drmod\input_scan.log` — результаты старых сканов (кнопка модульного скана убрана, методы оставлены dead_code).
