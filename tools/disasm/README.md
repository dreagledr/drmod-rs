# Дизассемблирование (инструменты)

PowerShell-скрипты для диагностики игровых механик через чтение кода игры. Принцип: **сначала дизассемблировать, потом хукать** — это оказалось в разы эффективнее эмпирических тестов (см. `docs/REPLAY_FINDINGS.md`, секция «Ripper / Blade Mode»).

## scan_calls.ps1

Найти call site функции по RVA. Сканирует exe на диске (ищет `call rel32` → target RVA) и, опционально, память запущенной игры (релоцированные абсолютные адреса).

```
powershell -NoProfile -ExecutionPolicy Bypass -File tools/disasm/scan_calls.ps1 -Rva 0x785190
powershell -NoProfile -ExecutionPolicy Bypass -File tools/disasm/scan_calls.ps1 -Rva 0x785190 -Mem
```

## dump.ps1

Hex dump диапазона кода (для ручного дизассемблирования вокруг call site).

```
powershell -NoProfile -ExecutionPolicy Bypass -File tools/disasm/dump.ps1 -Rva 0x810400 -Len 0x380
```

## Порядок работы

1. Определить целевую функцию (например, из SDK: `enableRipperMode` @ `base + 0x785190`).
2. `scan_calls.ps1 -Rva 0x785190` → список call site (RVA).
3. `dump.ps1 -Rva <call_site - 0x20> -Len 0x100` → hex байтов.
4. Дизассемблировать вручную: `cmp`/`test` + `jz`/`jnz` перед `call` = условие активации.

## Проверенные факты (2026-08-16)

- base игры = `0x00370000` (релокация), ImageBase exe = `0x00400000`.
- В exe на диске абсолютные адреса функций = `ImageBase + RVA` (`0x400000 + RVA`), в памяти = `base + RVA` (`0x370000 + RVA`); `call rel32` (E8) не релоцируется (относительный).
- `enableRipperMode` (0x785190) вызывается из RVA `0x8106BD`/`0x8106D0`; условие: `canActivateRipperMode() && isKeybindPressed(11)`.
- `disableRipperMode` (0x7D9590) — из `0x81053D` и др.
- `isKeybindDown` @ 0x61D280 (→ `isKeyDown` 0x9D93A0), `isKeybindPressed` @ 0x61D2D0 (→ `isKeyPressed` 0x9D9400).
- `vtable[209]` = `canActivateRipperMode` (`mov edx, [eax+0x344]`; 209*4 = 0x344).
