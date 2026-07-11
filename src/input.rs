// 鼠标输入：拖拽抓取 + 双击检测 + 三档穿透。
#![allow(non_snake_case)]

use std::time::Instant;

use winit::window::Window;

#[derive(Clone, Copy, Debug, Default)]
pub struct InputState {
    pub lastCursorClient: (f32, f32),
    pub dragging: bool,
    pub dragGrabClient: (f32, f32),
    pub lastLeftPressAt: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenetrationMode {
    Always,
    Never,
    Smart,
}

impl InputState {
    pub fn onLeftPress(&mut self) -> bool {
        let now = Instant::now();
        let isDouble = match self.lastLeftPressAt {
            Some(prev) => (now - prev).as_millis() < 400,
            None => false,
        };
        self.lastLeftPressAt = Some(now);
        self.dragging = true;
        self.dragGrabClient = self.lastCursorClient;
        isDouble
    }

    pub fn onLeftRelease(&mut self) {
        self.dragging = false;
    }

    pub fn onCursorMove(&mut self, x: f32, y: f32) -> Option<(f32, f32)> {
        self.lastCursorClient = (x, y);
        if self.dragging {
            let dx = x - self.dragGrabClient.0;
            let dy = y - self.dragGrabClient.1;
            Some((dx, dy))
        } else {
            None
        }
    }
}

pub fn applyPenetration(window: &Window, mode: PenetrationMode) {
    let want = match mode {
        PenetrationMode::Always => false,
        PenetrationMode::Never => true,
        PenetrationMode::Smart => true,
    };
    let _ = window.set_cursor_hittest(want);
}
