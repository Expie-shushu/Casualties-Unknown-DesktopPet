#![allow(non_snake_case)]
use crate::item::Item;
use crate::settings::{WheelConfig, WheelHighlightZone};

// ── Reward Wheel 独立窗口 ─────────────────────────────────────────────────────

use std::sync::Arc;
use anyhow::{anyhow, Result};
use egui::{Context, FontData};
use egui_wgpu::{wgpu, ScreenDescriptor};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

// 窗口/转盘几何现在从 WheelConfig 读取，不再使用硬编码常量。

/// 奖励类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RewardKind {
    Food,
    Drink,
    Backpack,
}

/// 三个扇区：面积比 40% / 40% / 20%
const SEGMENTS: &[(f32, &str, RewardKind)] = &[
    (0.40, "食", RewardKind::Food),     // 144°
    (0.40, "饮", RewardKind::Drink),    // 144°
    (0.20, "包", RewardKind::Backpack), // 72°
];

/// 每帧由 frame() 返回；调用方据此处理代币消耗 / 奖励掉落 / 关窗。
pub struct RewardWheelChange {
    pub reward: Option<Item>,
    pub closed: bool,
    pub coinSpent: bool,
    // ── UI → frame() 信号（内部用）──
    spinRequested: bool,
}

impl Default for RewardWheelChange {
    fn default() -> Self {
        Self {
            reward: None,
            closed: false,
            coinSpent: false,
            spinRequested: false,
        }
    }
}

/// 抽奖转盘独立透明窗口。
pub struct RewardWheelWindow {
    pub window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    pendingClose: bool,
    needsRedraw: bool,
    presentMode: wgpu::PresentMode,
    lastPresentAt: std::time::Instant,

    // ── 布局配置（可实时更新）──
    wheelCfg: WheelConfig,

    // ── 转盘状态 ──
    rotation: f32,
    spinning: bool,
    spinVelocity: f32,
    coinInserted: bool,
    rewardAngle: f32,
    lastFrameTime: std::time::Instant,
    // ── 拖拽状态 ──
    draggingToken: bool,
    dragHoverSlot: bool,
    // ── 窗口拖动（winit 级别）──
    winDragActive: bool,
    winDragStartCursor: Option<(f64, f64)>,
    winDragStartPos: Option<(i32, i32)>,
    // ── 配置窗口 ──
    pendingOpenCfg: bool,
    // ── PNG 纹理（desktopPet/wheel/*.png）──
    wheelDir: std::path::PathBuf,
    coinTex: Option<egui::TextureHandle>,
    pointerTex: Option<egui::TextureHandle>,
    closeTex: Option<egui::TextureHandle>,
    lockedTex: Option<egui::TextureHandle>,
    pressTex: Option<egui::TextureHandle>,
    foodIconTex: Option<egui::TextureHandle>,
    drinkIconTex: Option<egui::TextureHandle>,
    packIconTex: Option<egui::TextureHandle>,
    texturesLoaded: bool,
    /// PRESS 按钮 hover 缩放平滑值
    pressHoverScale: f32,
    /// LOCKED 按钮点击抖动开始时间
    shakeStart: Option<std::time::Instant>,
    /// PRESS 按钮点击按压动画开始时间
    pressStart: Option<std::time::Instant>,
    /// 等待按压动画结束后再启动旋转
    pendingSpin: bool,
    /// 配置窗口 hover 时高亮的区域
    highlightZone: Option<WheelHighlightZone>,
}

impl RewardWheelWindow {
    pub fn create(el: &ActiveEventLoop, wheelDir: &std::path::Path, cfg: &WheelConfig) -> Result<Self> {
        let winW = cfg.windowWidth.max(300.0);
        let winH = cfg.windowHeight.max(400.0);
        let attrs = Window::default_attributes()
            .with_title("reward-wheel")
            .with_inner_size(LogicalSize::new(winW, winH))
            .with_decorations(false)
            .with_transparent(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false);
        let attrs = applyWheelAttrs(attrs);
        let window: Arc<Window> = Arc::new(
            el.create_window(attrs)
                .map_err(|e| anyhow!("wheel window create: {e}"))?,
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| anyhow!("wheel surface: {e}"))?;
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .ok_or_else(|| anyhow!("wheel adapter not found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("wheel-device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| anyhow!("wheel device: {e}"))?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let maxDim = device.limits().max_texture_dimension_2d;
        let presentMode = crate::renderer::pickPresentMode(&caps.present_modes);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.clamp(1, maxDim),
            height: size.height.clamp(1, maxDim),
            present_mode: presentMode,
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
        installWheelFonts(&ctx, "");
        let egui = egui_winit::State::new(
            ctx,
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
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
            presentMode,
            lastPresentAt: std::time::Instant::now(),
            rotation: 0.0,
            spinning: false,
            spinVelocity: 0.0,
            coinInserted: false,
            rewardAngle: 0.0,
            lastFrameTime: std::time::Instant::now(),
            draggingToken: false,
            dragHoverSlot: false,
            winDragActive: false,
            winDragStartCursor: None,
            winDragStartPos: None,
            pendingOpenCfg: false,
            wheelDir: wheelDir.to_path_buf(),
            wheelCfg: cfg.clone(),
            coinTex: None,
            pointerTex: None,
            closeTex: None,
            lockedTex: None,
            pressTex: None,
            foodIconTex: None,
            drinkIconTex: None,
            packIconTex: None,
            texturesLoaded: false,
            pressHoverScale: 1.0,
            shakeStart: None,
            pressStart: None,
            pendingSpin: false,
            highlightZone: None,
        })
    }

