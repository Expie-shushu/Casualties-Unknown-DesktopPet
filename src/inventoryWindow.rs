// 独立仓库 GUI 窗口：winit 窗口 + 独立 wgpu surface + egui 0.30。
// 圆环装备盘（6 环形扇区）+ 下方背包栏 + 右侧子仓库网格面板；无边框 + ESC 关闭 + 空白区拖窗。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use egui::{Context, FontData, TextureHandle, ViewportId};
use egui_wgpu::{wgpu, ScreenDescriptor};
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::food::FOODS;
use crate::inventory::{Inventory, mainSlotLabel};
use crate::item::{Item, ItemKind};

const WINDOW_W: u32 = 760;
const WINDOW_H: u32 = 600;

/// 格子放入反馈动画时长（毫秒）。sin 脉冲 1.0 → ~1.24 → 1.0。
const SLOT_POP_DURATION_MS: u64 = 280;

// ── 圆环几何（egui 点坐标，y 向下）──
// 半径/间隙/颜色/字号等已迁到 InventoryStyle（用户可在仓库窗「🎨 外观」面板实时调）。
// 半角与 6 槽布局绑定（每槽 60°），固定不暴露，避免扇区重叠。
const SLOT_HALF_DEG: f32 = 30.0;
/// 6 槽 (主槽索引, 中心角度°)。egui 系：0°=右,+90°=下,-90°=上。
const SLOT_ANGLES_DEG: [(usize, f32); 6] = [
    (5, -90.0),  // 口腔 顶部
    (0, -150.0), // 主手 左上
    (1, -30.0),  // 副手 右上
    (2, 150.0),  // 上背部 左下
    (4, 90.0),   // 中背部 正底
    (3, 30.0),   // 下背部 右下
];
/// 需要加载的槽位背景图文件名（无扩展）。双手共用 hands。
const SLOT_BG_FILES: [&str; 5] = ["hands", "uptorso", "downtorso", "midtorso", "mouth"];
/// 主槽索引 → 槽位背景图文件名。双手(0/1)共用 hands；无映射返回 None。
fn slotBgFile(idx: usize) -> Option<&'static str> {
    match idx {
        0 | 1 => Some("hands"),
        2 => Some("uptorso"),
        3 => Some("downtorso"),
        4 => Some("midtorso"),
        5 => Some("mouth"),
        _ => None,
    }
}
/// 仓库内某个格子目标（拖放落点 / 右键拖出来源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotTarget {
    Main(usize),
    Pack(String, usize),
}

/// 圆环某扇区的几何（命中判定用：极坐标距离+角度）。empty 时才作为拖放落点。
#[derive(Clone, Debug)]
pub struct SectorGeom {
    pub target: SlotTarget,
    pub center: egui::Pos2,
    pub rIn: f32,
    pub rOut: f32,
    pub centerDeg: f32,
    pub halfDeg: f32,
    pub empty: bool,
}

/// 仓库窗口本帧交互结果。
#[derive(Clone, Debug, Default)]
pub struct InventoryChange {
    /// 右键主槽 idx → 拖出该格物品（喂食/放置/交换）。
    pub pickMain: Option<usize>,
    /// 右键子仓库格 (packId, slotIdx) → 拖出。
    pub pickPack: Option<(String, usize)>,
    /// 左键背包栏某背包 → 切换装备/卸下。
    pub toggleEquip: Option<String>,
    /// 右键已装备背包 → 打开其子仓库面板（窗口内自管，app 已读此字段但无需动作）。
    pub openPack: Option<String>,
    /// 右侧子仓库网格的「空格」矩形（egui 点坐标）+ 目标。拖放落点之一。
    pub emptySlots: Vec<(SlotTarget, egui::Rect)>,
    /// 右侧子仓库全部格子矩形（含 occupied），供交换拖放命中。
    pub allPackSlots: Vec<(SlotTarget, egui::Rect)>,
    /// 圆环 6 扇区几何（拖放落点：空扇区 + 点击/右键命中判定）。
    pub sectors: Vec<SectorGeom>,
    /// 本帧空白背景刚开始拖动（drag_started）→ frame() 末锚定拖动起点（全局鼠标+窗口位置）。
    pub dragStarted: bool,
    /// 本帧空白背景拖动持续中（dragged）→ frame() 末按全局鼠标位移重定位窗口。
    pub dragging: bool,
    /// 本帧外观样式被改动（编辑面板调参/恢复默认）→ app 持久化。
    pub styleChanged: bool,
    pub closed: bool,
}

