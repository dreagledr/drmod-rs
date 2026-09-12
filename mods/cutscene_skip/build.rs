//! Сборка завендоренного MinHook (x86). Пути относительны крейта — исходники
//! лежат в `vendor/minhook`, чтобы крейт был самодостаточен (см. README).

use std::path::Path;

fn main() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/minhook/src");

    cc::Build::new()
        .file(src.join("buffer.c"))
        .file(src.join("hook.c"))
        .file(src.join("trampoline.c"))
        .file(src.join("hde/hde32.c"))
        .compile("libminhook.a");

    println!("cargo:rerun-if-changed=vendor/minhook/src");
}
