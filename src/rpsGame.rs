#![allow(non_snake_case)]

// ── RPS 独立窗口 ──────────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::{anyhow, Result};
use egui::{Context, FontData, TextureHandle, ViewportId};
use egui_wgpu::{wgpu, ScreenDescriptor};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

// 透明小窗：竖排 3 个图片按钮，足够大以容纳放大的按钮 + 小编辑器，空白处透明。
const WINDOW_W: u32 = 300;
const WINDOW_H: u32 = 560;

/// 3 个按钮的文件名（无扩展），顺序=竖排从上到下。
const BTN_FILES: [(&str, Hand); 3] = [
    ("rock", Hand::Rock),
    ("scissors", Hand::Scissors),
    ("paper", Hand::Paper),
];

/// 每帧由 `frame()` 返回；调用方据此判定胜负 / 气泡 / 奖励 / 关窗。
pub struct RpsChange {
    /// 本帧点击的出招（None=未出手）。胜负与奖励由 app 处理。
    pub play: Option<Hand>,
    /// true = 窗口已请求关闭。
    pub closed: bool,
}

impl Default for RpsChange {
    fn default() -> Self {
        Self { play: None, closed: false }
    }
}

/// 桌宠右侧的透明图片按钮窗：石头剪刀布。
pub struct RpsWindow {
    pub window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    pendingClose: bool,
    /// 仅当本帧需要重绘时才 present，避免阻塞式 Fifo present 拖慢主循环。
    needsRedraw: bool,
    /// 出招按钮图（rock/scissors/paper → egui 纹理），首帧懒加载。
    icons: HashMap<String, TextureHandle>,
    iconsLoaded: bool,
    btnDir: std::path::PathBuf,
    /// present 模式（Fifo 时需软节流，避免阻塞式 present 钉死主循环 → 桌宠动画卡）。
    presentMode: wgpu::PresentMode,
    lastPresentAt: std::time::Instant,
}

impl RpsWindow {
    /// 创建透明无边框置顶按钮窗。`btnDir` = desktopPet/rps（放 rock/scissors/paper.png）。
    pub fn create(el: &ActiveEventLoop, btnDir: &std::path::Path) -> Result<Self> {
        let attrs = Window::default_attributes()
            .with_title("rps-buttons")
            .with_inner_size(LogicalSize::new(WINDOW_W, WINDOW_H))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false);
        let attrs = applyRpsAttrs(attrs);
        let window: Arc<Window> = Arc::new(
            el.create_window(attrs)
                .map_err(|e| anyhow!("rps window create: {e}"))?,
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| anyhow!("rps surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow!("rps adapter not found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("rps-device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| anyhow!("rps device: {e}"))?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let maxDim = device.limits().max_texture_dimension_2d;
        // 非阻塞 Mailbox（不支持回退 Fifo），避免每帧阻塞式 present 钉死主循环 → 桌宠动画卡。
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
        installRpsFonts(&ctx);
        let egui = egui_winit::State::new(
            ctx,
            ViewportId::ROOT,
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
            icons: HashMap::new(),
            iconsLoaded: false,
            btnDir: btnDir.to_path_buf(),
            presentMode,
            lastPresentAt: std::time::Instant::now(),
        })
    }

    /// 首帧加载 rps/<file>.png 为 egui 纹理（缺图则该按钮回退文字）。
    fn ensureIcons(&mut self) {
        if self.iconsLoaded {
            return;
        }
        self.iconsLoaded = true;
        let ctx = self.egui.egui_ctx().clone();
        for (file, _) in BTN_FILES {
            let path = self.btnDir.join(format!("{file}.png"));
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let img = match image::load_from_memory(&bytes) {
                Ok(i) => i.to_rgba8(),
                Err(e) => {
                    log::warn!("rps btn decode {file} failed: {e:?}");
                    continue;
                }
            };
            let (w, h) = img.dimensions();
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
            let tex = ctx.load_texture(format!("rps_{file}"), color, egui::TextureOptions::LINEAR);
            self.icons.insert(file.to_string(), tex);
        }
    }

    /// 设置窗口左上角逻辑屏幕坐标（app 用于贴到桌宠右侧）。
    pub fn setPos(&self, lx: f32, ly: f32) {
        let _ = self.window.set_outer_position(LogicalPosition::new(lx, ly));
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// 处理窗口事件；ESC / CloseRequested 设 pendingClose，其余交 egui。
    pub fn handleEvent(&mut self, event: &WindowEvent) -> bool {
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
                event: winit::event::KeyEvent {
                    physical_key: winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Escape),
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

    /// 本帧是否需要重绘 present。
    pub fn wantsRedraw(&self) -> bool {
        if !self.needsRedraw {
            return false;
        }
        // Fifo 回退时软节流到 ~30fps，避免每帧阻塞式 present 钉死主循环。
        if self.presentMode == wgpu::PresentMode::Fifo
            && self.lastPresentAt.elapsed().as_millis() < 33
        {
            return false;
        }
        true
    }

    /// 跑一帧：采输入 → run UI → 提交渲染。返回本帧改动。
    pub fn frame(&mut self, cfg: &crate::settings::RpsConfig) -> RpsChange {
        self.ensureIcons();
        let raw = self.egui.take_egui_input(&self.window);
        let mut change = RpsChange::default();
        let pendingClose = &mut self.pendingClose;
        let icons = &self.icons;
        let full = self.egui.egui_ctx().clone().run(raw, |ctx| {
            drawRpsUi(ctx, icons, cfg, &mut change, pendingClose);
        });
        self.needsRedraw = full
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|v| v.repaint_delay.is_zero())
            .unwrap_or(false);
        self.egui.handle_platform_output(&self.window, full.platform_output);

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("rps surface frame: {e:?}");
                self.surface.configure(&self.device, &self.config);
                return change;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rps-encoder"),
        });
        for (id, delta) in &full.textures_delta.set {
            self.renderer.update_texture(&self.device, &self.queue, *id, delta);
        }
        let pixelsPerPoint = full.pixels_per_point;
        let primitives = self.egui.egui_ctx().tessellate(full.shapes, pixelsPerPoint);
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: pixelsPerPoint,
        };
        self.renderer.update_buffers(&self.device, &self.queue, &mut encoder, &primitives, &screen);
        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("rps-pass"),
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