/// [u8;4] RGBA → egui Color32（外观样式颜色用）。
fn c32(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

pub struct InventoryWindow {
    pub window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    pendingClose: bool,
    /// 仅当本帧需要重绘时才 present；空闲时为 false，避免阻塞式 present 拖慢主循环 / 桌宠动画。
    needsRedraw: bool,
    /// foodId -> egui 纹理（首帧懒加载）。
    icons: HashMap<String, TextureHandle>,
    foodsDir: std::path::PathBuf,
    iconsLoaded: bool,
    /// 当前打开的背包子仓库面板（右侧网格）；None=只显示圆环。
    openPanel: Option<String>,
    /// 配饰/背包贴图（inventory/<id>.png），首帧懒加载。
    accIcons: HashMap<String, TextureHandle>,
    accLoaded: bool,
    invDir: std::path::PathBuf,
    /// 槽位背景图（slotbg/<file>.png，如 hands/mouth），首帧懒加载，画在扇区图标背后。
    slotBgIcons: HashMap<String, TextureHandle>,
    slotBgLoaded: bool,
    slotBgDir: std::path::PathBuf,
    /// present 模式（Fifo 时需软节流避免钉死主循环）。
    presentMode: wgpu::PresentMode,
    lastPresentAt: Instant,
    /// 上一帧圆环 6 扇区几何（跨窗口拖放命中：空扇区作落点）。
    lastSectors: Vec<SectorGeom>,
    /// 上一帧右侧网格空格矩形 + pixels_per_point（跨窗口拖放命中）。
    lastEmptySlots: Vec<(SlotTarget, egui::Rect)>,
    lastAllPackSlots: Vec<(SlotTarget, egui::Rect)>,
    lastPixelsPerPoint: f32,
    /// egui 缩放因子（仓库整体放大/缩小）。
    inventoryScale: f32,
    /// 拖窗锚点：(拖动起点的窗口外位置, 拖动起点的全局鼠标物理坐标)。
    /// 用全局坐标而非 egui 窗口相对坐标，避免「移动窗口改变坐标系」的反馈震荡。
    dragAnchor: Option<(winit::dpi::PhysicalPosition<i32>, (i32, i32))>,
    /// 格子放入反馈动画：(目标格, 过期时刻)。过期自动移除以避免无限增长。
    slotPopAnims: Vec<(SlotTarget, std::time::Instant)>,
}

impl InventoryWindow {
    pub fn create(el: &ActiveEventLoop, foodsDir: &std::path::Path, invDir: &std::path::Path, slotBgDir: &std::path::Path, scale: f32, uiFontFamily: &str) -> Result<Self> {
        let mut attrs = Window::default_attributes()
            .with_title("Casualties Unknown：desktopPet · 仓库")
            .with_inner_size(LogicalSize::new(WINDOW_W, WINDOW_H))
            .with_decorations(false) // 无边框：整窗即圆环界面，ESC 关闭、空白区拖窗
            .with_resizable(true)
            .with_transparent(true)  // 透明窗：圆环浮空，无黑色底板。
            .with_visible(false);    // 首帧渲染成功后再显示，避免白闪
        if let Some(icon) = loadIcon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        let window: Arc<Window> = Arc::new(
            el.create_window(attrs)
                .map_err(|e| anyhow!("inventory window create: {e}"))?,
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| anyhow!("inventory surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow!("inventory adapter not found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("inventory-device"),
                required_features: wgpu::Features::empty(),
                // 用硬件真实上限（桌面 GPU 通常 8192/16384）而非 downlevel 的 2048，
                // 否则窗口放大后 surface 尺寸超 2048 会触发 wgpu 校验 panic → 闪退。
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| anyhow!("inventory device: {e}"))?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        // 钳制到设备最大纹理尺寸，杜绝放大后 surface 越界 panic（防御性）。
        let maxDim = device.limits().max_texture_dimension_2d;
        // 非阻塞 Mailbox（不支持则回退 Fifo）：避免每帧阻塞式 present 钉死主循环 → 桌宠动画卡。
        let presentMode = crate::renderer::pickPresentMode(&caps.present_modes);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.clamp(1, maxDim),
            height: size.height.clamp(1, maxDim),
            present_mode: presentMode,
            // 优先 PreMultiplied（透明合成），不支持时回退 Auto。
            alpha_mode: caps
                .alpha_modes
                .iter()
                .copied()
                .find(|m| *m == wgpu::CompositeAlphaMode::PreMultiplied)
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let ctx = Context::default();
        installFonts(&ctx, uiFontFamily);
        let window_scale = window.scale_factor() as f32;
        let egui = egui_winit::State::new(
            ctx,
            ViewportId::ROOT,
            &window,
            Some(window_scale),
            None,
            None,
        );
        let renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

        Ok(Self {
            window,
            instance,
            surface,
            device,
            queue,
            config,
            egui,
            renderer,
            pendingClose: false,
            needsRedraw: true,
            icons: HashMap::new(),
            foodsDir: foodsDir.to_path_buf(),
            iconsLoaded: false,
            openPanel: None,
            accIcons: HashMap::new(),
            accLoaded: false,
            invDir: invDir.to_path_buf(),
            slotBgIcons: HashMap::new(),
            slotBgLoaded: false,
            slotBgDir: slotBgDir.to_path_buf(),
            presentMode,
            lastPresentAt: Instant::now(),
            lastSectors: Vec::new(),
            lastEmptySlots: Vec::new(),
            lastAllPackSlots: Vec::new(),
            lastPixelsPerPoint: window_scale,
            inventoryScale: scale.clamp(0.5, 2.0),
            dragAnchor: None,
            slotPopAnims: Vec::new(),
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn handleEvent(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                let sz = self.window.inner_size();
                let maxDim = self.device.limits().max_texture_dimension_2d;
                self.config.width = sz.width.clamp(1, maxDim);
                self.config.height = sz.height.clamp(1, maxDim);
                self.surface.configure(&self.device, &self.config);
                self.needsRedraw = true;
                false
            }
            WindowEvent::CloseRequested => {
                self.pendingClose = true;
                true
            }
            _ => {
                let resp = self.egui.on_window_event(&self.window, event);
                if resp.repaint {
                    self.needsRedraw = true;
                }
                resp.consumed
            }
        }
    }

    pub fn pendingClose(&self) -> bool {
        self.pendingClose
    }

    /// 本帧是否需要重绘 present。空闲时为 false 跳过 frame()。
    /// Fifo（阻塞 present）回退时再做 ~30fps 软节流，避免每帧阻塞钉死主循环 → 桌宠卡顿。
    pub fn wantsRedraw(&self) -> bool {
        if !self.needsRedraw {
            return false;
        }
        if self.presentMode == wgpu::PresentMode::Fifo
            && self.lastPresentAt.elapsed().as_millis() < 33
        {
            return false;
        }
        true
    }

    /// 首帧把 desktopPet/foods/<id>.png 上传为 egui 纹理（最近邻，像素风不糊）。
    fn ensureIcons(&mut self) {
        if self.iconsLoaded {
            return;
        }
        self.iconsLoaded = true;
        let ctx = self.egui.egui_ctx().clone();
        for f in FOODS {
            let path = self.foodsDir.join(format!("{}.png", f.id));
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue, // 缺图：UI 显示占位文字。
            };
            let img = match image::load_from_memory(&bytes) {
                Ok(i) => i.to_rgba8(),
                Err(e) => {
                    log::warn!("inventory icon decode {} failed: {e:?}", f.id);
                    continue;
                }
            };
            let (w, h) = img.dimensions();
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
            let tex = ctx.load_texture(
                format!("food_{}", f.id),
                color,
                egui::TextureOptions::NEAREST,
            );
            self.icons.insert(f.id.to_string(), tex);
        }
    }

    /// 首帧把 inventory/<id>.png 上传为 egui 纹理（背包贴图用）。
    fn ensureAccIcons(&mut self) {
        if self.accLoaded {
            return;
        }
        self.accLoaded = true;
        let ctx = self.egui.egui_ctx().clone();
        for id in crate::item::allBackpackIds() {
            let path = self.invDir.join(format!("{id}.png"));
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let img = match image::load_from_memory(&bytes) {
                Ok(i) => i.to_rgba8(),
                Err(e) => {
                    log::warn!("acc icon decode {id} failed: {e:?}");
                    continue;
                }
            };
            let (w, h) = img.dimensions();
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
            let tex = ctx.load_texture(format!("acc_{id}"), color, egui::TextureOptions::NEAREST);
            self.accIcons.insert(id.to_string(), tex);
        }
    }

    /// 首帧把 slotbg/<file>.png 上传为 egui 纹理（槽位背景图，缺图则该槽不画背景）。
    fn ensureSlotBg(&mut self) {
        if self.slotBgLoaded {
            return;
        }
        self.slotBgLoaded = true;
        let ctx = self.egui.egui_ctx().clone();
        for file in SLOT_BG_FILES {
            let path = self.slotBgDir.join(format!("{file}.png"));
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue, // 没放这张图：该槽位不画背景。
            };
            let img = match image::load_from_memory(&bytes) {
                Ok(i) => i.to_rgba8(),
                Err(e) => {
                    log::warn!("slot bg decode {file} failed: {e:?}");
                    continue;
                }
            };
            let (w, h) = img.dimensions();
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
            let tex = ctx.load_texture(format!("slotbg_{file}"), color, egui::TextureOptions::LINEAR);
            self.slotBgIcons.insert(file.to_string(), tex);
        }
    }

    pub fn frame(
        &mut self,
        inventory: &mut Inventory,
        style: &mut crate::settings::InventoryStyle,
    ) -> InventoryChange {
        self.ensureIcons();
        self.ensureAccIcons();
        self.ensureSlotBg();
        let raw = self.egui.take_egui_input(&self.window);
        let mut change = InventoryChange::default();
        let pendingClose = &mut self.pendingClose;
        let icons = &self.icons;
        let accIcons = &self.accIcons;
        let slotBgIcons = &self.slotBgIcons;
        let openPanel = &mut self.openPanel;
        // 应用缩放因子（仓库整体缩放）。
        self.egui.egui_ctx().set_zoom_factor(self.inventoryScale);
        // 清理已过期的格子放入动画。
        self.slotPopAnims
            .retain(|(_, expire)| std::time::Instant::now() < *expire);
        let slotPopAnims = &self.slotPopAnims;
        let full = self.egui.egui_ctx().clone().run(raw, |ctx| {
            drawUi(ctx, inventory, icons, accIcons, slotBgIcons, openPanel, style, slotPopAnims, &mut change, pendingClose);
        });
        // repaint_delay==ZERO 表示 egui 正在动画 / 需立即重绘 → 下帧继续；否则转入空闲不再 present。
        self.needsRedraw = full
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|v| v.repaint_delay.is_zero())
            .unwrap_or(false)
            || !self.slotPopAnims.is_empty(); // 格子放入动画进行中则持续重绘
        self.egui
            .handle_platform_output(&self.window, full.platform_output);

        // 缓存本帧扇区 + 右侧网格空格供跨窗口拖放命中（pixels_per_point 点→物理像素换算）。
        self.lastSectors = change.sectors.clone();
        self.lastEmptySlots = change.emptySlots.clone();
        self.lastAllPackSlots = change.allPackSlots.clone();
        self.lastPixelsPerPoint = full.pixels_per_point;

        // 拖动/缩放后 surface 常返回 Outdated；循环重试最多 5 次，避免白块。
        let frame = {
            let mut f = None;
            for _ in 0..5 {
                match self.surface.get_current_texture() {
                    Ok(frame) => {
                        f = Some(frame);
                        break;
                    }
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        self.surface.configure(&self.device, &self.config);
                        continue;
                    }
                    Err(e) => {
                        log::warn!("inventory surface frame: {e:?}");
                        return change;
                    }
                }
            }
            match f {
                Some(frame) => frame,
                None => {
                    log::warn!("inventory surface keeps returning Outdated");
                    return change;
                }
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("inventory-encoder"),
            });
        for (id, delta) in &full.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let pixelsPerPoint = full.pixels_per_point;
        let primitives = self.egui.egui_ctx().tessellate(full.shapes, pixelsPerPoint);
        // 使用当前物理尺寸：resize 后 config 已同步，这里再取一次确保一致。
        let physSize = self.window.inner_size();
        let screen = ScreenDescriptor {
            size_in_pixels: [physSize.width.max(1), physSize.height.max(1)],
            pixels_per_point: pixelsPerPoint,
        };
        self.renderer
            .update_buffers(&self.device, &self.queue, &mut encoder, &primitives, &screen);
        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("inventory-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 0.0, // 透明清屏：圆环浮空，背景完全透明。
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut rpass, &primitives, &screen);
        }
        for id in &full.textures_delta.free {
            self.renderer.free_texture(id);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        self.lastPresentAt = Instant::now();
        let _ = &self.instance;
        // 空白背景拖动 → 用全局鼠标坐标锚定法移动窗口（不进 Windows 模态循环，且不会
        // 因「移动窗口改变 egui 客户区坐标系」产生左右震荡）。
        if change.dragStarted {
            if let (Ok(pos), Some(cur)) = (self.window.outer_position(), cursorScreenGlobal()) {
                self.dragAnchor = Some((pos, cur));
            }
        }
        if change.dragging {
            if let (Some((startPos, startCur)), Some(cur)) = (self.dragAnchor, cursorScreenGlobal()) {
                // 新窗口位 = 起始窗口位 + (当前全局鼠标 - 起始全局鼠标)。全局坐标不随窗口移动而变。
                let nx = startPos.x + (cur.0 - startCur.0);
                let ny = startPos.y + (cur.1 - startCur.1);
                self.window
                    .set_outer_position(winit::dpi::PhysicalPosition::new(nx, ny));
            }
        } else {
            // 未在拖动：清除锚点，下次重新锚定。
            self.dragAnchor = None;
        }
        change
    }

    pub fn requestRedraw(&self) {
        self.window.request_redraw();
    }

    pub fn setScale(&mut self, s: f32) {
        self.inventoryScale = s.clamp(0.5, 2.0);
        self.needsRedraw = true;
        self.window.request_redraw();
    }

    /// 触发指定格子的放入反馈动画（缩放脉冲），并请求重绘。
    pub fn triggerSlotPop(&mut self, target: SlotTarget) {
        self.slotPopAnims.push((
            target,
            std::time::Instant::now() + std::time::Duration::from_millis(SLOT_POP_DURATION_MS),
        ));
        self.needsRedraw = true;
    }

    /// 拖放命中判定：给定全局屏幕物理坐标，落在空扇区或右侧网格空格则返回其目标。
    /// 窗口不可见时返回 None。
    pub fn slotAtScreen(&self, screenX: f32, screenY: f32) -> Option<SlotTarget> {
        if !self.window.is_visible().unwrap_or(true) {
            return None;
        }
        // 客户区左上角的屏幕物理坐标。
        let origin = self.window.inner_position().ok()?;
        let ppp = self.lastPixelsPerPoint.max(0.001);
        // 屏幕物理 → 客户区物理 → egui 点坐标（与扇区 center=ringRect.center() 同系）。
        let p = egui::pos2((screenX - origin.x as f32) / ppp, (screenY - origin.y as f32) / ppp);
        // 先判圆环空扇区。
        for s in &self.lastSectors {
            if s.empty && sectorHit(p, s.center, s.rIn, s.rOut, s.centerDeg, s.halfDeg) {
                return Some(s.target.clone());
            }
        }
        // 再判右侧网格空格。
        for (target, rect) in &self.lastEmptySlots {
            if rect.contains(p) {
                return Some(target.clone());
            }
        }
        None
    }

    /// 同 slotAtScreen，但不区分空/占位（交换拖放用）。
    pub fn anySlotAtScreen(&self, screenX: f32, screenY: f32) -> Option<SlotTarget> {
        if !self.window.is_visible().unwrap_or(true) {
            return None;
        }
        let origin = self.window.inner_position().ok()?;
        let ppp = self.lastPixelsPerPoint.max(0.001);
        let p = egui::pos2((screenX - origin.x as f32) / ppp, (screenY - origin.y as f32) / ppp);
        // 判圆环扇区（不分空/占位）。
        for s in &self.lastSectors {
            if sectorHit(p, s.center, s.rIn, s.rOut, s.centerDeg, s.halfDeg) {
                return Some(s.target.clone());
            }
        }
        // 判子仓库全部格子（含 occupied）。
        for (target, rect) in &self.lastAllPackSlots {
            if rect.contains(p) {
                return Some(target.clone());
            }
        }
        None
    }
}

