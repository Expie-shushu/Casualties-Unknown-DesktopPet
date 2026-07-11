// 掉落物透明窗：透明、无边框、置顶、可命中（set_cursor_hittest(true)）小窗，
// 停在屏幕某处，携带一个 Item，按 item.kind 选图标目录。
#![allow(non_snake_case)]

use std::sync::Arc;

use anyhow::Result;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

use crate::asset::SpriteAsset;
use crate::item::{Item, ItemKind};
use crate::renderer::spritePipeline::buildSpriteMatrix;
use crate::renderer::{Renderer, SpriteDraw};

/// 透明置顶小窗的逻辑边长（像素）。pub：app 据此算随机落点与停驻线。
pub const WIN: u32 = 80;
/// 图标在小窗内的目标显示边长（留白）。
const ICON_BOX: f32 = 72.0;
/// 落体重力加速度（逻辑像素/秒²）。
const GRAVITY: f32 = 900.0;
/// 落体终速上限（逻辑像素/秒），让掉落"慢慢"落而非瞬移。
const MAX_FALL: f32 = 300.0;

/// 右键抓取反馈动画时长（秒）。
const GRAB_ANIM_DURATION: f32 = 0.24;

/// 屏幕上一个待拾取的掉落物：透明置顶可命中小窗 + 携带 Item。
pub struct DroppedItem {
    pub window: Arc<Window>,
    renderer: Renderer,
    /// 图标贴图（按 item.kind 选目录加载）。
    icon: Option<SpriteAsset>,
    pub item: Item,
    /// 是否正在被拖拽（由外部状态机管理）。
    pub dragging: bool,
    /// 窗口左上角逻辑屏幕坐标（落体/拖动时持续更新）。
    x: f32,
    y: f32,
    /// 竖直速度（逻辑像素/秒），落体用。
    vy: f32,
    /// 停驻 Y（窗口左上角）：到此即视为落到地平线，窗口底边贴地平线。
    restY: f32,
    /// 右键抓取反馈动画剩余秒数；>0 时播放缩放脉冲。
    grab_anim_t: f32,
}

impl DroppedItem {
    /// 创建掉落物窗。`topLeftX/topLeftY`=窗口左上角逻辑屏幕坐标（生成在地平线以上的半空）；
    /// `horizonY`=地平线逻辑屏幕 Y（桌宠脚底线），停驻时窗口底边落在此线上。
    /// - `foodsDir`：食物图标目录（Food 类物品）。
    /// - `accDir`：饰品/背包图标目录（非 Food 类物品）。
    pub fn create(
        el: &ActiveEventLoop,
        foodsDir: &std::path::Path,
        accDir: &std::path::Path,
        item: Item,
        topLeftX: f32,
        topLeftY: f32,
        horizonY: f32,
    ) -> Result<Self> {
        // 停驻线（窗口左上角）：使窗口底边正好落在地平线上。
        let restY = horizonY - WIN as f32;
        let attrs = Window::default_attributes()
            .with_title("pet-drop")
            .with_inner_size(LogicalSize::new(WIN, WIN))
            .with_position(LogicalPosition::new(topLeftX, topLeftY))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false); // 先隐藏，首帧渲染后再显示（避免白闪）
        let attrs = applyDropAttrs(attrs);
        let window: Arc<Window> = Arc::new(el.create_window(attrs)?);
        // 可命中：能右键抓取，区别于 feedDrag 的穿透窗。
        let _ = window.set_cursor_hittest(true);
        let renderer = Renderer::new(window.clone())?;

        // 按物品类型选图标目录，加载对应 PNG。
        let dir = match item.kind {
            ItemKind::Food => foodsDir,
            _ => accDir,
        };
        let icon = std::fs::read(dir.join(format!("{}.png", item.id)))
            .ok()
            .and_then(|b| renderer.factory().fromPng(&b, &item.id).ok());

