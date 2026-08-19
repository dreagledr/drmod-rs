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

**Реализация** (`src/tas/hooks.rs`): обобщённая keybind-эмуляция — `set_keybind_pressed(keybind, frames)` (фронт `isKeybindPressed`) + `set_keybind_hold(keybind, on)` (удержание `isKeybindDown`) для произвольных индексов. NumPad7 = фронт ripper, NumPad8 = toggle blade-удержания.

## Keybind-механизм всех действий (дизассемблирование, 2026-08-18)

Полный скан call sites `isKeybindDown` (0x61D280) / `isKeybindPressed` (0x61D2D0) в exe:

**`isKeybindPressed` — только 2 call sites, оба ripper:** `0x810599` (disable-путь) и `0x8106AF` (enable-путь, `canActivateRipperMode() && isKeybindPressed(11)`). **Все остальные действия активируются через `isKeybindDown`.**

**`isKeybindDown` — 14 call sites:**

| Call site | Keybind | Действие |
|-----------|---------|----------|
| `0x61DBE7` | `push 9` | NINJARUN (ниндзя-бег) |
| `0x61DCB6` | `push 1` | BACK (движение назад) |
| `0x61DCFB`/`0x61DD1D` | `push 3` | RIGHT (движение вправо; `isKeyDown(клавиша) || isKeybindDown(3)`) |
| `0x61DD2B` | `push 2` | LEFT (движение влево) |
| `0x61DE21` | цикл `esi=5..22` | JUMP..FIRE_SUBWEAPON: `if (isKeybindDown(k)) { m_button = getButton(k); or [esp+0x4C], m_button }` — сборка нормализованных кнопок |
| `0x69AD1D`, `0x79403C` | `push 0x15` | DEFFENSIVE_OFFENSIVE (dodge) |
| `0x69AD73`, `0x794092` | `push 0x14` | EXECUTION (zandatsu) |
| `0x120BEF`, `0x1A33F4`, `0x1AD09B`, `0x441E68` | `push 0x14` | EXECUTION (zandatsu, другие контексты) |

Функция `0x779F30` (вызывается из цикла 0x61DE21) — switch по keybind 5..22, возвращает `m_nButton*` поле Pl0000 (`0xE18`=Jump, `0xE20`=LightAttack, `0xE24`=HeavyAttack, `0xE50`=Blademode, `0xE48`=Ninjarun, `0xE38`=Action, `0xE08`=SwitchLockOn, ...) — это сборка нормализованных кнопок, а не InputUnit-битов.

**Биты движения (подтверждено логом `cur_in down`):** `0x400000`=W, `0x800000`=S, `0x200000`=A, `0x100000`=D; `0x4000`=ninja run (сопутствует FORWARD). В `updateInputUnit` (0x9DAAC0) биты стиков: left_stick → `0x10000`/`0x20000`/`0x40000`/`0x80000`, right_stick → `0x1000`/`0x2000`/`0x4000`/`0x8000`, триггеры → `0x800`/`0x4000`.

**Меню-клавиши (стрелки/Enter) — НЕ keybind'ы:** в `eSaveKeybind` их нет. Меню читает их через `KeyInput::isKeyDown`/`isKeyPressed` (0x9D93A0/0x9D9400, **thiscall**: `mov esi, ecx; ... and eax, [esi+edx*4]` — чтение кэша `ms_KeyInput`). Подача: запись бита в кэш + заморозка `ms_bUpdateKeyboard` (0x14CDDE8) в детуре `updateInputUnit` (`hooks::set_raw_key`). Требует рантайм-проверки (docs/API.md §10.3).

**Маппинг `isKeyDown`/`isKeyPressed` (дизассемблирование, 2026-08-18):** принимают **VK-код** (не игровой): `bit = 0x80000000 >> (vKey & 31)`, `index = vKey >> 5`, чтение `ms_KeyInput.m_aKeysDown[index]` / `m_aKeysPressed[index]` (+0x18). Эквивалентно маппингу REPLAY.md (`игровой код = VK ^ 0x1F`, `bit = 1 << (code & 31)`): `1 << (n ^ 0x1F) = 0x80000000 >> n` для 5-битных n.