/// 取指定目标格的放入反馈动画缩放系数：sin 脉冲 1.0 → peak → 1.0，无动画返回 1.0。
fn slotPopScale(anims: &[(SlotTarget, std::time::Instant)], target: &SlotTarget) -> f32 {
    let now = std::time::Instant::now();
    for (t, expire) in anims {
        if *t == *target {
            let remaining = (*expire - now).as_secs_f32().max(0.0);
            let duration = SLOT_POP_DURATION_MS as f32 / 1000.0;
            if duration <= 0.0 {
                return 1.0;
            }
            let progress = 1.0 - remaining / duration; // 0→1
            return 1.0 + 0.24 * (progress * std::f32::consts::PI).sin();
        }
    }
    1.0
}

/// 扇形命中：点 p 到 center 的距离 ∈ [rIn,rOut] 且角度落在 [centerDeg±halfDeg]（度，归一化 ±180）。
fn sectorHit(p: egui::Pos2, center: egui::Pos2, rIn: f32, rOut: f32, centerDeg: f32, halfDeg: f32) -> bool {
    let (dx, dy) = (p.x - center.x, p.y - center.y);
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < rIn || dist > rOut {
        return false;
    }
    let ang = dy.atan2(dx).to_degrees();
    let mut d = (ang - centerDeg) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    d.abs() <= halfDeg
}

