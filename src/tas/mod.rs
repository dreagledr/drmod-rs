//! TAS (Tool-Assisted Speedrun) — модуль записи и воспроизведения ввода,
//! а в дальнейшем — инструментов для построения TAS-прохождения.

pub mod addresses;
pub mod db;
pub mod hooks;
pub mod replay;
pub mod types;
/// Аппаратная точка останова на запись (DR0 + VEH) — диагностика, debug-only.
#[cfg(debug_assertions)]
pub(crate) mod watch;
