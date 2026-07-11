// 转盘外观配置独立窗口：winit + wgpu + egui 0.30。
// 从转盘 ⚙ 按钮打开，调整窗口大小、颜色和视觉效果。
#![allow(non_snake_case)]

use std::sync::Arc;

use anyhow::{anyhow, Result};
use egui::{Context, ViewportId};
use egui_wgpu::{wgpu, ScreenDescriptor};
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::rewardWheel::installWheelFonts;
use crate::settings::{WheelConfig, WheelHighlightZone};

const WIN_W: f32 = 420.0;
const WIN_H: f32 = 600.0;

#[derive(Clone, Debug, Default)]
pub struct CfgChange {
    pub cfg: Option<WheelConfig>,
    pub hoveredZone: Option<WheelHighlightZone>,
    pub saveRequested: bool,
}

pub struct WheelCfgWindow {
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
    cfg: WheelConfig,
    cfgDirty: bool,
    hoveredZone: Option<WheelHighlightZone>,
    // 窗口拖动
    dragStarted: bool,
    dragging: bool,
    dragAnchor: Option<(winit::dpi::PhysicalPosition<i32>, (i32, i32))>,
}

impl WheelCfgWindow {
    pub fn create(el: &ActiveEventLoop, uiFontFamily: &str, cfg: &WheelConfig) -> Result<Self> {
        let attrs = Window::default_attributes()
            .with_title("Casualties Unknown：desktopPet · 转盘外观设置")
            .with_inner_size(LogicalSize::new(WIN_W, WIN_H))
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(false);
        let window: Arc<Window> = Arc::new(
            el.create_window(attrs)
                .map_err(|e| anyhow!("wheelCfg window create: {e}"))?,
        );
        window.set_min_inner_size(Some(LogicalSize::new(320.0, 400.0)));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| anyhow!("wheelCfg surface: {e}"))?;
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .ok_or_else(|| anyhow!("wheelCfg adapter not found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("wheelCfg-device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| anyhow!("wheelCfg device: {e}"))?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let maxDim = device.limits().max_texture_dimension_2d;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.clamp(1, maxDim),
            height: size.height.clamp(1, maxDim),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let ctx = Context::default();
        installWheelFonts(&ctx, uiFontFamily);
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
            cfg: cfg.clone(),
            cfgDirty: false,
            hoveredZone: None,
            dragStarted: false,
            dragging: false,
            dragAnchor: None,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

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

    pub fn wantsRedraw(&self) -> bool {
        self.needsRedraw
    }

    pub fn requestRedraw(&self) {
        self.window.request_redraw();
    }

    pub fn frame(&mut self) -> CfgChange {
        let raw = self.egui.take_egui_input(&self.window);
        let cfgBefore = self.cfg.clone();
        let cfg = &mut self.cfg;
        let dirty = &mut self.cfgDirty;
        let pendingClose = &mut self.pendingClose;
        let mut saveRequestedFlag = false;
        let saveRequested = &mut saveRequestedFlag;
        let dragStarted = &mut self.dragStarted;
        let dragging = &mut self.dragging;
        let hovered = &mut self.hoveredZone;

        let full = self.egui.egui_ctx().clone().run(raw, |ctx| {
            drawCfgUi(ctx, cfg, dirty, pendingClose, saveRequested, dragStarted, dragging, hovered);

            // 自动检测 slider 导致的变更
            if *cfg != cfgBefore {
                *dirty = true;
            }

            // 窗口拖动
            if *dragStarted {
                if let (Ok(pos), Some(cur)) =
                    (self.window.outer_position(), crate::musicPlayerWindow::cursorScreenGlobal())
                {
                    self.dragAnchor = Some((pos, cur));
                }
            }
            if *dragging {
                if let (Some((startPos, startCur)), Some(cur)) =
                    (self.dragAnchor, crate::musicPlayerWindow::cursorScreenGlobal())
                {
                    let nx = startPos.x + (cur.0 - startCur.0);
                    let ny = startPos.y + (cur.1 - startCur.1);
                    let _ = self
                        .window
                        .set_outer_position(winit::dpi::PhysicalPosition::new(nx, ny));
                }
            } else {
                self.dragAnchor = None;
            }
        });

        self.needsRedraw = full
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|v| v.repaint_delay.is_zero())
            .unwrap_or(false);
        self.egui
            .handle_platform_output(&self.window, full.platform_output);

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return CfgChange { cfg: None, ..Default::default() };
            }
            Err(e) => {
                log::warn!("wheelCfg surface frame: {e:?}");
                return CfgChange { cfg: None, ..Default::default() };
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("wheelCfg-encoder"),
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
        self.renderer
            .update_buffers(&self.device, &self.queue, &mut encoder, &primitives, &screen);
        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("wheelCfg-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.02,
                                g: 0.03,
                                b: 0.04,
                                a: 1.0,
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
        let _ = &self.instance;

        // 关闭时不返回 cfg（不保存），保存时才返回
        let changed = if self.cfgDirty && !self.pendingClose {
            self.cfgDirty = false;
            Some(self.cfg.clone())
        } else {
            None
        };

        CfgChange { cfg: changed, hoveredZone: self.hoveredZone, saveRequested: saveRequestedFlag }
    }
}

// ── UI rendering ──────────────────────────────────────

fn drawCfgUi(
    ctx: &Context,
    cfg: &mut WheelConfig,
    dirty: &mut bool,
    pendingClose: &mut bool,
    saveRequested: &mut bool,
    dragStarted: &mut bool,
    dragging: &mut bool,
    hoveredZone: &mut Option<WheelHighlightZone>,
) {
    let accent = egui::Color32::from_rgb(cfg.accentColor[0], cfg.accentColor[1], cfg.accentColor[2]);
    let bg = egui::Color32::from_rgb(cfg.bgColor[0], cfg.bgColor[1], cfg.bgColor[2]);
    let accentDim = egui::Color32::from_rgb(cfg.accentDimColor[0], cfg.accentDimColor[1], cfg.accentDimColor[2]);

    let frame = egui::Frame::central_panel(&ctx.style()).fill(bg);
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        ui.visuals_mut().window_rounding = egui::Rounding::ZERO;
        ui.visuals_mut().widgets.inactive.rounding = egui::Rounding::ZERO;
        ui.visuals_mut().widgets.hovered.rounding = egui::Rounding::ZERO;
        ui.visuals_mut().widgets.active.rounding = egui::Rounding::ZERO;

        // 背景拖拽
        *dragStarted = false;
        *dragging = false;
        let contentRect = ui.max_rect();
        let bgId = ui.next_auto_id();
        let bgResp = ui.interact(contentRect, bgId, egui::Sense::drag());
        if bgResp.drag_started_by(egui::PointerButton::Primary) {
            *dragStarted = true;
        }
        if bgResp.dragged_by(egui::PointerButton::Primary) {
            *dragging = true;
        }

        // 外框
        crate::musicPlayerWindow::drawCornerCutBorder(
            ui.painter(),
            contentRect.shrink(2.0),
            cfg.cornerCutPx,
            egui::Stroke::new(cfg.borderWidth, accentDim),
            None,
        );

        let inner = contentRect.shrink(8.0);

        egui::ScrollArea::vertical()
            .max_height(inner.height() - 50.0)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.set_min_width(inner.width() - 8.0);

                // Sci-Fi 视觉覆盖
                ui.visuals_mut().widgets.inactive.bg_fill = bg;
                ui.visuals_mut().widgets.hovered.bg_fill = accent.linear_multiply(0.15);
                ui.visuals_mut().widgets.active.bg_fill = accent.linear_multiply(0.25);
                ui.visuals_mut().widgets.inactive.fg_stroke =
                    egui::Stroke::new(1.0_f32, accentDim);
                ui.visuals_mut().widgets.hovered.fg_stroke = egui::Stroke::new(1.5_f32, accent);
                ui.visuals_mut().widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, accent);
                ui.visuals_mut().selection.bg_fill = accent.linear_multiply(0.3);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);

                // 重置本帧 hoverZone
                *hoveredZone = None;

                // 标题
                ui.label(
                    egui::RichText::new("转盘外观设置")
                        .size(14.0)
                        .color(accent),
                );
                ui.add_space(4.0);

                // ── 滑块辅助（含 hover 追踪）──
                fn slider(
                    ui: &mut egui::Ui,
                    val: &mut f32, lo: f32, hi: f32, step: f64, label: &str,
                    zone: WheelHighlightZone,
                    hoveredZone: &mut Option<WheelHighlightZone>,
                ) {
                    let resp = ui.label(
                        egui::RichText::new(format!("{}: {:.2}", label, *val))
                            .size(11.0),
                    );
                    if resp.hovered() {
                        *hoveredZone = Some(zone);
                    }
                    let resp = ui.add(egui::Slider::new(val, lo..=hi).step_by(step));
                    if resp.hovered() {
                        *hoveredZone = Some(zone);
                    }
                }

                fn colorSlider(
                    ui: &mut egui::Ui,
                    val: &mut u8, label: &str,
                    zone: WheelHighlightZone,
                    hoveredZone: &mut Option<WheelHighlightZone>,
                ) {
                    let mut v = *val as f32;
                    let resp = ui.label(
                        egui::RichText::new(format!("{}: {}", label, *val))
                            .size(10.0),
                    );
                    if resp.hovered() {
                        *hoveredZone = Some(zone);
                    }
                    let resp = ui.add(egui::Slider::new(&mut v, 0.0..=255.0).step_by(1.0));
                    if resp.hovered() {
                        *hoveredZone = Some(zone);
                    }
                    *val = v as u8;
                }

                // ── 分组标题 hover 辅助宏 ──
                macro_rules! trackHeader {
                    ($resp:expr, $zone:expr) => {
                        if $resp.hovered() {
                            *hoveredZone = Some($zone);
                        }
                    };
                }

                // ── 颜色 ──
                let respColor = ui.collapsing(
                    egui::RichText::new("■ 颜色").color(accent),
                    |ui| {
                        ui.label(egui::RichText::new("强调色").size(11.0).color(accent));
                        ui.horizontal(|ui| {
                            colorSlider(ui, &mut cfg.accentColor[0], "R", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.accentColor[1], "G", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.accentColor[2], "B", WheelHighlightZone::Appearance, hoveredZone);
                        });
                        ui.separator();
                        ui.label(egui::RichText::new("暗调强调色").size(11.0).color(accentDim));
                        ui.horizontal(|ui| {
                            colorSlider(ui, &mut cfg.accentDimColor[0], "R", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.accentDimColor[1], "G", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.accentDimColor[2], "B", WheelHighlightZone::Appearance, hoveredZone);
                        });
                        ui.separator();
                        ui.label(egui::RichText::new("文字色").size(11.0).color(egui::Color32::from_rgb(cfg.textColor[0], cfg.textColor[1], cfg.textColor[2])));
                        ui.horizontal(|ui| {
                            colorSlider(ui, &mut cfg.textColor[0], "R", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.textColor[1], "G", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.textColor[2], "B", WheelHighlightZone::Appearance, hoveredZone);
                        });
                        ui.separator();
                        ui.label(egui::RichText::new("暗调文字色").size(11.0).color(egui::Color32::from_rgb(cfg.textDimColor[0], cfg.textDimColor[1], cfg.textDimColor[2])));
                        ui.horizontal(|ui| {
                            colorSlider(ui, &mut cfg.textDimColor[0], "R", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.textDimColor[1], "G", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.textDimColor[2], "B", WheelHighlightZone::Appearance, hoveredZone);
                        });
                        ui.separator();
                        ui.label(egui::RichText::new("背景色").size(11.0).color(egui::Color32::from_rgb(cfg.bgColor[0], cfg.bgColor[1], cfg.bgColor[2])));
                        ui.horizontal(|ui| {
                            colorSlider(ui, &mut cfg.bgColor[0], "R", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.bgColor[1], "G", WheelHighlightZone::Appearance, hoveredZone);
                            colorSlider(ui, &mut cfg.bgColor[2], "B", WheelHighlightZone::Appearance, hoveredZone);
                        });
                    },
                );
                trackHeader!(respColor.header_response, WheelHighlightZone::Appearance);

                ui.add_space(4.0);

                // ── 外观 ──
                let respApp = ui.collapsing(
                    egui::RichText::new("■ 外观效果").color(accent),
                    |ui| {
                        slider(ui, &mut cfg.pressHoverScale, 1.0, 1.8, 0.02, "悬停放大倍率", WheelHighlightZone::Appearance, hoveredZone);
                        slider(ui, &mut cfg.glowAlpha, 0.0, 0.8, 0.01, "辉光透明度", WheelHighlightZone::Appearance, hoveredZone);
                        slider(ui, &mut cfg.glowRadius, 0.0, 30.0, 1.0, "辉光半径", WheelHighlightZone::Appearance, hoveredZone);
                        slider(ui, &mut cfg.borderWidth, 0.5, 4.0, 0.5, "边框线宽", WheelHighlightZone::Appearance, hoveredZone);
                        slider(ui, &mut cfg.cornerCutPx, 2.0, 16.0, 1.0, "切角大小", WheelHighlightZone::Appearance, hoveredZone);
                        slider(ui, &mut cfg.scanlineSpacing, 1.0, 8.0, 1.0, "扫描线间距", WheelHighlightZone::Appearance, hoveredZone);
                        slider(ui, &mut cfg.scanlineAlpha, 0.0, 0.15, 0.005, "扫描线透明度", WheelHighlightZone::Appearance, hoveredZone);
                    },
                );
                trackHeader!(respApp.header_response, WheelHighlightZone::Appearance);
            });

        // ── 底部按钮栏 ──
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal(|ui| {
            // RESET
            {
                let resp = ui.button(
                    egui::RichText::new("[ 重置默认 ]").size(12.0).color(accent),
                );
                if resp.clicked() {
                    *cfg = WheelConfig::default();
                    *dirty = true;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // CLOSE — 直接关闭不保存
                {
                    let resp = ui.button(
                        egui::RichText::new("[ 关闭 ]")
                            .size(12.0)
                            .color(egui::Color32::from_rgb(255, 100, 80)),
                    );
                    if resp.clicked() {
                        *pendingClose = true;
                    }
                }
                // SAVE — 保存但不关闭
                {
                    let resp = ui.button(
                        egui::RichText::new("[ 保存 ]")
                            .size(12.0)
                            .color(accent),
                    );
                    if resp.clicked() {
                        *dirty = true;
                        *saveRequested = true;
                    }
                }
            });
        });

        // ESC 关闭
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            *pendingClose = true;
        }

        ctx.request_repaint();
    });
}