/// 画一个环形扇区（甜甜圈分段）：外弧正向 + 内弧反向，闭合填充 + 描边。
fn drawSector(
    painter: &egui::Painter,
    center: egui::Pos2,
    rIn: f32,
    rOut: f32,
    a0: f32,
    a1: f32,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    const SEG: usize = 24;
    let mut pts = Vec::with_capacity(SEG * 2 + 2);
    for i in 0..=SEG {
        let t = a0 + (a1 - a0) * (i as f32 / SEG as f32);
        pts.push(egui::pos2(center.x + rOut * t.cos(), center.y + rOut * t.sin()));
    }
    for i in 0..=SEG {
        let t = a1 + (a0 - a1) * (i as f32 / SEG as f32);
        pts.push(egui::pos2(center.x + rIn * t.cos(), center.y + rIn * t.sin()));
    }
    painter.add(egui::Shape::Path(egui::epaint::PathShape {
        points: pts,
        closed: true,
        fill,
        stroke: stroke.into(),
    }));
}

fn drawUi(
    ctx: &Context,
    inventory: &Inventory,
    icons: &HashMap<String, TextureHandle>,
    accIcons: &HashMap<String, TextureHandle>,
    slotBgIcons: &HashMap<String, TextureHandle>,
    openPanel: &mut Option<String>,
    style: &mut crate::settings::InventoryStyle,
    slotPopAnims: &[(SlotTarget, std::time::Instant)],
    change: &mut InventoryChange,
    pendingClose: &mut bool,
) {
    // ESC 关闭仓库。
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *pendingClose = true;
        change.closed = true;
    }


    // ── 右侧：子仓库网格面板（openPanel=Some 时）──
    if let Some(packId) = openPanel.clone() {
        // 卸下背包后面板自动关闭。
        let stillEquipped = inventory.backpackById(&packId).map_or(false, |b| b.equipped);
        if !stillEquipped {
            *openPanel = None;
        } else {
            egui::SidePanel::right("inv-pack")
                .resizable(false)
                .default_width(300.0)
                // 无底色 + 无分隔线：只留可放物的格子；右键背包再次点击即可关闭。
                .frame(egui::Frame::none())
                .show_separator_line(false)
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    drawBackpackGrid(ui, inventory, icons, accIcons, &packId, style, slotPopAnims, change);
                });
        }
    }

    // ── 中央：圆环装备盘 + 背包栏 ──
    // 用「固定宽度的居中列」强制圆环与背包栏共享同一中轴（背包栏必在物品栏正下方）。
    // 列宽 = 圆环直径（随 overallScale 缩放），列整体在可用宽度内居中。
    egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
        // 底层空白可拖窗（先 interact，圆环在其上）。只响应左键拖拽。
        let bgResp = ui.interact(
            ui.max_rect(),
            ui.id().with("ring-bg"),
            egui::Sense::drag(),
        );
        let s = style.overallScale.max(0.1);
        let colW = s * (2.0 * style.labelRadius + 50.0); // = 圆环 side
        let pad = ((ui.available_width() - colW) * 0.5).max(0.0);
        let colH = ui.available_height();
        let mut ringHit = false;
        ui.horizontal(|ui| {
            ui.add_space(pad);
            ui.allocate_ui_with_layout(
                egui::vec2(colW, colH),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ringHit = drawRing(ui, inventory, icons, accIcons, slotBgIcons, style, slotPopAnims, change);
                    ui.add_space(12.0);
                    // 仅显示已拥有背包；无背包则不显示该行。
                    if !inventory.backpacks.is_empty() {
                        drawBackpackBar(ui, inventory, accIcons, style, change);
                    }
                },
            );
        });
        // 空白背景被拖动（未命中圆环）→ 回报拖动状态，frame() 末用全局鼠标坐标重定位窗口。
        // 仅响应左键拖拽：Sense::drag() 在 egui 中对所有按键产生 drag_started/dragged，
        // 必须用 _by(Primary) 过滤，否则右键拖动也会移动仓库窗口。
        if !ringHit {
            if bgResp.drag_started_by(egui::PointerButton::Primary) {
                change.dragStarted = true;
            }
            if bgResp.dragged_by(egui::PointerButton::Primary) {
                change.dragging = true;
            }
        }
    });

    // 右键背包切换开/关：再次右键同一背包则关闭，否则打开（drawBackpackBar 不能直接改外层借用）。
    if let Some(id) = change.openPack.clone() {
        if openPanel.as_deref() == Some(id.as_str()) {
            *openPanel = None;
        } else {
            *openPanel = Some(id);
        }
    }
}

