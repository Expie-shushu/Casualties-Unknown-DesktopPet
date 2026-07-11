// 喂食拖拽：Idle ↔ Carrying{foodId}。携带时一个透明置顶穿透小窗跟随光标，绘制等比缩放的食物图标。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

use crate::asset::SpriteAsset;
use crate::renderer::spritePipeline::buildSpriteMatrix;
use crate::renderer::{Renderer, SpriteDraw};

/// 跟随小窗的逻辑边长（像素）。
const DRAG_WIN: u32 = 96;
/// 图标在小窗内的目标显示边长（留白）。
const ICON_BOX: f32 = 64.0;

/// 拖拽喂食状态机。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedDragState {
    /// 空闲：无拖拽中食物。
    Idle,
    /// 携带中：记录正在拖拽的食物 id（先不扣库存）。
    Carrying { foodId: String },
}

impl Default for FeedDragState {
    fn default() -> Self {
        Self::Idle
    }
}

impl FeedDragState {
    pub fn start(&mut self, foodId: &str) {
        *self = FeedDragState::Carrying {
            foodId: foodId.to_string(),
        };
    }

    pub fn cancel(&mut self) {
        *self = FeedDragState::Idle;
    }

    pub fn currentFood(&self) -> Option<&str> {
        match self {
            FeedDragState::Idle => None,
            FeedDragState::Carrying { foodId } => Some(foodId.as_str()),
        }
    }

    pub fn isCarrying(&self) -> bool {
        matches!(self, FeedDragState::Carrying { .. })
    }
}

/// 跟随光标的透明置顶穿透小窗：独立 wgpu Renderer + 食物图标贴图缓存。
pub struct FeedDragWindow {
    pub window: Arc<Window>,
    renderer: Renderer,
    /// foodId -> 图标贴图。
    icons: HashMap<String, SpriteAsset>,
    foodsDir: std::path::PathBuf,
    /// 非食物物品（背包/配饰）图标目录。
    accDir: std::path::PathBuf,
    /// 当前携带的食物 id。
    foodId: String,
}

impl FeedDragWindow {
    /// 创建跟随小窗并加载指定物品图标。`foodsDir` = desktopPet/foods，`accDir` = desktopPet/inventory。
    /// 暂不设置鼠标穿透：首帧渲染成功后再穿透，避免 WS_EX_TRANSPARENT 干扰首次呈现。
    pub fn create(
        el: &ActiveEventLoop,
        foodsDir: &std::path::Path,
        accDir: &std::path::Path,
        foodId: &str,
    ) -> Result<Self> {
        let attrs = Window::default_attributes()
            .with_title("pet-feed-drag")
            .with_inner_size(LogicalSize::new(DRAG_WIN, DRAG_WIN))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false);
        let attrs = applyDragWinAttrs(attrs);
        let window: Arc<Window> = Arc::new(el.create_window(attrs)?);
        let renderer = Renderer::new(window.clone())?;

        let mut me = Self {
            window,
            renderer,
            icons: HashMap::new(),
            foodsDir: foodsDir.to_path_buf(),
            accDir: accDir.to_path_buf(),
            foodId: String::new(),
        };
        me.setFood(foodId);
        Ok(me)
    }

    /// 首次显示后调用：使小窗鼠标穿透，不拦截点击/拖拽事件。
    pub fn enablePassthrough(&self) {
        let _ = self.window.set_cursor_hittest(false);
    }

    /// 切换当前携带物品图标（复用同一小窗）。先在 foodsDir 查找，未找到再查 accDir。
    pub fn setFood(&mut self, foodId: &str) {
        self.foodId = foodId.to_string();
        if !self.icons.contains_key(foodId) {
            // 先查 foodsDir，再查 accDir（背包/配饰图标在 inventory 目录）。
            let path = self.foodsDir.join(format!("{foodId}.png"));
            let bytes = std::fs::read(&path)
                .or_else(|_| std::fs::read(self.accDir.join(format!("{foodId}.png"))));
            match bytes {
                Ok(bytes) => match self.renderer.factory().fromPng(&bytes, foodId) {
                    Ok(a) => {
                        self.icons.insert(foodId.to_string(), a);
                    }
                    Err(e) => log::warn!("feed icon decode {foodId} failed: {e:?}"),
                },
                Err(e) => log::warn!("feed icon read {foodId} failed: {e:?}"),
            }
        }
    }

    pub fn id(&self) -> winit::window::WindowId {
        self.window.id()
    }

    /// 把小窗移动到光标处（逻辑坐标），让图标中心对准光标。不改变可见性。
    pub fn followCursor(&self, cursorLogicalX: f32, cursorLogicalY: f32) {
        let half = DRAG_WIN as f32 * 0.5;
        let _ = self.window.set_outer_position(LogicalPosition::new(
            cursorLogicalX - half,
            cursorLogicalY - half,
        ));
    }

    /// 首帧定位：移到光标处 → 渲染图标（可重试） → 再显示窗口。
    /// 不在此处设穿透——首帧渲染时 WS_EX_TRANSPARENT 可能干扰呈现；
    /// 穿透由 tickFeedDrag 在后续帧调用 enablePassthrough() 设置。
    pub fn showAtCursor(&mut self, cursorLogicalX: f32, cursorLogicalY: f32) {
        let half = DRAG_WIN as f32 * 0.5;
        let _ = self.window.set_outer_position(LogicalPosition::new(
            cursorLogicalX - half,
            cursorLogicalY - half,
        ));
        // 先渲染，再显示。
        self.render();
        self.window.set_visible(true);
    }

    /// 渲染图标：等比缩放进 ICON_BOX，居中，最近邻采样（像素风不糊）。
    pub fn render(&mut self) {
        let w = self.renderer.config.width as f32;
        let h = self.renderer.config.height as f32;
        let mut draws: Vec<SpriteDraw> = Vec::new();
        if let Some(icon) = self.icons.get(&self.foodId) {
            let (iw, ih) = (icon.width as f32, icon.height as f32);
            // 等比缩放：较长边贴合 ICON_BOX，保持宽高比不变形。
            let fit = (ICON_BOX / iw).min(ICON_BOX / ih);
            let dw = iw * fit;
            let dh = ih * fit;
            let m = buildSpriteMatrix(w, h, w * 0.5, h * 0.5, dw, dh, 0.0, 1.0);
            draws.push(SpriteDraw::full(icon, m));
        }
        if let Err(e) = self.renderer.renderFrame(&draws) {
            log::warn!("feed drag render: {e:?}");
        }
    }

    pub fn requestRedraw(&self) {
        self.window.request_redraw();
    }
}

#[cfg(windows)]
fn applyDragWinAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    attrs.with_skip_taskbar(true).with_drag_and_drop(false)
}

#[cfg(not(windows))]
fn applyDragWinAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs
}