    pub fn setPos(&self, lx: f32, ly: f32) {
        let (lx, ly) = clampToScreen(lx, ly, self.window.clone());
        let _ = self.window.set_outer_position(LogicalPosition::new(lx, ly));
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn handleEvent(&mut self, event: &WindowEvent) -> bool {
        // ── winit 级别窗口拖动：左键在空白区域拖动 → 移动窗口 ──
        match event {
            WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                self.winDragActive = true;
                if let Some(cur) = crate::musicPlayerWindow::cursorScreenGlobal() {
                    self.winDragStartCursor = Some((cur.0 as f64, cur.1 as f64));
                    if let Ok(pos) = self.window.outer_position() {
                        self.winDragStartPos = Some((pos.x, pos.y));
                    }
                }
                // 仍然交给 egui 处理（按钮点击等）
                let resp = self.egui.on_window_event(&self.window, event);
                self.needsRedraw = true;
                return resp.consumed;
            }
            WindowEvent::MouseInput {
                state: winit::event::ElementState::Released,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                self.winDragActive = false;
                self.winDragStartCursor = None;
                self.winDragStartPos = None;
                let resp = self.egui.on_window_event(&self.window, event);
                self.needsRedraw = true;
                return resp.consumed;
            }
            WindowEvent::CursorMoved { .. } => {
                if self.winDragActive {
                    if let (Some((sx, sy)), Some((wx, wy)), Some(cur)) =
                        (self.winDragStartCursor, self.winDragStartPos,
                         crate::musicPlayerWindow::cursorScreenGlobal())
                    {
                        let dx = cur.0 as f64 - sx;
                        let dy = cur.1 as f64 - sy;
                        if dx.abs() > 3.0 || dy.abs() > 3.0 {
                            let nx = wx as f64 + dx;
                            let ny = wy as f64 + dy;
                            let _ = self.window.set_outer_position(
                                winit::dpi::PhysicalPosition::new(nx, ny),
                            );
                            // 更新锚点避免漂移，同步 config 位置
                            self.winDragStartCursor = Some((cur.0 as f64, cur.1 as f64));
                            if let Ok(pos) = self.window.outer_position() {
                                self.winDragStartPos = Some((pos.x, pos.y));
                                self.wheelCfg.windowPosX = pos.x as f32;
                                self.wheelCfg.windowPosY = pos.y as f32;
                            }
                        }
                    }
                }
                let resp = self.egui.on_window_event(&self.window, event);
                if resp.repaint {
                    self.needsRedraw = true;
                }
                return resp.consumed;
            }
            _ => {}
        }

        match event {
            WindowEvent::Resized(size) => {
                let maxDim = self.device.limits().max_texture_dimension_2d;
                self.config.width = size.width.clamp(1, maxDim);
                self.config.height = size.height.clamp(1, maxDim);
                self.surface.configure(&self.device, &self.config);
                self.needsRedraw = true;
                false
            }
            WindowEvent::CloseRequested => {
                self.pendingClose = true;
                true
            }
            WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        physical_key:
                            winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Escape),
                        state: winit::event::ElementState::Pressed,
                        ..
                    },
                ..
            } => {
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

    pub fn wantsOpenCfg(&self) -> bool {
        self.pendingOpenCfg
    }

    pub fn takeOpenCfg(&mut self) -> bool {
        let v = self.pendingOpenCfg;
        self.pendingOpenCfg = false;
        v
    }

    /// 实时更新布局配置（由 cfg 窗口调用）。
    pub fn setConfig(&mut self, cfg: &WheelConfig) {
        let oldW = self.wheelCfg.windowWidth.max(300.0);
        let oldH = self.wheelCfg.windowHeight.max(400.0);
        let oldX = self.wheelCfg.windowPosX;
        let oldY = self.wheelCfg.windowPosY;
        self.wheelCfg = cfg.clone();
        let newW = cfg.windowWidth.max(300.0);
        let newH = cfg.windowHeight.max(400.0);
        if (newW - oldW).abs() > 0.5 || (newH - oldH).abs() > 0.5 {
            let _ = self.window.request_inner_size(LogicalSize::new(newW, newH));
        }
        if (cfg.windowPosX - oldX).abs() > 0.5 || (cfg.windowPosY - oldY).abs() > 0.5 {
            self.setPos(cfg.windowPosX, cfg.windowPosY);
        }
        self.needsRedraw = true;
    }

    pub fn config(&self) -> &WheelConfig {
        &self.wheelCfg
    }

    pub fn setHighlight(&mut self, zone: Option<WheelHighlightZone>) {
        self.highlightZone = zone;
        self.needsRedraw = true;
    }

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

    pub fn requestRedraw(&self) {
        self.window.request_redraw();
    }

    /// 首帧加载 desktopPet/wheel/*.png 为 egui 纹理（缺图则回退代码绘制）。
    fn ensureTextures(&mut self) {
        if self.texturesLoaded {
            return;
        }
        self.texturesLoaded = true;
        log::info!("wheel: loading textures from {:?}", self.wheelDir);
        let ctx = self.egui.egui_ctx().clone();
        // coin.png
        if let Ok(bytes) = std::fs::read(self.wheelDir.join("coin.png")) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let color =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                self.coinTex =
                    Some(ctx.load_texture("wheel_coin", color, egui::TextureOptions::LINEAR));
                log::info!("wheel: loaded coin.png ({}x{})", w, h);
            }
        } else { log::warn!("wheel: coin.png not found"); }
        // pointer.png
        if let Ok(bytes) = std::fs::read(self.wheelDir.join("pointer.png")) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let color =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                self.pointerTex =
                    Some(ctx.load_texture("wheel_pointer", color, egui::TextureOptions::LINEAR));
                log::info!("wheel: loaded pointer.png ({}x{})", w, h);
            }
        } else { log::warn!("wheel: pointer.png not found"); }
        // exit.png
        if let Ok(bytes) = std::fs::read(self.wheelDir.join("exit.png")) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let color =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                self.closeTex =
                    Some(ctx.load_texture("wheel_exit", color, egui::TextureOptions::LINEAR));
                log::info!("wheel: loaded exit.png ({}x{})", w, h);
            }
        } else { log::warn!("wheel: exit.png not found"); }
        // btn_locked.png
        if let Ok(bytes) = std::fs::read(self.wheelDir.join("btn_locked.png")) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let color =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                self.lockedTex =
                    Some(ctx.load_texture("wheel_locked", color, egui::TextureOptions::LINEAR));
                log::info!("wheel: loaded btn_locked.png ({}x{})", w, h);
            }
        } else { log::warn!("wheel: btn_locked.png not found"); }
        // btn_press.png
        if let Ok(bytes) = std::fs::read(self.wheelDir.join("btn_press.png")) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let color =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                self.pressTex =
                    Some(ctx.load_texture("wheel_press", color, egui::TextureOptions::LINEAR));
                log::info!("wheel: loaded btn_press.png ({}x{})", w, h);
            }
        } else { log::warn!("wheel: btn_press.png not found"); }
        // icon_food.png
        if let Ok(bytes) = std::fs::read(self.wheelDir.join("icon_food.png")) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let color =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                self.foodIconTex =
                    Some(ctx.load_texture("wheel_food_icon", color, egui::TextureOptions::LINEAR));
                log::info!("wheel: loaded icon_food.png ({}x{})", w, h);
            }
        } else { log::warn!("wheel: icon_food.png not found"); }
        // icon_drink.png
        if let Ok(bytes) = std::fs::read(self.wheelDir.join("icon_drink.png")) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let color =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                self.drinkIconTex =
                    Some(ctx.load_texture("wheel_drink_icon", color, egui::TextureOptions::LINEAR));
                log::info!("wheel: loaded icon_drink.png ({}x{})", w, h);
            }
        } else { log::warn!("wheel: icon_drink.png not found"); }
        // icon_backpack.png
        if let Ok(bytes) = std::fs::read(self.wheelDir.join("icon_backpack.png")) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let color =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
                self.packIconTex =
                    Some(ctx.load_texture("wheel_pack_icon", color, egui::TextureOptions::LINEAR));
                log::info!("wheel: loaded icon_backpack.png ({}x{})", w, h);
            }
        } else { log::warn!("wheel: icon_backpack.png not found"); }
    }

    /// 跑一帧：更新动画 → 采输入 → run UI → 提交渲染。返回本帧改动。
    pub fn frame(&mut self, coinCount: u32) -> RewardWheelChange {
        self.ensureTextures();

        let now = std::time::Instant::now();
        let dt = (now - self.lastFrameTime).as_secs_f32().min(0.1);
        self.lastFrameTime = now;

        let mut change = RewardWheelChange::default();

        // ── 旋转动画更新 ──
        if self.spinning {
            self.rotation += self.spinVelocity * dt;
            self.spinVelocity *= 0.985;
            if self.spinVelocity < 0.05 {
                self.spinning = false;
                self.rewardAngle = self.rotation % std::f32::consts::TAU;
                // 停止即发放奖励，按钮回到 LOCKED
                let kind = angleToReward(self.rewardAngle);
                change.reward = Some(match kind {
                    RewardKind::Food => rewardFoodItem(),
                    RewardKind::Drink => rewardDrinkItem(),
                    RewardKind::Backpack => rewardBackpackItem(),
                });
                self.coinInserted = false;
            }
            self.needsRedraw = true;
        }

        // ── 按钮动画平滑 ──
        let canPress = self.coinInserted && !self.spinning;
        let targetScale = if canPress { self.wheelCfg.pressHoverScale } else { 1.0 };
        self.pressHoverScale += (targetScale - self.pressHoverScale) * 0.22;
        // 抖动衰减
        let shakeOffset = if let Some(t) = self.shakeStart {
            let elapsed = t.elapsed().as_secs_f32();
            if elapsed < 0.25 {
                (elapsed * 50.0).sin() * (0.25 - elapsed) * 24.0
            } else {
                self.shakeStart = None;
                0.0
            }
        } else {
            0.0
        };
        // 按压动画缩放（点击瞬间缩小，然后弹回）
        let pressScale = if let Some(t) = self.pressStart {
            let elapsed = t.elapsed().as_secs_f32();
            if elapsed < 0.24 {
                if elapsed < 0.13 {
                    // 0→0.13s 缩小到 0.85
                    let phase = elapsed / 0.13;
                    1.0 - 0.15 * phase
                } else {
                    // 0.13→0.24s 弹回 1.0
                    let phase = (elapsed - 0.13) / 0.11;
                    0.85 + 0.15 * phase
                }
            } else {
                self.pressStart = None;
                1.0
            }
        } else {
            1.0
        };

        let raw = self.egui.take_egui_input(&self.window);
        let rotation = self.rotation;
        let spinning = self.spinning;

        let full = self.egui.egui_ctx().clone().run(raw, |ctx| {
            drawWheelUi(
                ctx,
                &self.wheelCfg,
                rotation,
                spinning,
                &mut self.coinInserted,
                coinCount,
                &mut self.draggingToken,
                &mut self.dragHoverSlot,
                &mut change,
                &mut self.pendingClose,
                &mut self.pendingOpenCfg,
                self.coinTex.as_ref(),
                self.pointerTex.as_ref(),
                self.closeTex.as_ref(),
                self.lockedTex.as_ref(),
                self.pressTex.as_ref(),
                self.foodIconTex.as_ref(),
                self.drinkIconTex.as_ref(),
                self.packIconTex.as_ref(),
                self.pressHoverScale,
                pressScale,
                shakeOffset,
                &mut self.shakeStart,
                &mut self.pressStart,
                self.highlightZone,
            );
        });

        // ── 处理 UI 发出的信号 ──
        if change.spinRequested {
            // 不立即启动旋转，等按压动画播完（0.30s）
            self.pendingSpin = true;
            change.spinRequested = false;
        }
        // 按压动画结束后再开始旋转
        if self.pendingSpin && self.pressStart.is_none() {
            self.spinning = true;
            self.spinVelocity = 8.0 + crate::behavior::rand01() * 7.0; // 8~15 rad/s
            self.pendingSpin = false;
            self.needsRedraw = true;
        }

        self.needsRedraw = full
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay.is_zero())
            .unwrap_or(false)
            || self.spinning;

        self.egui
            .handle_platform_output(&self.window, full.platform_output);

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("wheel surface frame: {e:?}");
                self.surface.configure(&self.device, &self.config);
                return change;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("wheel-encoder"),
                });
        for (id, delta) in &full.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let pixelsPerPoint = full.pixels_per_point;
        let primitives = self
            .egui
            .egui_ctx()
            .tessellate(full.shapes, pixelsPerPoint);
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: pixelsPerPoint,
        };
        self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &primitives,
            &screen,
        );
        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("wheel-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 0.0,
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
        self.lastPresentAt = std::time::Instant::now();
        let _ = &self.instance;
        change
    }
}