/// 取物品贴图（食物→icons，背包/配饰→accIcons）。
fn itemTexture<'a>(
    item: &Item,
    icons: &'a HashMap<String, TextureHandle>,
    accIcons: &'a HashMap<String, TextureHandle>,
) -> Option<&'a TextureHandle> {
    match item.kind {
        ItemKind::Food => icons.get(&item.id),
        ItemKind::Backpack | ItemKind::Accessory => accIcons.get(&item.id),
    }
}

/// 画一个子仓库格子（边长 cellSize）：空=暗块，有物=贴图。返回该格 Response（点击/右键检测）。
/// `pop_scale` 为放入反馈动画的缩放系数（无动画时 1.0）。
fn slotCell(
    ui: &mut egui::Ui,
    slot: &Option<Item>,
    icons: &HashMap<String, TextureHandle>,
    accIcons: &HashMap<String, TextureHandle>,
    cellSize: f32,
    cellBg: egui::Color32,
    popScale: f32,
) -> egui::Response {
    let scaled = cellSize * popScale;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(scaled, scaled), egui::Sense::click());
    // 格子背景按缩放尺寸绘制，放大时略微突出。
    ui.painter().rect_filled(rect, 4.0 * popScale, cellBg);
    let iconBox = (scaled - 8.0 * popScale).max(1.0);
    let shrink = (scaled - iconBox) * 0.5;
    if let Some(item) = slot {
        if let Some(tex) = itemTexture(item, icons, accIcons) {
            let img = egui::Image::new(tex).fit_to_exact_size(egui::vec2(iconBox, iconBox));
            img.paint_at(ui, rect.shrink(shrink));
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &item.id,
                egui::FontId::proportional(10.0),
                egui::Color32::LIGHT_GRAY,
            );
        }
    }
    resp
}

