// 音乐播放器配置独立窗口：winit + wgpu + egui 0.30。
// CFG 按钮打开此窗口，hover 设置行时在播放器主窗口高亮对应 UI 区域。
#![allow(non_snake_case)]

use std::sync::Arc;

use anyhow::{anyhow, Result};
use egui::{Context, ViewportId};
use egui_wgpu::{wgpu, ScreenDescriptor};
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::musicPlayerWindow::{cursorScreenGlobal, installFonts, loadIcon};
use crate::settings::{HighlightZone, MusicPlayerStyle};

const WIN_W: f32 = 420.0;
const WIN_H: f32 = 580.0;

#[derive(Clone, Debug, Default)]
pub struct CfgChange {
    pub style: Option<MusicPlayerStyle>,
    pub hoveredZone: Option<HighlightZone>,
}

pub struct MusicPlayerCfgWindow {
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
    style: MusicPlayerStyle,
    styleDirty: bool,
    hoveredZone: Option<HighlightZone>,
    dragStarted: bool,
    dragging: bool,
    dragAnchor: Option<(winit::dpi::PhysicalPosition<i32>, (i32, i32))>,
}

impl MusicPlayerCfgWindow {
    pub fn create(el: &ActiveEventLoop, uiFontFamily: &str, style: &MusicPlayerStyle) -> Result<Self> {
        let mut attrs = Window::default_attributes()
            .with_title("Casualties Unknown：desktopPet · 外观设置")
            .with_inner_size(LogicalSize::new(WIN_W, WIN_H))
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(false);
        if let Some(icon) = loadIcon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        let window: Arc<Window> = Arc::new(
            el.create_window(attrs)
                .map_err(|e| anyhow!("musicPlayerCfg window create: {e}"))?,
        );
        window.set_min_inner_size(Some(LogicalSize::new(320.0, 400.0)));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| anyhow!("musicPlayerCfg surface: {e}"))?;
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .ok_or_else(|| anyhow!("musicPlayerCfg adapter not found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("musicPlayerCfg-device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| anyhow!("musicPlayerCfg device: {e}"))?;

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
        installFonts(&ctx, uiFontFamily);
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
            style: style.clone(),
            styleDirty: false,
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
        let style = &mut self.style;
        let dirty = &mut self.styleDirty;
        let hovered = &mut self.hoveredZone;
        let pendingClose = &mut self.pendingClose;
        let dragStarted = &mut self.dragStarted;
        let dragging = &mut self.dragging;

        let full = self.egui.egui_ctx().clone().run(raw, |ctx| {
            drawCfgUi(ctx, style, dirty, hovered, pendingClose, dragStarted, dragging);

            // 窗口拖动
            if *dragStarted {
                if let (Ok(pos), Some(cur)) = (self.window.outer_position(), cursorScreenGlobal()) {
                    self.dragAnchor = Some((pos, cur));
                }
            }
            if *dragging {
                if let (Some((startPos, startCur)), Some(cur)) = (self.dragAnchor, cursorScreenGlobal()) {
                    let nx = startPos.x + (cur.0 - startCur.0);
                    let ny = startPos.y + (cur.1 - startCur.1);
                    self.window
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
                        log::warn!("musicPlayerCfg surface frame: {e:?}");
                        return CfgChange {
                            style: None,
                            hoveredZone: self.hoveredZone,
                        };
                    }
                }
            }
            match f {
                Some(frame) => frame,
                None => {
                    log::warn!("musicPlayerCfg surface keeps returning Outdated");
                    return CfgChange {
                        style: None,
                        hoveredZone: self.hoveredZone,
                    };
                }
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("musicPlayerCfg-encoder"),
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
                    label: Some("musicPlayerCfg-pass"),
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

        let changed = if self.styleDirty {
            self.styleDirty = false;
            Some(self.style.clone())
        } else {
            None
        };

        CfgChange {
            style: changed,
            hoveredZone: self.hoveredZone,
        }
    }
}

// ── UI rendering ──────────────────────────────────────

fn drawCfgUi(
    ctx: &Context,
    style: &mut MusicPlayerStyle,
    dirty: &mut bool,
    hoveredZone: &mut Option<HighlightZone>,
    pendingClose: &mut bool,
    dragStarted: &mut bool,
    dragging: &mut bool,
) {
    let to_c32 = |c: &[f32; 3]| {
        egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
    };
    let accent = to_c32(&style.accentColor);
    let textColor = to_c32(&style.textColor);
    let bg = to_c32(&style.bgColor);

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

        // 重置本帧 hoverZone
        *hoveredZone = None;

        let cut = style.cornerCutPx;
        let bw = style.borderWidth;

        // 外框
        crate::musicPlayerWindow::drawCornerCutBorder(
            ui.painter(),
            contentRect.shrink(2.0),
            cut,
            egui::Stroke::new(bw, to_c32(&style.accentDimColor)),
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
                    egui::Stroke::new(1.0_f32, to_c32(&style.accentDimColor));
                ui.visuals_mut().widgets.hovered.fg_stroke = egui::Stroke::new(1.5_f32, accent);
                ui.visuals_mut().widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, accent);
                ui.visuals_mut().selection.bg_fill = accent.linear_multiply(0.3);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);

                // 标题区
                ui.label(
                    egui::RichText::new("外观设置")
                        .size(14.0)
                        .color(accent),
                );
                ui.add_space(4.0);

                // ── 设定 hover 跟踪的辅助宏 ──
                macro_rules! trackHover {
                    ($resp:expr, $zone:expr) => {
                        if $resp.hovered() {
                            *hoveredZone = Some($zone);
                        }
                    };
                }

                // ── 本地副本（仅颜色 + 边框相关字段） ──
                let mut bgC = style.bgColor;
                let mut accentC = style.accentColor;
                let mut accentDimC = style.accentDimColor;
                let mut textC = style.textColor;
                let mut textDimC = style.textDimColor;
                let mut warn = style.warnColor;
                let mut rowNormal = style.rowColorNormal;
                let mut rowCur = style.rowColorCurrent;
                let mut rowHov = style.rowColorHover;
                let mut progBg = style.progressBgColor;
                let mut waveform = style.waveformColor;
                let mut borderW = style.borderWidth;
                let mut cutPx = style.cornerCutPx;
                let mut glowR = style.glowRadius;
                let mut glowA = style.glowAlpha;
                let mut scanOn = style.scanlineEnabled;
                let mut scanA = style.scanlineAlpha;
                let mut scanSp = style.scanlineSpacing;

                // ── 颜色 ──
                let respColors = ui.collapsing(
                    egui::RichText::new("■ 颜色").color(accent),
                    |ui| {
                        for (c, label, zone) in [
                            (&mut bgC, "窗口背景最深色", HighlightZone::Colors),
                            (&mut accentC, "荧光绿强调色", HighlightZone::Colors),
                            (&mut accentDimC, "暗强调色", HighlightZone::Colors),
                            (&mut textC, "主文字浅绿色", HighlightZone::Colors),
                            (&mut textDimC, "次级文字暗绿色", HighlightZone::Colors),
                            (&mut warn, "警告/状态琥珀色", HighlightZone::Colors),
                            (&mut rowNormal, "歌单普通行底色", HighlightZone::Playlist),
                            (&mut rowCur, "歌单当前行底色", HighlightZone::Playlist),
                            (&mut rowHov, "歌单悬停行底色", HighlightZone::Playlist),
                            (&mut progBg, "进度条底色", HighlightZone::ProgressBar),
                            (&mut waveform, "波形占位图颜色", HighlightZone::TopPanel),
                        ] {
                            let resp = ui.label(
                                egui::RichText::new(label).color(textColor).size(11.0),
                            );
                            trackHover!(resp, zone);
                            let resp = ui.color_edit_button_rgb(c);
                            trackHover!(resp, zone);
                        }
                    },
                );
                trackHover!(respColors.header_response, HighlightZone::Colors);

                ui.add_space(4.0);

                // ── 边框 & 辉光 & CRT ──
                let respBorder = ui.collapsing(
                    egui::RichText::new("■ 边框 · 辉光 · 扫描线").color(accent),
                    |ui| {
                        for (val, lo, hi, step, label) in [
                            (&mut borderW, 1.0, 4.0, 0.5, "边框线宽"),
                            (&mut cutPx, 2.0, 16.0, 1.0, "切角大小"),
                            (&mut glowR, 0.0, 20.0, 1.0, "辉光半径"),
                            (&mut glowA, 0.0, 0.4, 0.01, "辉光透明度"),
                        ] {
                            let resp = ui.label(
                                egui::RichText::new(format!("{}: {:.1}", label, *val))
                                    .color(textColor)
                                    .size(11.0),
                            );
                            trackHover!(resp, HighlightZone::GlobalAppearance);
                            let resp = ui.add(egui::Slider::new(val, lo..=hi).step_by(step));
                            trackHover!(resp, HighlightZone::GlobalAppearance);
                        }
                        {
                            let resp = ui.checkbox(
                                &mut scanOn,
                                egui::RichText::new("启用 CRT 扫描线").color(textColor).size(11.0),
                            );
                            trackHover!(resp, HighlightZone::GlobalAppearance);
                        }
                        for (val, lo, hi, step, label) in [
                            (&mut scanA, 0.0, 0.15, 0.005, "扫描线透明度"),
                            (&mut scanSp, 1.0, 8.0, 1.0, "扫描线间距"),
                        ] {
                            let resp = ui.label(
                                egui::RichText::new(format!("{}: {:.3}", label, *val))
                                    .color(textColor)
                                    .size(11.0),
                            );
                            trackHover!(resp, HighlightZone::GlobalAppearance);
                            let resp = ui.add(egui::Slider::new(val, lo..=hi).step_by(step));
                            trackHover!(resp, HighlightZone::GlobalAppearance);
                        }
                    },
                );
                trackHover!(respBorder.header_response, HighlightZone::GlobalAppearance);

                // ── 将本地副本同步回 style（仅更新颜色和边框字段） ──
                let mut newStyle = style.clone();
                newStyle.bgColor = bgC;
                newStyle.accentColor = accentC;
                newStyle.accentDimColor = accentDimC;
                newStyle.textColor = textC;
                newStyle.textDimColor = textDimC;
                newStyle.warnColor = warn;
                newStyle.rowColorNormal = rowNormal;
                newStyle.rowColorCurrent = rowCur;
                newStyle.rowColorHover = rowHov;
                newStyle.progressBgColor = progBg;
                newStyle.waveformColor = waveform;
                newStyle.borderWidth = borderW;
                newStyle.cornerCutPx = cutPx;
                newStyle.glowRadius = glowR;
                newStyle.glowAlpha = glowA;
                newStyle.scanlineEnabled = scanOn;
                newStyle.scanlineAlpha = scanA;
                newStyle.scanlineSpacing = scanSp;
                if newStyle != *style {
                    *style = newStyle;
                    *dirty = true;
                }
            });

        // ── 底部按钮栏 ──
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal(|ui| {
            // RESET
            {
                let resp = ui.button(
                    egui::RichText::new("[ 重置 ]").size(12.0).color(accent),
                );
                if resp.clicked() {
                    *style = MusicPlayerStyle::default();
                    *dirty = true;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // CLOSE
                {
                    let resp = ui.button(
                        egui::RichText::new("[ 关闭 ]")
                            .size(12.0)
                            .color(to_c32(&style.warnColor)),
                    );
                    if resp.clicked() {
                        *pendingClose = true;
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
