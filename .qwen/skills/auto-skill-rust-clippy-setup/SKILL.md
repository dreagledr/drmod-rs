---
name: rust-clippy-setup
description: Set up Clippy in a Rust project — fix all warnings, add [lints.clippy] to Cargo.toml for always-on linting, and tune clippy.toml thresholds
source: auto-skill
extracted_at: '2026-07-14T15:17:00.000Z'
---

# Rust Clippy Setup and Enforcement

Configure a Rust project so that Clippy lints are always active — warnings appear during `cargo check`/`cargo build`, not just `cargo clippy`. Includes fixing existing warnings and tuning thresholds for the project's style.

## When to use

- A Rust project has no Clippy configuration (no `[lints.clippy]` in Cargo.toml, no `clippy.toml`)
- The user wants Clippy to run automatically on every build, not just when explicitly invoked
- There are existing Clippy warnings that need cleanup

## Procedure

### 1. Discover existing warnings

```bash
cargo clippy --all-targets 2>&1
```

Capture all warnings. Group them by type to see the landscape before fixing.

### 2. Fix warnings by category

Fix each warning systematically. Common categories and their fixes:

| Warning | Fix |
|---------|-----|
| `type_complexity` | Extract a type alias: `type Foo = (A, B, C);` |
| `manual_flatten` | Replace `for row in rows { if let Ok(x) = row { ... } }` with `for x in rows.flatten() { ... }` |
| `collapsible_if` | Merge with `&& let`: `if a && let Some(x) = b { ... }` (requires Rust ≥1.78) |
| `needless_borrow` | Remove `ref` from patterns where a reference already exists: `Some(ref x)` → `Some(x)` when matching on `&Option<T>` |
| `needless_return` | Remove the `return` keyword: `return Ok(x);` → `Ok(x)` |

**Important**: use `rustfmt`-compatible formatting. After edits, verify with `cargo fmt --check`.

### 3. Add `[lints.clippy]` to Cargo.toml

This is the core step — it makes Clippy lints part of every `cargo check`/`cargo build`:

```toml
[lints.clippy]
all = { level = "warn", priority = -1 }
missing_safety_doc = "allow"
```

**Why `priority = -1`:** Without it, `all = "warn"` and individual lint overrides (like `missing_safety_doc = "allow"`) have the same priority, causing a `lint_groups_priority` warning. Setting `all` to priority `-1` lets individual lints override the group.

**`missing_safety_doc = "allow"`:** For projects with heavy `unsafe` usage (game hacking, FFI), the `missing_safety_doc` lint is unreasonably noisy. Allow it explicitly so `all = "warn"` doesn't re-enable it.

### 4. Create `clippy.toml` with project-specific thresholds

Default Clippy thresholds are conservative. For projects with complex logic (game overlays, render loops, FFI), raise them:

```toml
# clippy.toml
cognitive-complexity-threshold = 35    # default: 25
too-many-arguments-threshold = 10      # default: 7
type-complexity-threshold = 300        # default: 250
```

Place the file in the project root (not `.cargo/clippy.toml`). Standard location works with all tooling.

### 5. Verify

```bash
cargo clippy --all-targets   # must pass with 0 warnings
cargo check                   # must also pass (now inherits clippy lints)
```

Both must exit clean. If `cargo check` emits warnings that `cargo clippy` doesn't, double-check the `[lints.clippy]` section syntax.

## When NOT to apply

- **No** `clippy::pedantic` or `clippy::nursery` by default — they are too noisy for most projects and should be opted into deliberately
- **No** `clippy::all = "deny"` — use `warn` so that development isn't blocked by non-critical style nits; use CI to enforce clean clippy on PRs instead
- **Don't** silence warnings with `#[allow(...)]` annotations unless the lint is genuinely a false positive for that specific site. Fix the code first.

## Common pitfalls

| Symptom | Cause | Fix |
|---------|-------|-----|
| `lint_groups_priority` warning after adding `[lints.clippy]` | `all = "warn"` and individual lint overrides have same priority | Use `all = { level = "warn", priority = -1 }` |
| `collapsible_if` fix doesn't compile | `&& let` chains require Rust ≥1.78 | Check `rust-version` in Cargo.toml or bump MSRV |
| `type_complexity` fires on a type alias in another module | The alias needs to be `pub` if used across module boundaries | Make the type alias `pub type` |