    pub fn requestRedraw(&self) {
        self.window.request_redraw();
    }
}

#[cfg(windows)]
fn applyRpsAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    attrs.with_skip_taskbar(true).with_drag_and_drop(false)
}
#[cfg(not(windows))]
fn applyRpsAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs
}

/// 安装中文字体（否则按钮中文显示为方块）。
fn installRpsFonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(path) = findRpsCJKFont() {
        if let Ok(bytes) = std::fs::read(&path) {
            let key = "rpsCJK".to_string();
            fonts.font_data.insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
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
fn findRpsCJKFont() -> Option<std::path::PathBuf> {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];
    candidates.iter().map(std::path::PathBuf::from).find(|p| p.exists())
}

#[cfg(not(windows))]
fn findRpsCJKFont() -> Option<std::path::PathBuf> {
    None
}

/// 根据出招枚举返回中文名称（app 组装结果气泡用）。
pub fn handLabel(h: Hand) -> &'static str {
    match h {
        Hand::Rock => "石头",
        Hand::Scissors => "剪刀",
        Hand::Paper => "布",
    }
}

/// 出招回退文字（缺图标时显示）。
fn handText(h: Hand) -> &'static str {
    match h {
        Hand::Rock => "✊石头",
        Hand::Scissors => "✌剪刀",
        Hand::Paper => "✋布",
    }
}

/// RPS 按钮窗 UI：竖排 3 个纯图片按钮（缺图回退文字）。透明背景。参数已定死，无编辑器。
fn drawRpsUi(
    ctx: &Context,
    icons: &HashMap<String, TextureHandle>,
    cfg: &crate::settings::RpsConfig,
    change: &mut RpsChange,
    pendingClose: &mut bool,
) {
    // ESC 关闭。
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *pendingClose = true;
        change.closed = true;
    }
    let size = cfg.buttonSize.max(8.0);
    let gap = cfg.buttonGap.max(0.0);
    let hover_scale: f32 = 1.22;
    egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
        ui.vertical(|ui| {
            for (file, hand) in BTN_FILES {
                // 固定分配基座大小以保证布局稳定；hover 时图标在此区域内居中放大。
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
                let hovered = response.hovered();
                let draw_size = if hovered { size * hover_scale } else { size };
                let draw_rect =
                    egui::Rect::from_center_size(rect.center(), egui::vec2(draw_size, draw_size));

                if let Some(tex) = icons.get(file) {
                    ui.put(
                        draw_rect,
                        egui::Image::new(tex).fit_to_exact_size(egui::vec2(draw_size, draw_size)),
                    );
                } else {
                    ui.put(rect, egui::Button::new(handText(hand)));
                }
                if response.clicked() {
                    change.play = Some(hand);
                }
                ui.add_space(gap);
            }
        });
    });
}

/// 玩家出招枚举：石头 / 剪刀 / 布。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand { Rock, Scissors, Paper }

/// 游戏结果：玩家视角的胜 / 平 / 负。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome { Win, Draw, Lose }

/// 判断胜负：Stone>Scissors>Paper>Stone。
pub fn judge(player: Hand, computer: Hand) -> Outcome {
    use Hand::*;
    match (player, computer) {
        (Rock, Scissors) | (Scissors, Paper) | (Paper, Rock) => Outcome::Win,
        (a, b) if a == b => Outcome::Draw,
        _ => Outcome::Lose,
    }
}

/// 电脑随机出招（rand01 ∈ [0,1)）。
pub fn computerHand(rand01: f32) -> Hand {
    match (rand01 * 3.0) as u32 {
        0 => Hand::Rock,
        1 => Hand::Scissors,
        _ => Hand::Paper,
    }
}