/// 右侧子仓库网格（每行 4 格）。空格记入 change.emptySlots（拖放落点），有物右键 pickPack。
fn drawBackpackGrid(
    ui: &mut egui::Ui,
    inventory: &Inventory,
    icons: &HashMap<String, TextureHandle>,
    accIcons: &HashMap<String, TextureHandle>,
    packId: &str,
    style: &crate::settings::InventoryStyle,
    slotPopAnims: &[(SlotTarget, std::time::Instant)],
    change: &mut InventoryChange,
) {
    let bp = match inventory.backpackById(packId) {
        Some(b) => b,
        None => {
            ui.label("背包不存在");
            return;
        }
    };
    const COLS: usize = 4;
    let s = style.overallScale.max(0.1);
    let cell = style.subCellSize * s;
    let gap = style.subCellGap * s;
    let cellBg = c32(style.subCellBg);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for rowStart in (0..bp.slots.len()).step_by(COLS) {
            ui.horizontal(|ui| {
                for i in rowStart..(rowStart + COLS).min(bp.slots.len()) {
                    let target = SlotTarget::Pack(packId.to_string(), i);
                    let pop = slotPopScale(slotPopAnims, &target);
                    let resp = slotCell(ui, &bp.slots[i], icons, accIcons, cell, cellBg, pop);
                    // 右键按下立即拾取：button_pressed 在按下帧触发，比 drag_started(需等 0.8s)
                    // 和 secondary_clicked(需松开)都快。
                    if ui.ctx().input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary))
                        && resp.hovered()
                        && bp.slots[i].is_some()
                    {
                        change.pickPack = Some((packId.to_string(), i));
                    }
                    if bp.slots[i].is_none() {
                        change
                            .emptySlots
                            .push((SlotTarget::Pack(packId.to_string(), i), resp.rect));
                    }
                    change
                        .allPackSlots
                        .push((SlotTarget::Pack(packId.to_string(), i), resp.rect));
                    ui.add_space(gap);
                }
            });
            ui.add_space(gap);
        }
    });
}

