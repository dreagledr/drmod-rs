//! Headless-проверка монтирования раскладки.
//!
//! ⚠️ Ограничение, выясненное живьём 2026-09-20: `RecordingRuntime` снаружи только
//! монтирует компонент — применение команд идёт в цикле `App`, а `Pump::dispatch_events`
//! не публичен. После `mount_view`/`update_view` в записи остаётся один корневой узел без
//! вида (`nodes=1, kinds=[]`), поэтому проверить структуру (виды контролов, колонки)
//! снаружи нельзя. Расстановку колонок поэтому вынесли в `panels::timeline::row_cells()`
//! и покрыли обычным unit-тестом.
//!
//! Здесь проверяется то, что доступно: планировщик принимает декларации раскладки
//! (паника на невалидных `GridLength`, конфликт слотов и т. п. сорвала бы монтирование).

use crate::editor::Editor;
use windows_reactor::View;
use windows_reactor::test::{Pump, RecordingRuntime};

#[test]
fn editor_mounts_without_layout_errors() {
    let mut pump = Pump::new(RecordingRuntime::default());
    pump.mount_view(View::component::<Editor>(()))
        .expect("раскладка Editor должна монтироваться без ошибок планировщика");

    assert!(pump.root().is_some(), "корень дерева создан");
    assert!(!pump.poisoned(), "планировщик не отравлен");
}