#[cfg(windows)]
fn applyWheelAttrs(
    attrs: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    attrs.with_skip_taskbar(true).with_drag_and_drop(false)
}
#[cfg(not(windows))]
fn applyWheelAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs
}

/// 将窗口坐标 clamp 到任一显示器可见范围内（保留至少 80px 可见）。
fn clampToScreen(mut x: f32, mut y: f32, window: Arc<Window>) -> (f32, f32) {
    let minVisible = 80.0;
    if let Some(monitor) = window.current_monitor() {
        let sz = monitor.size();
        let pos = monitor.position();
        let (mw, mh) = (sz.width as f32, sz.height as f32);
        let (ml, mt) = (pos.x as f32, pos.y as f32);
        let mr = ml + mw;
        let mb = mt + mh;
        x = x.clamp(ml - mw + minVisible, mr - minVisible);
        y = y.clamp(mt - mh + minVisible, mb - minVisible);
    }
    (x, y)
}

// ── 字体 ──────────────────────────────────────────────────────────────────────

pub fn installWheelFonts(ctx: &Context, _uiFontFamily: &str) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(path) = findWheelCJKFont() {
        if let Ok(bytes) = std::fs::read(&path) {
            let key = "wheelCJK".to_string();
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
    ctx.set_fonts(fonts);
}

#[cfg(windows)]
fn findWheelCJKFont() -> Option<std::path::PathBuf> {
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
fn findWheelCJKFont() -> Option<std::path::PathBuf> {
    None
}

// ── 奖励判定 ──────────────────────────────────────────────────────────────────

fn angleToReward(rotation: f32) -> RewardKind {
    // 指针固定在顶部（12点钟方向 = 3π/2），需要计算指针指向的扇区
    let pointerDir = 3.0 * std::f32::consts::PI / 2.0;
    let mut a = (pointerDir - rotation) % std::f32::consts::TAU;
    if a < 0.0 {
        a += std::f32::consts::TAU;
    }
    let mut acc = 0.0f32;
    for (frac, _, kind) in SEGMENTS {
        acc += frac * std::f32::consts::TAU;
        if a < acc {
            return *kind;
        }
    }
    // 兜底（浮点精度）：返回最后一个
    SEGMENTS.last().map(|(_, _, k)| *k).unwrap_or(RewardKind::Food)
}

fn rewardFoodItem() -> Item {
    use crate::food::FoodKind;
    let foods: Vec<_> = crate::food::FOODS.iter().filter(|f| f.kind == FoodKind::Eat).collect();
    if foods.is_empty() { return Item::food("apple"); }
    let i = ((crate::behavior::rand01() * foods.len() as f32) as usize).min(foods.len() - 1);
    Item::food(foods[i].id)
}

fn rewardDrinkItem() -> Item {
    use crate::food::FoodKind;
    let drinks: Vec<_> = crate::food::FOODS.iter().filter(|f| f.kind == FoodKind::Drink).collect();
    if drinks.is_empty() { return Item::food("water"); }
    let i = ((crate::behavior::rand01() * drinks.len() as f32) as usize).min(drinks.len() - 1);
    Item::food(drinks[i].id)
}

fn rewardBackpackItem() -> Item {
    let bags = crate::item::allBackpackIds();
    let i = ((crate::behavior::rand01() * bags.len() as f32) as usize).min(bags.len() - 1);
    Item::backpack(bags[i])
}

// ── UI 绘制 ───────────────────────────────────────────────────────────────────

fn drawWheelUi(
    ctx: &Context,
    cfg: &WheelConfig,
    rotation: f32,
    spinning: bool,
    coinInserted: &mut bool,
    coinCount: u32,
    draggingToken: &mut bool,
    dragHoverSlot: &mut bool,
    change: &mut RewardWheelChange,
    pendingClose: &mut bool,
    pendingOpenCfg: &mut bool,
    coinTex: Option<&egui::TextureHandle>,
    pointerTex: Option<&egui::TextureHandle>,
    _closeTex: Option<&egui::TextureHandle>,
    lockedTex: Option<&egui::TextureHandle>,
    pressTex: Option<&egui::TextureHandle>,
    foodIconTex: Option<&egui::TextureHandle>,
    drinkIconTex: Option<&egui::TextureHandle>,
    packIconTex: Option<&egui::TextureHandle>,
    pressHoverScale: f32,
    pressScale: f32,
    shakeOffset: f32,
    shakeStart: &mut Option<std::time::Instant>,
    pressStart: &mut Option<std::time::Instant>,
    highlightZone: Option<WheelHighlightZone>,
) {
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *pendingClose = true;
        change.closed = true;
    }

    let bg = egui::Color32::from_rgb(cfg.bgColor[0], cfg.bgColor[1], cfg.bgColor[2]);
    let green = egui::Color32::from_rgb(cfg.accentColor[0], cfg.accentColor[1], cfg.accentColor[2]);
    let greenDim = egui::Color32::from_rgb(cfg.accentDimColor[0], cfg.accentDimColor[1], cfg.accentDimColor[2]);
    let textGreen = egui::Color32::from_rgb(cfg.textColor[0], cfg.textColor[1], cfg.textColor[2]);
    let textGreenDim = egui::Color32::from_rgb(cfg.textDimColor[0], cfg.textDimColor[1], cfg.textDimColor[2]);

    egui::CentralPanel::default()
        .frame(egui::Frame::none())
        .show(ctx, |ui| {
            let painter = ui.painter().clone();
            let fontId = egui::FontId::monospace(12.0);
            let fontSm = egui::FontId::monospace(10.0);
            let fontBtn = egui::FontId::monospace(13.0);

            let winW = cfg.windowWidth;
            let winH = cfg.windowHeight;
            // ── 背景 ──
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(winW, winH)),
                0.0,
                bg,
            );

            // ── CRT 扫描线 ──
            {
                let scanAlpha = cfg.scanlineAlpha;
                let spacing = cfg.scanlineSpacing;
                let lineColor = green.linear_multiply(scanAlpha);
                let mut y = 0.0f32;
                while y < winH {
                    painter.line_segment(
                        [egui::pos2(0.0, y), egui::pos2(winW, y)],
                        egui::Stroke::new(1.0_f32, lineColor),
                    );
                    y += spacing;
                }
            }

            // ── 辐射线背景（从转盘圆心向外）──
            let cx = cfg.wheelCenterX;
            let cy = cfg.wheelCenterY;
            let r = cfg.wheelOuterR;
            let maxDist = (winW.powi(2) + winH.powi(2)).sqrt();
            let radLineColor = greenDim.linear_multiply(0.25);
            for i in 0..72 {
                let a = (i as f32 / 72.0) * std::f32::consts::TAU;
                let (s, c) = (a.sin(), a.cos());
                painter.line_segment(
                    [
                        egui::pos2(cx + c * (r + 10.0), cy + s * (r + 10.0)),
                        egui::pos2(cx + c * maxDist, cy + s * maxDist),
                    ],
                    egui::Stroke::new(1.0_f32, radLineColor),
                );
            }

            // ── 右上角控制按钮 ──
            let btnSz = cfg.cornerBtnSz;
            let btnCut = cfg.cornerCutPx;
            let settingsBtnX = cfg.settingsBtnX;
            let settingsBtnY = cfg.settingsBtnY;
            let closeBtnX = cfg.closeBtnX;
            let closeBtnY = cfg.closeBtnY;

            // 设置按钮 (⚙) — 直接打开配置窗口
            {
                let r = egui::Rect::from_min_size(
                    egui::pos2(settingsBtnX, settingsBtnY),
                    egui::vec2(btnSz, btnSz),
                );
                let resp = ui.allocate_rect(r, egui::Sense::click());
                let hov = resp.hovered();
                let stk = if hov { green } else { greenDim };
                let fill = if hov { Some(green.linear_multiply(0.10)) } else { None };
                crate::musicPlayerWindow::drawCornerCutBorder(
                    &painter, r, btnCut, egui::Stroke::new(1.5_f32, stk), fill,
                );
                painter.text(
                    r.center(), egui::Align2::CENTER_CENTER, "⚙", fontBtn.clone(),
                    if hov { green } else { textGreenDim },
                );
                if resp.clicked() {
                    *pendingOpenCfg = true;
                }
            }

            // 关闭按钮 (✕)
            {
                let r = egui::Rect::from_min_size(
                    egui::pos2(closeBtnX, closeBtnY),
                    egui::vec2(btnSz, btnSz),
                );
                let resp = ui.allocate_rect(r, egui::Sense::click());
                let hov = resp.hovered();
                let warnColor = egui::Color32::from_rgb(255, 100, 80);
                let stk = if hov { warnColor } else { greenDim };
                let fill = if hov { Some(warnColor.linear_multiply(0.10)) } else { None };
                crate::musicPlayerWindow::drawCornerCutBorder(
                    &painter, r, btnCut, egui::Stroke::new(1.5_f32, stk), fill,
                );
                // 关闭图标：直接用 ❌ emoji
                painter.text(
                    r.center(), egui::Align2::CENTER_CENTER, "❌", fontBtn.clone(),
                    if hov { warnColor } else { textGreenDim },
                );
                if resp.clicked() {
                    *pendingClose = true;
                    change.closed = true;
                }
            }

            // ── 转盘环形扇区（仓库圆环风格，3 扇区 40/40/20）──
            let ringROuter = r;
            let ringRInner = cfg.wheelInnerR; // 内圈留空给中心按钮

            // 扇区底色（Food/Drink/Backpack 微妙区分）
            let colorFood = egui::Color32::from_rgb(10, 18, 14);
            let colorDrink = egui::Color32::from_rgb(10, 14, 20);
            let colorPack = egui::Color32::from_rgb(8, 14, 18);
            let colorDivider = greenDim.linear_multiply(0.6);

            let mut currentA = rotation; // 累积起始角
            for (_frac, label, kind) in SEGMENTS {
                let span = _frac * std::f32::consts::TAU;
                let a0 = currentA;
                let a1 = currentA + span;
                let fill = match kind {
                    RewardKind::Food => colorFood,
                    RewardKind::Drink => colorDrink,
                    RewardKind::Backpack => colorPack,
                };
                drawRingSegment(
                    &painter, cx, cy, ringRInner, ringROuter, a0, a1,
                    fill,
                    egui::Stroke::new(1.2_f32, colorDivider),
                );
                // 段位图标（扇区中心，旋转跟随）
                let midA = a0 + span / 2.0;
                let labelR = ringRInner + (ringROuter - ringRInner) * cfg.iconRadiusFrac;
                let labelPos = egui::pos2(
                    cx + midA.cos() * labelR,
                    cy + midA.sin() * labelR,
                );
                let iconSize = (ringROuter - ringRInner) * cfg.iconScale;
                let iconTex = match kind {
                    RewardKind::Food => foodIconTex,
                    RewardKind::Drink => drinkIconTex,
                    RewardKind::Backpack => packIconTex,
                };
                if let Some(tex) = iconTex {
                    egui::Image::new(tex).paint_at(
                        ui,
                        egui::Rect::from_center_size(labelPos, egui::vec2(iconSize, iconSize)),
                    );
                } else {
                    // 缺图回退文字标签
                    painter.text(
                        labelPos,
                        egui::Align2::CENTER_CENTER,
                        label,
                        fontSm.clone(),
                        textGreenDim,
                    );
                }
                currentA = a1;
            }

            // 内圈环线
            painter.circle_stroke(
                egui::pos2(cx, cy), ringRInner,
                egui::Stroke::new(1.5_f32, greenDim),
            );
            // 辉光外圈（3-pass glow）
            drawCircleGlow(&painter, cx, cy, ringROuter, cfg.glowRadius, cfg.glowAlpha, green);

            // ── 指针（顶部，用 pointer.png；缺图回退绿色三角）──
            let ptrCenter = egui::pos2(cfg.pointerX, cfg.pointerY);
            if let Some(tex) = pointerTex {
                egui::Image::new(tex).paint_at(
                    ui,
                    egui::Rect::from_center_size(
                        ptrCenter,
                        egui::vec2(cfg.pointerW, cfg.pointerH),
                    ),
                );
            } else {
                let ptrTip = egui::pos2(cx, cy - ringROuter + 6.0);
                let ptrLeft = egui::pos2(cx - 10.0, cy - ringROuter - 14.0);
                let ptrRight = egui::pos2(cx + 10.0, cy - ringROuter - 14.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![ptrTip, ptrLeft, ptrRight],
                    green,
                    egui::Stroke::new(1.0_f32, green),
                ));
            }

            // ── 中心按钮（仅两种状态：LOCKED / PRESS）──
            let pressCenter = egui::pos2(cx + shakeOffset, cy);
            let pressR = cfg.pressButtonR;
            let canPress = *coinInserted && !spinning;
            // 按钮显示尺寸：PRESS 状态应用 hover 缩放 + 按压缩放
            let btnDisplayR = if canPress { pressR * pressHoverScale * pressScale } else { pressR };

            if canPress {
                // PRESS 状态：用 btn_press.png，缺图回退圆形+文字
                if let Some(tex) = pressTex {
                    egui::Image::new(tex).paint_at(
                        ui,
                        egui::Rect::from_center_size(
                            pressCenter,
                            egui::vec2(btnDisplayR * 2.0, btnDisplayR * 2.0),
                        ),
                    );
                } else {
                    painter.circle_filled(pressCenter, btnDisplayR, bg);
                    drawCircleGlow(&painter, cx, cy, btnDisplayR, cfg.glowRadius, cfg.glowAlpha, green);
                    painter.text(
                        pressCenter,
                        egui::Align2::CENTER_CENTER,
                        "开始",
                        fontSm.clone(),
                        green,
                    );
                }
            } else {
                // LOCKED 状态：用 btn_locked.png，缺图回退圆形+文字
                if let Some(tex) = lockedTex {
                    egui::Image::new(tex).paint_at(
                        ui,
                        egui::Rect::from_center_size(
                            pressCenter,
                            egui::vec2(btnDisplayR * 2.0, btnDisplayR * 2.0),
                        ),
                    );
                } else {
                    painter.circle_filled(pressCenter, btnDisplayR, bg);
                    painter.circle_stroke(
                        pressCenter,
                        btnDisplayR,
                        egui::Stroke::new(1.5_f32, greenDim),
                    );
                    painter.text(
                        pressCenter,
                        egui::Align2::CENTER_CENTER,
                        "锁定",
                        fontSm.clone(),
                        textGreenDim,
                    );
                }
            }

            // 按钮交互区域（用实际 pressR，不受缩放影响，更好点）
            let pressRect = egui::Rect::from_center_size(
                egui::pos2(cx, cy), // 命中区不偏移
                egui::vec2(pressR * 2.5, pressR * 2.5),
            );
            let pressResp = ui.allocate_rect(pressRect, egui::Sense::click());

            if pressResp.clicked() {
                if canPress {
                    change.spinRequested = true;
                    *pressStart = Some(std::time::Instant::now());
                } else {
                    // 未投币 → 触发抖动
                    *shakeStart = Some(std::time::Instant::now());
                }
            }

            // ── Token Jar（左上 - 切角线框）──
            let jarX = cfg.jarX;
            let jarY = cfg.jarY;
            let jarW = cfg.jarW;
            let jarH = cfg.jarH;
            let jarRect =
                egui::Rect::from_min_size(egui::pos2(jarX, jarY), egui::vec2(jarW, jarH));

            crate::musicPlayerWindow::drawCornerCutBorder(
                &painter,
                jarRect,
                cfg.cornerCutPx,
                egui::Stroke::new(cfg.borderWidth, greenDim),
                Some(bg),
            );
            painter.text(
                egui::pos2(jarX + 8.0, jarY + 6.0),
                egui::Align2::LEFT_TOP,
                "金闪闪的硬币（可拖动哦~）",
                fontSm.clone(),
                textGreenDim,
            );

            // 代币图标：拖拽时隐藏在 jar 里（跟随光标显示），否则按配置显示
            let coinImgW = cfg.coinImgW;
            let coinImgH = cfg.coinImgH;
            let coinCenterX = cfg.coinImgX;
            let coinCenterY = cfg.coinImgY;
            if !*draggingToken {
                if let Some(tex) = coinTex {
                    egui::Image::new(tex).paint_at(
                        ui,
                        egui::Rect::from_center_size(
                            egui::pos2(coinCenterX, coinCenterY),
                            egui::vec2(coinImgW, coinImgH),
                        ),
                    );
                } else {
                    painter.circle_stroke(
                        egui::pos2(coinCenterX, coinCenterY),
                        12.0,
                        egui::Stroke::new(1.5_f32, green),
                    );
                    painter.text(
                        egui::pos2(coinCenterX, coinCenterY),
                        egui::Align2::CENTER_CENTER,
                        "◎",
                        egui::FontId::monospace(14.0),
                        green,
                    );
                }
            }

            let countText = format!("硬币 x {}", coinCount);
            painter.text(
                egui::pos2(jarX + jarW / 2.0, jarY + jarH - 12.0),
                egui::Align2::CENTER_CENTER,
                &countText,
                fontId.clone(),
                textGreen,
            );

            // ── 拖拽代币（仅当 coinCount > 0 时可拖拽）──
            let dragCoinRect = egui::Rect::from_center_size(
                egui::pos2(coinCenterX, coinCenterY),
                egui::vec2(coinImgW + 8.0, coinImgH + 8.0),
            );
            let canDrag = coinCount > 0 && !*coinInserted;
            let dragResp = ui.allocate_rect(dragCoinRect, egui::Sense::drag());
            if dragResp.drag_started() && canDrag {
                *draggingToken = true;
            }
            // drag_stopped: 优先判定是否成功放入 Insert 槽，再清除拖拽状态
            if dragResp.drag_stopped() {
                if *dragHoverSlot && !*coinInserted && coinCount > 0 {
                    *coinInserted = true;
                    change.coinSpent = true;
                }
                *draggingToken = false;
                *dragHoverSlot = false;
            }
            // ── Insert Token 槽（切角线框）──
            let slotX = cfg.slotX;
            let slotY = cfg.slotY;
            let slotW = cfg.slotW;
            let slotH = cfg.slotH;
            let slotRect =
                egui::Rect::from_min_size(egui::pos2(slotX, slotY), egui::vec2(slotW, slotH));

            if *draggingToken {
                if let Some(mpos) = ctx.input(|i| i.pointer.hover_pos()) {
                    *dragHoverSlot = slotRect.contains(mpos);
                }
            }

            let slotStrokeColor = if *coinInserted {
                greenDim
            } else if *dragHoverSlot {
                green
            } else {
                greenDim
            };
            let slotStrokeW = if *dragHoverSlot { cfg.borderWidth + 0.5 } else { cfg.borderWidth };
            crate::musicPlayerWindow::drawCornerCutBorder(
                &painter,
                slotRect,
                cfg.cornerCutPx,
                egui::Stroke::new(slotStrokeW, slotStrokeColor),
                Some(bg),
            );

            if *coinInserted {
                painter.text(
                    egui::pos2(slotX + slotW / 2.0, slotY + 36.0),
                    egui::Align2::CENTER_CENTER,
                    "已投入",
                    fontSm.clone(),
                    textGreenDim,
                );
            } else {
                painter.text(
                    egui::pos2(slotX + slotW / 2.0, slotY + 20.0),
                    egui::Align2::CENTER_CENTER,
                    "投放",
                    fontSm.clone(),
                    textGreen,
                );
                painter.text(
                    egui::pos2(slotX + slotW / 2.0, slotY + 42.0),
                    egui::Align2::CENTER_CENTER,
                    "硬币",
                    fontSm.clone(),
                    textGreen,
                );
                // 向下箭头
                painter.text(
                    egui::pos2(slotX + slotW / 2.0, slotY + 64.0),
                    egui::Align2::CENTER_CENTER,
                    "\u{25BC}",
                    fontSm.clone(),
                    textGreenDim,
                );
            }

            // 拖拽释放 → 投币（逻辑已合并到 dragResp.drag_stopped() 分支）

            // 拖拽中显示跟随光标的代币（渲染在 slot 之后，盖在 "INSERTED" 文字上面）
            if *draggingToken {
                if let Some(mpos) = ctx.input(|i| i.pointer.hover_pos()) {
                    if let Some(tex) = coinTex {
                        egui::Image::new(tex).paint_at(
                            ui,
                            egui::Rect::from_center_size(mpos, egui::vec2(coinImgW, coinImgH)),
                        );
                    }
                }
            }

            // ── 底部提示 ──
            let hintY = winH - 36.0;
            let hint = if spinning {
                "抽奖中..."
            } else if *coinInserted {
                "\u{25B6} 按下中心按钮开始抽奖！"
            } else if coinCount == 0 {
                "请从游戏中获取硬币"
            } else {
                "\u{2192} 将硬币拖入槽中以解锁"
            };
            painter.text(
                egui::pos2(winW / 2.0, hintY),
                egui::Align2::CENTER_CENTER,
                hint,
                fontSm.clone(),
                textGreenDim,
            );

            // ── 高亮区域（配置窗口 hover 反馈）──
            if let Some(zone) = highlightZone {
                let hlColor = green;
                let hlStroke = egui::Stroke::new(2.5_f32, hlColor);
                let hlFill = hlColor.linear_multiply(0.06);
                match zone {
                    WheelHighlightZone::Window => {
                        let r = egui::Rect::from_min_size(egui::pos2(2.0, 2.0), egui::vec2(winW - 4.0, winH - 4.0));
                        painter.rect_filled(r, 0.0, hlFill);
                        painter.rect_stroke(r, 0.0, hlStroke);
                    }
                    WheelHighlightZone::Appearance => {
                        // 外观影响全局视觉，高亮整个窗口边框
                        let r = egui::Rect::from_min_size(egui::pos2(4.0, 4.0), egui::vec2(winW - 8.0, winH - 8.0));
                        painter.rect_stroke(r, 4.0, egui::Stroke::new(3.0_f32, hlColor.linear_multiply(0.5)));
                    }
                }
            }

            // 持续请求重绘以支持动画 + 窗口拖动
            ctx.request_repaint();
        });
}

