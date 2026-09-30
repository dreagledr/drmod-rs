# drmod-hudhook

A DirectX 9 renderer-hook library for building [Dear ImGui](https://github.com/ocornut/imgui)
overlays, forked from [`hudhook`](https://github.com/veeenu/hudhook) for
[drmod-rs](../QWEN.md).

## Why a fork

`hudhook` upstream supports several rendering backends — DirectX 9, DirectX 11,
DirectX 12 and OpenGL 3 — plus a variety of examples, tests and a tutorial book
for each. MGR:R is a DirectX 9 title, so drmod-rs uses only the DX9 path. Rather
than carry (and build) three unused backends, this fork keeps **only DX9** and
drops everything upstream's other backends pull in.

What the fork changes relative to upstream `hudhook` 0.9.0:

- **Orphaned backends removed.** `dx11`, `dx12` and `opengl3` hooks, their
  renderer backends, examples, tests and harnesses are gone, along with the
  `hudbook/` tutorial and the upstream `.github/` workflows. `gl_generator` and
  the `windows` features only those backends needed are no longer dependencies.
  `minhook` and the pipeline/input plumbing shared by all backends stay.
- **Renamed.** The crate is `drmod-hudhook`; the `DllMain` macro is
  `drmod_hudhook!` (was `hudhook!`) and refers to `::drmod_hudhook` — it does not
  depend on how the caller renames the dependency.
- **Headless switches.** Two process-global flags let the injected mod drop the
  overlay:
  - `set_skip_draw(bool)` — the DX9 backend does not send the ImGui geometry to
    the device;
  - `set_skip_present(bool)` / `skip_present()` — the `Present` detour does not
    call the real `Present`, so the frame is never blitted to the window.

  They exist for the mod's `POST /render` headless runs (see
  [`../docs/HEADLESS.md`](../docs/HEADLESS.md)). Both are additive changes; a
  plain overlay that never touches them behaves exactly as upstream.

Everything else — the hook machinery, the render pipeline, input handling and
the public API — is upstream `hudhook` and is documented at its
[rustdoc](https://veeenu.github.io/hudhook/rustdoc/hudhook) and
[tutorial book](https://veeenu.github.io/hudhook).

## Example

```rust
// src/lib.rs
use drmod_hudhook::*;

pub struct MyRenderLoop;

impl ImguiRenderLoop for MyRenderLoop {
    fn render(&mut self, ui: &mut imgui::Ui) {
        ui.window("My first render loop")
            .position([0., 0.], imgui::Condition::FirstUseEver)
            .size([320., 200.], imgui::Condition::FirstUseEver)
            .build(|| {
                ui.text("Hello, hello!");
            });
    }
}

use drmod_hudhook::hooks::dx9::ImguiDx9Hooks;
drmod_hudhook!(ImguiDx9Hooks, MyRenderLoop);
```

```rust
// src/main.rs
use drmod_hudhook::inject::Process;

fn main() {
    let dll = std::path::PathBuf::from("libmyhook.dll").canonicalize().unwrap();
    Process::by_name("MyTargetApplication.exe").unwrap().inject(dll).unwrap();
}
```

## Building and testing

The crate belongs to the drmod-rs workspace; build it from the repository root
(`cargo build --release`). The DX9 test harness creates its own window and device
and is meant to run on a desktop, not headless:

```bash
cargo test -p drmod-hudhook
```

⚠️ `tests/dx9.rs` opens two windows and takes a few seconds; the two `test_inject_*`
tests are `#[ignore]`d and outdated (they expect an old Notepad window title).
The `src/util.rs` doctests are illustrative fragments and are `ignore`d.

Both `minhook` and the `windows` crate are pinned by this crate's `Cargo.toml`
and `build.rs`, which compiles `../vendor/minhook/src/*.c` into `libminhook.a`.

## License

MIT, as upstream. See [LICENSE](LICENSE).