        Ok(Self {
            window,
            renderer,
            icon,
            item,
            dragging: false,
            x: topLeftX,
            y: topLeftY,
            vy: 0.0,
            restY,
            grab_anim_t: 0.0,
        })
    }

    /// 返回窗口 ID，供事件分发用。
    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// 拖动中：把小窗移动到光标处（逻辑坐标，图标中心对准光标），并同步内部坐标、清空落速。
    /// 松手后从当前位置按重力重新下落。
    pub fn followCursor(&mut self, lx: f32, ly: f32) {
        let half = WIN as f32 * 0.5;
        self.x = lx - half;
        self.y = ly - half;
        self.vy = 0.0;
        let _ = self.window.set_outer_position(LogicalPosition::new(self.x, self.y));
    }

    /// 落体一帧：若窗口在地平线以上（y &lt; restY），按重力加速下落到 restY 停住。
    /// 返回是否发生移动（移动了才需重绘）。
    pub fn tickFall(&mut self, dt: f32) -> bool {
        if self.y >= self.restY {
            return false; // 已落到地平线，静止。
        }
        self.vy = (self.vy + GRAVITY * dt).min(MAX_FALL);
        self.y = (self.y + self.vy * dt).min(self.restY);
        if self.y >= self.restY {
            self.vy = 0.0; // 到地停住。
        }
        let _ = self.window.set_outer_position(LogicalPosition::new(self.x, self.y));
        true
    }

    /// 渲染图标：等比缩放进 ICON_BOX，居中显示。若抓取动画进行中则叠加缩放脉冲。
    pub fn render(&mut self) {
        let w = self.renderer.config.width as f32;
        let h = self.renderer.config.height as f32;
        let mut draws: Vec<SpriteDraw> = Vec::new();
        // 抓取反馈缩放：sin 脉冲 1.0 → ~1.48 → 1.0，模拟挤压拉伸选中感。
        let grab_scale = if self.grab_anim_t > 0.0 {
            let progress = 1.0 - self.grab_anim_t / GRAB_ANIM_DURATION; // 0→1
            1.0 + 0.48 * (progress * std::f32::consts::PI).sin()
        } else {
            1.0
        };
        if let Some(icon) = &self.icon {
            let (iw, ih) = (icon.width as f32, icon.height as f32);
            // 等比缩放：较长边贴合 ICON_BOX，保持宽高比不变形。
            let fit = (ICON_BOX / iw).min(ICON_BOX / ih);
            let dw = iw * fit * grab_scale;
            let dh = ih * fit * grab_scale;
            let m = buildSpriteMatrix(w, h, w * 0.5, h * 0.5, dw, dh, 0.0, 1.0);
            draws.push(SpriteDraw::full(icon, m));
        }
        if let Err(e) = self.renderer.renderFrame(&draws) {
            log::warn!("drop item render: {e:?}");
        }
    }

    /// 请求重绘（下一帧渲染）。
    pub fn requestRedraw(&self) {
        self.window.request_redraw();
    }

    /// 启动右键抓取反馈动画（挤压拉伸脉冲）。
    pub fn startGrabAnim(&mut self) {
        self.grab_anim_t = GRAB_ANIM_DURATION;
    }

    /// 推进抓取反馈动画计时器（每帧调用）。
    pub fn tickAnim(&mut self, dt: f32) {
        if self.grab_anim_t > 0.0 {
            self.grab_anim_t = (self.grab_anim_t - dt).max(0.0);
        }
    }

    /// 抓取反馈动画是否仍在播放（用于外部判断是否需要持续重绘）。
    pub fn grabAnimActive(&self) -> bool {
        self.grab_anim_t > 0.0
    }
}

/// Windows 平台：隐藏任务栏图标、禁用拖放，保持与 feedDrag 一致的窗属性。
#[cfg(windows)]
fn applyDropAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    attrs.with_skip_taskbar(true).with_drag_and_drop(false)
}

#[cfg(not(windows))]
fn applyDropAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs
}