**Верификация Этапа 2 (2026-08-18, прервано; уточнено 2026-08-19):**
- ✅ Keybind-эмуляция работает для действий с **отдельными call sites** `isKeybindDown`: dodge (21, r_anim 0→99), движение 1/2/3, ninja (9), execution (20).
- ✅ **`subweapon` (13) из цикла 5..22 РАБОТАЕТ** (2026-08-19): долгое удержание → режим прицеливания, короткий тап → мгновенное применение; игра ставит бит `0x400` в InputUnit (debug.log: `down=00000400 pressed=00000400` на 1-м кадре, далее `down=00000400`). Т.е. цикл 5..22 влияет на unit 0 для subweapon.
- ❌ Keybind-эмуляция **НЕ работает** для меню-действий из **цикла 5..22** (0x61DE21): `weapon_select` (16) не открыл меню, `pause` (18) — тоже. Цикл собирает константы/поля в `[esp+0x4C]`, но для меню-действий результат не влияет на активный unit 0 (вероятно, это сборка для геймпада/других unit'ов).
- ❌ Запись в кэш `ms_KeyInput` + заморозка `ms_bUpdateKeyboard=false` **НЕ работает**: `ms_bUpdateKeyboard=0` ставится (видно в памяти), но `ms_KeyInput` остаётся 0 при активной подаче — DirectInput перезаписывает кэш независимо от флага (или меню читает ввод напрямую из DirectInput).
- ⚠️ `ms_aKeyMap` (base+0x14CD838, «маппинг keybind→VK») — прочитаны странные значения (32, 57, 65, 30, 66, 48, 67, 46, 68, 32, 69, 18, 70, 33, 71, 34, 72, 35, 73, 23, 74, 36, 75) — не похожи на VK/игровые коды биндов; структура/смещение не подтверждены.
- **Следующий шаг:** хук `isKeyDown` (0x9D93A0) / `isKeyPressed` (0x9D9400) — детур `extern "C" fn(vKey: i32) -> i32` (this в ECX игнорируется): эмуляция (битмаски `RAW_KEYS_*`) + чтение `ms_KeyInput` напрямую (оригинальный trampoline thiscall не вызываем).

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
| 9 | Запись в кэш `ms_KeyInput` + `ms_bUpdateKeyboard=false` — подача меню-клавиш (esc/стрелки/Enter) | ❌ (2026-08-18) `ms_bUpdateKeyboard=0` ставится, но `ms_KeyInput` остаётся 0 — DirectInput перезаписывает кэш независимо от флага |
| 10 | Keybind-эмуляция `isKeybindDown(16)` открывает меню оружия (`weapon_select`) | ❌ (2026-08-18) Цикл 5..22 (0x61DE21) не влияет на unit 0 — меню не открылось. Работают только отдельные call sites (dodge 21 и т.д.). ⚠️ Уточнение (2026-08-19): для `subweapon` (13) keybind-удержание РАБОТАЕТ — игра ставит бит `0x400` в InputUnit (см. docs/INPUT_STATUS.md); вывод «цикл 5..22 не влияет на unit 0» верен для меню-действий (16/18), но не для subweapon |

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

## weapon_select: бит 0x01 = DPAD_LEFT (геймпад), не клавиша "2" (2026-08-19)

**Проблема:** бит `0x01` в `InputUnit.buttons_down` подаётся через override (как `ar_mode` 0x08 / `jump` 0x10), но:
- Открывает меню оружия **и** листает слоты (навигация)
- На 1 кадре — меню открывается, но выбор сдвигается
- На нескольких кадрах — меню крутится по слотам

**Дизассемблирование функции 0x8AC570** (маппинг битов InputUnit → клавиши меню):

```
test ebx, 0x400 → push 0x92 (KEY_LEFT)  → call isKeyDown (0x9D93A0)
test ebx, 0x2000 → push 0x93 (KEY_DOWN) → call isKeyDown
test bl, 0x01   → push 0x8D             → call isKeyDown  ← weapon_select
test bl, 0x02   → push 0x8E (KEY_ESC)   → call isKeyDown  ← pause
test bl, 0x04   → push 0x8C (KEY_ENTER) → call isKeyDown  ← confirm
test bl, 0x08   → push 0x8F             → call isKeyDown  ← codec?
```

**Вывод:** биты `0x01`/`0x02`/`0x04`/`0x08` — это **кнопки геймпада** (DPAD_LEFT/RIGHT/DOWN/UP из `eInputButton`), которые игра маппит на действия меню через `isKeyDown`. Бит 0x01 = DPAD_LEFT, который на геймпаде открывает weapon select.

**Почему битовый путь не работает для weapon_select:**
- Бит 0x01 в InputUnit = геймпадный D-Pad Left
- D-Pad Left используется **и для открытия меню, и для навигации** по слотам
- В отличие от `jump` (0x10) / `ar_mode` (0x08) — у них нет "навигационного" побочного эффекта
- Поэтому подача бита 0x01 одновременно открывает меню и листает слоты

**Call sites функции 0x8AC570** (16 мест):
- 0x594921, 0x59493D — близко к `cWeaponSelectMenu` (0x5926A0)
- 0x5BC25E, 0x5BC2A2, 0x5BCA47, 0x5BCA60, 0x5BCA7B, 0x5BCA96 — обработка меню
- 0x5C328E, 0x5C3954 — codec/pause меню
- 0x9150A3, 0x915114, 0x915175, 0x915187, 0x91519C, 0x9151B1 — другие меню

**Решение:** хук `isKeyDown` (0x9D93A0) / `isKeyPressed` (0x9D9400). Когда функция 0x8AC570 вызывает `isKeyDown(0x8D)` для бита 0x01, детур возвращает 1 для эмулируемых клавиш. Это единственный путь, который видит вся функция 0x8AC570.

**Игровые коды клавиш меню** (из `addresses.rs`):
- `KEY_ENTER = 0x8C` (confirm)
- `KEY_ESC = 0x8E` (pause)
- `KEY_UP = 0x90`, `KEY_RIGHT = 0x91`, `KEY_LEFT = 0x92`, `KEY_DOWN = 0x93`
- `KEY_DIGIT2 = 0x2D` (weapon_select на клавиатуре) — **не маппится** из битов InputUnit

**Альтернативный путь (RedTrainer):** прямая запись `GameMenuStatus = 9` по адресу `base + 0x17E9F9C` через `setMenuType(7)` + патчинг памяти. Но это открывает меню без выбора слота — навигация всё равно нужна.

## Логи отладки

- `%LOCALAPPDATA%\drmod\debug.log` — детуры, override, frame-дампы (cur_in/g_unit0/pos, плюс `mouse=`/`space=`/`w=` для сопоставления битов).
- `%LOCALAPPDATA%\drmod\input_scan.log` — результаты старых сканов (кнопка модульного скана убрана, методы оставлены dead_code).