/// 画一个环形扇区（甜甜圈分段），用于转盘 8 段（仓库圆环同款风格）。
fn drawRingSegment(
    painter: &egui::Painter,
    cx: f32, cy: f32,
    rIn: f32, rOut: f32,
    a0: f32, a1: f32,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    const SEG: usize = 16;
    let mut pts = Vec::with_capacity(SEG * 2 + 2);
    for i in 0..=SEG {
        let t = a0 + (a1 - a0) * (i as f32 / SEG as f32);
        pts.push(egui::pos2(cx + rOut * t.cos(), cy + rOut * t.sin()));
    }
    for i in 0..=SEG {
        let t = a1 + (a0 - a1) * (i as f32 / SEG as f32);
        pts.push(egui::pos2(cx + rIn * t.cos(), cy + rIn * t.sin()));
    }
    painter.add(egui::Shape::Path(egui::epaint::PathShape {
        points: pts,
        closed: true,
        fill,
        stroke: stroke.into(),
    }));
}

/// 圆的 3-pass 辉光描边（对标 drawGlowBorder，但用于圆形）
fn drawCircleGlow(painter: &egui::Painter, cx: f32, cy: f32, r: f32, glowRadius: f32, glowAlpha: f32, baseColor: egui::Color32) {
    let passes: [(f32, f32); 3] = [
        (r + glowRadius, glowAlpha * 0.15),
        (r + glowRadius * 0.5, glowAlpha * 0.40),
        (r, 1.0),
    ];
    for (radius, alpha) in &passes {
        let c = if *alpha < 0.99 {
            baseColor.linear_multiply(*alpha)
        } else {
            baseColor
        };
        let w = if *alpha < 0.99 { 2.0_f32 } else { 1.5_f32 };
        painter.circle_stroke(
            egui::pos2(cx, cy),
            *radius,
            egui::Stroke::new(w, c),
        );
    }
}