/// 圆环下方一排已拥有背包：左键装备/卸下，右键已装备背包展开子仓库。
fn drawBackpackBar(
    ui: &mut egui::Ui,
    inventory: &Inventory,
    accIcons: &HashMap<String, TextureHandle>,
    style: &crate::settings::InventoryStyle,
    change: &mut InventoryChange,
) {
    let s = style.overallScale.max(0.1);
    let cell = style.bpCellSize * s;
    let gap = style.bpCellGap * s;
    let iconBox = (cell - 6.0).max(1.0);
    ui.horizontal(|ui| {
        for bp in &inventory.backpacks {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, cell), egui::Sense::click());
            let bg = if bp.equipped { c32(style.bpEquipped) } else { c32(style.bpUnequipped) };
            ui.painter().rect_filled(rect, 4.0, bg);
            if let Some(tex) = accIcons.get(&bp.id) {
                egui::Image::new(tex)
                    .fit_to_exact_size(egui::vec2(iconBox, iconBox))
                    .paint_at(ui, rect.shrink(3.0));
            }
            if resp.clicked() {
                change.toggleEquip = Some(bp.id.clone());
            }
            if resp.secondary_clicked() && bp.equipped {
                change.openPack = Some(bp.id.clone());
            }
            ui.add_space(gap);
        }
    });
}

/// 画圆环装备盘（6 环形扇区 + 标签 + 圆心占位）。返回光标是否落在某扇区环带内（用于抑制拖窗）。
fn drawRing(
    ui: &mut egui::Ui,
    inventory: &Inventory,
    icons: &HashMap<String, TextureHandle>,
    accIcons: &HashMap<String, TextureHandle>,
    slotBgIcons: &HashMap<String, TextureHandle>,
    style: &crate::settings::InventoryStyle,
    slotPopAnims: &[(SlotTarget, std::time::Instant)],
    change: &mut InventoryChange,
) -> bool {
    let s = style.overallScale.max(0.1); // 整体大小倍率。
    let rIn = style.ringInnerR * s;
    let rOut = style.ringOuterR * s;
    let iconRadius = style.iconRadius * s;
    let labelRadius = style.labelRadius * s;
    let side = 2.0 * labelRadius + 50.0 * s;
    // click_and_drag：需要同时检测右键单击（secondary_clicked）和右键长按拖拽
    // （drag_started_by(Secondary)）。只用 click() 时底层 Sense::drag() 背景会把
    // 右键按下误判为背景拖拽，从而阻止 secondary_clicked 触发。
    let (ringRect, ringResp) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let center = ringRect.center();
    let painter = ui.painter_at(ringRect);
    let hover = ringResp.hover_pos();

    let mut anyHover = false;
    let mut sectors = Vec::with_capacity(6);
    for (idx, cDeg) in SLOT_ANGLES_DEG {
        let a0 = (cDeg - SLOT_HALF_DEG + style.sectorGapDeg * 0.5).to_radians();
        let a1 = (cDeg + SLOT_HALF_DEG - style.sectorGapDeg * 0.5).to_radians();
        let occupied = inventory.mainSlots[idx].is_some();
        let isHover = hover
            .map_or(false, |p| sectorHit(p, center, rIn, rOut, cDeg, SLOT_HALF_DEG));
        if isHover {
            anyHover = true;
        }
        let fill = if isHover {
            c32(style.sectorHover)
        } else if occupied {
            c32(style.sectorOccupied)
        } else {
            c32(style.sectorEmpty)
        };
        let stroke = egui::Stroke::new(style.strokeWidth * s, c32(style.strokeColor));
        // 格子放入反馈动画：扇形 + 图标 + 背景图一起缩放脉冲。
        let pop = slotPopScale(slotPopAnims, &SlotTarget::Main(idx));
        let popExtra = (pop - 1.0) * 0.4; // 扇形径向扩展比例（峰值~14%）。
        let popRIn = rIn * (1.0 - popExtra);
        let popROut = rOut * (1.0 + popExtra);
        drawSector(&painter, center, popRIn, popROut, a0, a1, fill, stroke);

        let mid = cDeg.to_radians();
        // 槽位背景图：画在图标背后，居中于环带，按该槽位透明度淡入。
        if let Some(file) = slotBgFile(idx) {
            if let Some(tex) = slotBgIcons.get(file) {
                let a = (style.slotBgAlpha[idx].clamp(0.0, 1.0) * 255.0) as u8;
                if a > 0 {
                    let bgR = (popRIn + popROut) * 0.5;
                    let bx = center.x + bgR * mid.cos();
                    let by = center.y + bgR * mid.sin();
                    let bgSize = (popROut - popRIn) * 0.92;
                    let bgRect =
                        egui::Rect::from_center_size(egui::pos2(bx, by), egui::vec2(bgSize, bgSize));
                    egui::Image::new(tex)
                        .tint(egui::Color32::from_white_alpha(a))
                        .paint_at(ui, bgRect);
                }
            }
        }
        // 扇区内图标 / 占位。
        let ix = center.x + iconRadius * mid.cos();
        let iy = center.y + iconRadius * mid.sin();
        let iconSize = 40.0 * s * pop;
        let iconRect = egui::Rect::from_center_size(egui::pos2(ix, iy), egui::vec2(iconSize, iconSize));
        if let Some(item) = &inventory.mainSlots[idx] {
            if let Some(tex) = itemTexture(item, icons, accIcons) {
                egui::Image::new(tex).paint_at(ui, iconRect);
            } else {
                painter.text(
                    iconRect.center(),
                    egui::Align2::CENTER_CENTER,
                    &item.id,
                    egui::FontId::proportional(10.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        }
        // 外侧标签。
        let lx = center.x + labelRadius * mid.cos();
        let ly = center.y + labelRadius * mid.sin();
        painter.text(
            egui::pos2(lx, ly),
            egui::Align2::CENTER_CENTER,
            mainSlotLabel(idx),
            egui::FontId::proportional(style.labelFontPx * s),
            c32(style.labelColor),
        );

        sectors.push(SectorGeom {
            target: SlotTarget::Main(idx),
            center,
            rIn,
            rOut,
            centerDeg: cDeg,
            halfDeg: SLOT_HALF_DEG,
            empty: !occupied,
        });
    }

    // 圆心占位文字。
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        "物品栏",
        egui::FontId::proportional(style.centerFontPx * s),
        c32(style.centerColor),
    );

    // 左键拖拽环形区 → 移动仓库窗口。ringResp 为 click_and_drag 后抢走了
    // bgResp(Sense::drag) 的拖拽权，必须在 ringResp 里转发左键拖拽。
    if ringResp.drag_started_by(egui::PointerButton::Primary) {
        change.dragStarted = true;
    }
    if ringResp.dragged_by(egui::PointerButton::Primary) {
        change.dragging = true;
    }

    // 右键按下扇形格 → 立即拿起物品（不等待 drag_started 的 0.8s 延迟）。
    // button_pressed 在按下帧触发，图标即时出现在光标上。
    if ui.ctx().input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary)) {
        if let Some(p) = ringResp.hover_pos() {
            for (idx, cDeg) in SLOT_ANGLES_DEG {
                if sectorHit(p, center, rIn, rOut, cDeg, SLOT_HALF_DEG) {
                    if inventory.mainSlots[idx].is_some() {
                        change.pickMain = Some(idx);
                    }
                    break;
                }
            }
        }
    }

    change.sectors = sectors;
    // 返回 true 阻止 bgResp 重复处理左键拖拽（即使鼠标不在扇区上也阻止，
    // 因为 ringResp 会处理整个矩形区域内的左键拖拽）。
    ringResp.hovered() || anyHover
}

