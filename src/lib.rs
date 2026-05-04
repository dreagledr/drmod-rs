use hudhook::ImguiRenderLoop;
use imgui::*;
use std::time::Instant;

struct HelloHud {
    start_time: Instant,
}

impl HelloHud {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
        }
    }
}

impl ImguiRenderLoop for HelloHud {
    fn render(&mut self, ui: &mut Ui) {
        ui.window("##hello")
            .size([320., 200.], Condition::Always)
            .build(|| {
                ui.text("Hello, world!");
                ui.text(format!("Elapsed: {:?}", self.start_time.elapsed()));
            });
    }
}

hudhook::hudhook!(hudhook::hooks::dx9::ImguiDx9Hooks, HelloHud::new());
