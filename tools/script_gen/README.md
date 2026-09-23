# script_gen — JSON-фикстуры скрипта для редактора

Генерирует набор JSON-скриптов (`POST /script/run`, формат `docs/API.md` §4) для
round-trip тестов редактора `tas-editor-cs`: редактор читает фикстуру, пишет рядом
своё золото (`<имя>.expected.json` — тот же JSON, `<имя>.expected.tas` — тот же
скрипт текстом, формат [`docs/SCRIPT_DSL.md`](../../docs/SCRIPT_DSL.md)), а здешний
тест-приёмка читает это золото обратно.

Ключевое: фикстуры строятся **теми же типами**, которыми мод десериализует
запрос, — `drmod_replay_types::script` (`replay-types/src/script.rs`, общий крейт).
Поэтому «примет ли мод этот JSON» решает определение типа, а не сверка текста:
поля команд, `trigger`, `restart` и `when_enemy` описаны в одном месте на мод и
на тул.

## Сборка и запуск

Тул — workspace-член корневого проекта, собирается под **x64** (свой
`.cargo/config.toml`, как у `dbdump` и `server/`):

```bash
cargo run -p drmod-script-gen                     # из корня репозитория
cd tools/script_gen && cargo run                  # или из своей директории

# куда писать
cargo run -p drmod-script-gen -- --out <каталог>
```

По умолчанию фикстуры пишутся в `tas-editor-cs/TasEditorCs.Tests/Fixtures` —
путь считается от `CARGO_MANIFEST_DIR`, поэтому работать можно из любого каталога.
Файлы редактора (`*.expected.json`, `*.expected.tas`) тул **не** трогает: их
пишет сам редактор.

## Что в наборе

| Файл | Что покрывает |
|------|---------------|
| `all_inputs.json` | Все входы, выражаемые в DSL: 26 булевых, `left_stick` (осевой, дробный и угол-диагональ `[1000,-1000]` — сила > 1), `camera`; перекрытия команд, подряд идущие одинаковые входы, разрывы, `duration = 1`. |
| `edge_inputs.json` | Края §4.2, которых нет в тексте: `raw_key`, `dik_key`, `when_enemy` (минимальный и со всеми полями), `ripper` + явный стик в угол. |
| `rules_restart.json` | `name`, `trigger.pos`, `restart` со всеми параметрами не по умолчанию. |
| `rules_ticks.json` | Тиковый триггер (`trigger.ticks`) — воспроизводимый старт. |
| `minimal.json` | Только `commands`: дефолты мода (`name = "script"`). |

Тесты:

```bash
cargo test            # из этой директории или `cargo test -p drmod-script-gen`
```

- `fixtures_cover_every_input_key` — список ключей `input` берётся **из типа**
  (сериализация заполненного `ScriptInput`), иначе переименование поля прошло бы
  незаметно; тест требует, чтобы все они встречались в фикстурах.
- `fixtures_deserialize_and_pass_the_mod_limits` — каждая фикстура разбирается
  общими типами и проходит кросс-полевые лимиты `parse_script` (`t + duration ≤
  MAX_SCRIPT_FRAMES` — верхняя страховка в 1 000 000 кадров, `duration ≥ 1`,
  непустой `input`, `name ≤ 64`; практический ограничитель приёма — размер тела
  запроса, `API.md` §6).
- `tests/expected_json_is_accepted.rs` — **приёмка C#**: каждый
  `*.expected.json` из каталога фикстур десериализуется типами мода и сверяется с
  фикстурой, из которой редактор его собрал. Пропуск файла — провал теста, а не
  «нет проверки»: комплект золотых файлов должен быть полным.

## Порядок при изменениях формата

1. Правите общие DTO (`replay-types/src/script.rs`) или сам мод (`src/api.rs`).
2. `cargo run -p drmod-script-gen` — перегенерировать фикстуры.
3. В редакторе: `set TAS_REGEN_GOLDENS=1 && dotnet test TasEditorCs.slnx` —
   перезаписать золотые файлы, затем глазами прочитать `.expected.tas`
   (это и есть текст DSL) и `.expected.json`.
4. `cd tools/script_gen && cargo test` — приёмка со стороны мода.