fn loadIcon() -> Option<winit::window::Icon> {
    const PNG: &[u8] = include_bytes!("../icons/icon.png");
    let img = image::load_from_memory(PNG).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

fn installFonts(ctx: &Context, uiFontFamily: &str) {
    let mut fonts = egui::FontDefinitions::default();
    let font_loaded = if !uiFontFamily.is_empty() {
        loadUserFont(&mut fonts, uiFontFamily)
    } else {
        false
    };
    if !font_loaded {
        if let Some(path) = findCJKFont() {
            if let Ok(bytes) = std::fs::read(&path) {
                let key = "petCJK".to_string();
                fonts
                    .font_data
                    .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, key.clone());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push(key);
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn loadUserFont(fonts: &mut egui::FontDefinitions, family: &str) -> bool {
    let font_dir = std::path::PathBuf::from(
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default(),
    )
    .join("desktopPet")
    .join("fonts");
    if let Some(path) = crate::text::findFontFile(family, &font_dir) {
        if let Ok(bytes) = std::fs::read(&path) {
            let key = "userUI".to_string();
            fonts
                .font_data
                .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, key.clone());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push(key);
            return true;
        }
    }
    false
}

#[cfg(windows)]
fn findCJKFont() -> Option<std::path::PathBuf> {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];
    candidates
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.exists())
}

#[cfg(not(windows))]
fn findCJKFont() -> Option<std::path::PathBuf> {
    None
}

/// 全局鼠标物理屏幕坐标（像素）。用于拖窗锚点：与 set_outer_position 同坐标系，
/// 且不随窗口移动而变，避免 egui 窗口相对坐标做拖窗增量时的左右震荡。
#[cfg(windows)]
fn cursorScreenGlobal() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut pt) };
    if ok == 0 {
        None
    } else {
        Some((pt.x, pt.y))
    }
}

#[cfg(not(windows))]
fn cursorScreenGlobal() -> Option<(i32, i32)> {
    None
}
