---
name: rusqlite-injected-dll
description: Add SQLite persistence to a hudhook-injected DLL overlay — database path, init pattern, graceful degradation, and UI display
source: auto-skill
extracted_at: '2026-07-13T17:00:00.000Z'
---

# SQLite Persistence in an Injected DLL (rusqlite + imgui-rs + hudhook)

Add persistent storage to a game overlay DLL using rusqlite with the `bundled` feature. The pattern covers database file location for an injected DLL, one-shot init on load, graceful error handling, and displaying stored data in the imgui UI.

## When to use

- The project is an imgui-rs + hudhook game overlay injected into a running process (see `QWEN.md` in drmod-rs)
- You need to persist data across DLL loads (run history, config, stats, save slots)
- The DLL has no control over the working directory — you must choose a reliable, writable path

## Procedure

### 1. Add dependencies to `Cargo.toml`

```toml
[dependencies]
rusqlite = { version = "0.40.1", features = ["bundled"] }
chrono = "0.4.45"
```

- `bundled` statically links SQLite — avoids depending on the system/host SQLite version, critical for an injected DLL
- `chrono` for human-readable timestamp formatting (optional; raw `SystemTime` Debug output works too but is less readable)

### 2. Choose the database path

**Rule: use `%LOCALAPPDATA%\<project>\<name>.db`.**

Injected DLLs run inside the game process, so the working directory is the game's install folder (possibly read-only or non-portable). Using `%LOCALAPPDATA%` is:
- Always writable
- Per-user (no permission conflicts)
- Standard Windows convention

```rust
let localappdata = std::env::var("LOCALAPPDATA").unwrap_or_default();
let db_dir = format!("{}\\{}", localappdata, "drmod"); // project subfolder
let db_path = format!("{}\\{}", db_dir, "runs.db");
```

Alternative: use `%APPDATA%` (roaming) if the data should follow the user across machines in a domain.

### 3. Create the init function (free function, not a method)

Extract database init into a standalone function called once from the struct constructor. This keeps the constructor clean and isolates all fallible I/O:

```rust
fn init_db() -> (String, Option<String>) {
    let now = Local::now();
    let current = now.format("%Y-%m-%d %H:%M:%S").to_string();

    let localappdata = match std::env::var("LOCALAPPDATA") {
        Ok(v) => v,
        Err(_) => return (current, None), // graceful: no env var
    };

    let db_dir = format!("{}\\drmod", localappdata);
    let db_path = format!("{}\\runs.db", db_dir);

    if std::fs::create_dir_all(&db_dir).is_err() {
        return (current, None); // graceful: can't create dir
    }

    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(_) => return (current, None), // graceful: can't open DB
    };

    if conn.execute("CREATE TABLE IF NOT EXISTS runs (...)", ()).is_err() {
        return (current, None); // graceful: can't create table
    }

    let prev = conn
        .query_row("SELECT started_at FROM runs ORDER BY id DESC LIMIT 1", [], |row| {
            row.get::<_, String>(0)
        })
        .ok(); // ok() → None if no rows or error

    let _ = conn.execute("INSERT INTO runs (started_at) VALUES (?1)", [&current]);

    (current, prev)
}
```

**Key pattern — return early with defaults on every fallible step.** The function never panics; every error returns `(current_time, None)`. This means:
- If `LOCALAPPDATA` is unset → overlay still works, just shows `N/A` for previous
- If disk is full → overlay still works
- If DB is corrupted → overlay still works (next run will create a fresh DB)

The `Connection` is NOT stored — it's opened, used, and dropped. This is fine for write-once-read-once patterns. If you need per-frame writes, store `Option<Connection>` as a struct field instead.

### 4. Wire into the struct constructor

```rust
struct HelloHud {
    start_time: Instant,
    current_run_start: String,
    prev_run_start: Option<String>,
    // ... existing fields
}

impl HelloHud {
    fn new() -> Self {
        let (current_run_start, prev_run_start) = init_db();
        // ... existing init code
        Self {
            start_time: Instant::now(),
            current_run_start,
            prev_run_start,
            // ... existing fields
        }
    }
}
```

### 5. Display in the imgui UI

```rust
ui.text(format!("Elapsed: {:?}", self.start_time.elapsed()));
ui.text(format!("Current run:  {}", self.current_run_start));
if let Some(ref prev) = self.prev_run_start {
    ui.text(format!("Previous run: {}", prev));
} else {
    ui.text_colored([0.5, 0.5, 0.5, 1.0], "Previous run: N/A");
}
```

Use `ui.text_colored` with gray `[0.5, 0.5, 0.5, 1.0]` for unavailable data — visually distinguishes from active data without being an error.

### 6. Update QWEN.md

After adding persistence, update the project's `QWEN.md`:

1. **Dependencies table** — add `rusqlite` and `chrono` rows
2. **Overlay display list** — add the new data points
3. **New section: "Run Persistence (SQLite)"** — document:
   - Database path and schema
   - Init behavior (`init_db()` runs once in `new()`)
   - Graceful degradation policy
   - What's displayed in the UI

### 7. Build and verify

```bash
cargo build --release
```

- Check that the bundled SQLite compiled (no system `sqlite3.lib` needed)
- Inject, check `%LOCALAPPDATA%\drmod\` exists with `runs.db`
- Restart DLL — previous run time should appear
- Delete the DB file — should show `N/A` without crashing

## Schema design for run tracking

Minimal schema that supports future extensibility:

```sql
CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL
);
```

- `AUTOINCREMENT` ensures IDs are strictly increasing (safe to `ORDER BY id DESC`)
- `TEXT` timestamps in ISO-like format (`YYYY-MM-DD HH:MM:SS`) are human-readable in any SQLite browser
- Future columns can be added with `ALTER TABLE ... ADD COLUMN`

## Common pitfalls

| Symptom | Cause | Fix |
|---------|-------|-----|
| `rusqlite::ffi` link errors | Missing bundled feature or wrong target | Ensure `features = ["bundled"]` in Cargo.toml |
| DLL crashes on inject, no DB created | `LOCALAPPDATA` resolves to a path with Unicode chars | Use `OsString` / `PathBuf` instead of `format!` with `String` |
| DB file created but empty | `INSERT` failed silently (ignored `_ = ...`) | Check return value; log errors via `MessageBoxW` in debug builds |
| Previous run always `N/A` | `query_row` returns `Err` because table is empty on first run | Expected behavior; `.ok()` converts to `None` correctly |
| Two runs recorded per inject | `init_db()` called twice (e.g. in both `new()` and `initialize()`) | Call `init_db()` only once — in `new()` |
