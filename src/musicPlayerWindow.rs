// 音乐播放器独立 GUI 窗口：winit + wgpu + egui 0.30。
// 音量、播放顺序、打开文件夹、进度条四个功能。
#![allow(non_snake_case)]

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use egui::{Context, FontData, ViewportId};
use egui_wgpu::{wgpu, ScreenDescriptor};
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::music::{MusicPlayer, PlayMode};
use crate::settings::MusicPlayerStyle;

const MIN_W: u32 = 400;
const MIN_H: u32 = 300;

pub struct MusicPlayerWindow {
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
    musicDir: std::path::PathBuf,
    style: MusicPlayerStyle,
    styleDirty: bool,
    highlightZone: Option<crate::settings::HighlightZone>,
    pendingOpenCfg: bool,
    dragStarted: bool,
    dragging: bool,
    dragAnchor: Option<(winit::dpi::PhysicalPosition<i32>, (i32, i32))>,
    /// 上次 frame() 执行时刻，用于帧率节流（避免阻塞桌宠动画）。
    lastFrameAt: Option<std::time::Instant>,
}

impl MusicPlayerWindow {
    pub fn create(
        el: &ActiveEventLoop,
        musicDir: std::path::PathBuf,
        uiFontFamily: &str,
        style: &MusicPlayerStyle,
    ) -> Result<Self> {
        let win_w = style.windowWidth.max(MIN_W as f32);
        let win_h = style.windowHeight.max(MIN_H as f32);
        let mut attrs = Window::default_attributes()
            .with_title("Casualties Unknown：desktopPet · 音乐播放器")
            .with_inner_size(LogicalSize::new(win_w, win_h))
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(false);
        if let Some(icon) = loadIcon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        let window: Arc<Window> = Arc::new(
            el.create_window(attrs)
                .map_err(|e| anyhow!("musicPlayer window create: {e}"))?,
        );
        window.set_min_inner_size(Some(LogicalSize::new(win_w, win_h)));
        // 初始位置（>=0 时设置，<0 = 系统决定）
        if style.windowPosX >= 0.0 || style.windowPosY >= 0.0 {
            let _ = window.set_outer_position(winit::dpi::LogicalPosition::new(style.windowPosX, style.windowPosY));
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| anyhow!("musicPlayer surface: {e}"))?;
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .ok_or_else(|| anyhow!("musicPlayer adapter not found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("musicPlayer-device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| anyhow!("musicPlayer device: {e}"))?;

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
            musicDir,
            style: style.clone(),
            styleDirty: false,
            highlightZone: None,
            pendingOpenCfg: false,
            dragStarted: false,
            dragging: false,
            dragAnchor: None,
            lastFrameAt: None,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn setStyle(&mut self, style: &MusicPlayerStyle) {
        let oldW = self.style.windowWidth;
        let oldH = self.style.windowHeight;
        let oldX = self.style.windowPosX;
        let oldY = self.style.windowPosY;
        self.style = style.clone();
        // 动态调整窗口尺寸
        let newW = style.windowWidth.max(MIN_W as f32);
        let newH = style.windowHeight.max(MIN_H as f32);
        if (newW - oldW).abs() > 0.5 || (newH - oldH).abs() > 0.5 {
            let _ = self.window.request_inner_size(winit::dpi::LogicalSize::new(newW, newH));
        }
        // 动态调整窗口位置
        if (style.windowPosX >= 0.0 || style.windowPosY >= 0.0)
            && ((style.windowPosX - oldX).abs() > 0.5 || (style.windowPosY - oldY).abs() > 0.5)
        {
            let _ = self.window.set_outer_position(winit::dpi::LogicalPosition::new(style.windowPosX, style.windowPosY));
        }
    }

    /// 返回用户在播放器内修改的外观设置（仅在 dirty 时返回，消费 dirty 标记）。
    pub fn take_style_changes(&mut self) -> Option<MusicPlayerStyle> {
        if self.styleDirty {
            self.styleDirty = false;
            Some(self.style.clone())
        } else {
            None
        }
    }

    /// 设置高亮区域（由 CFG 窗口触发）。
    pub fn setHighlight(&mut self, zone: Option<crate::settings::HighlightZone>) {
        self.highlightZone = zone;
    }

    /// CFG 按钮被点击，请求打开配置窗口。
    pub fn wantsOpenCfg(&self) -> bool {
        self.pendingOpenCfg
    }

    /// 消费 CFG 打开请求。
    pub fn takeOpenCfg(&mut self) -> bool {
        let v = self.pendingOpenCfg;
        self.pendingOpenCfg = false;
        v
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

    pub fn frame(&mut self, player: &mut MusicPlayer, files: &[crate::music::MusicFile]) {
        // 帧率节流 ~30fps：避免密集 GPU 渲染阻塞桌宠主窗口动画。
        let now = std::time::Instant::now();
        if let Some(last) = self.lastFrameAt {
            if now.duration_since(last).as_secs_f32() < 0.033 {
                return;
            }
        }
        self.lastFrameAt = Some(now);
        let raw = self.egui.take_egui_input(&self.window);
        let musicDir = &self.musicDir;
        // 在 &mut self.style 借用前取出 clear color
        let bg = self.style.bgColor;
        let full = self.egui.egui_ctx().clone().run(raw, |ctx| {
            drawUi(
                ctx,
                player,
                musicDir,
                files,
                &mut self.style,
                &mut self.styleDirty,
                &mut self.pendingClose,
                &mut self.dragStarted,
                &mut self.dragging,
                &mut self.pendingOpenCfg,
                self.highlightZone,
            );
            // 窗口拖动：全局鼠标坐标锚定法（同 inventoryWindow 方案）
            if self.dragStarted {
                if let (Ok(pos), Some(cur)) = (self.window.outer_position(), cursorScreenGlobal()) {
                    self.dragAnchor = Some((pos, cur));
                }
            }
            if self.dragging {
                if let (Some((startPos, startCur)), Some(cur)) = (self.dragAnchor, cursorScreenGlobal()) {
                    let nx = startPos.x + (cur.0 - startCur.0);
                    let ny = startPos.y + (cur.1 - startCur.1);
                    self.window.set_outer_position(winit::dpi::PhysicalPosition::new(nx, ny));
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
                        log::warn!("musicPlayer surface frame: {e:?}");
                        return;
                    }
                }
            }
            match f {
                Some(frame) => frame,
                None => {
                    log::warn!("musicPlayer surface keeps returning Outdated");
                    return;
                }
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("musicPlayer-encoder"),
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
                    label: Some("musicPlayer-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: bg[0] as f64,
                                g: bg[1] as f64,
                                b: bg[2] as f64,
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
    }

    pub fn requestRedraw(&self) {
        self.window.request_redraw();
    }
}

// ── UI 绘制 ──────────────────────────────────────────

fn drawUi(
    ctx: &Context,
    player: &mut MusicPlayer,
    musicDir: &std::path::Path,
    files: &[crate::music::MusicFile],
    style: &mut MusicPlayerStyle,
    _styleDirty: &mut bool,
    pendingClose: &mut bool,
    dragStarted: &mut bool,
    dragging: &mut bool,
    pendingOpenCfg: &mut bool,
    highlightZone: Option<crate::settings::HighlightZone>,
) {
    // 辅助：线性 RGB → egui Color32
    let to_c32 = |c: &[f32; 3]| {
        egui::Color32::from_rgb(
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
        )
    };
    let accent = to_c32(&style.accentColor);
    let accentDim = to_c32(&style.accentDimColor);
    let textColor = to_c32(&style.textColor);
    let textDim = to_c32(&style.textDimColor);
    let warnColor = to_c32(&style.warnColor);
    let panelBg = to_c32(&style.bgColor);

    let frame = egui::Frame::central_panel(&ctx.style()).fill(panelBg);
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        // 关闭所有默认圆角
        let zero = egui::Rounding::ZERO;
        ui.visuals_mut().window_rounding = zero;
        ui.visuals_mut().widgets.inactive.rounding = zero;
        ui.visuals_mut().widgets.hovered.rounding = zero;
        ui.visuals_mut().widgets.active.rounding = zero;

        let contentRect = ui.max_rect();

        // ── 背景拖拽区：空白区域左键拖动 → 移动窗口 ──
        // 每帧先重置，由当帧 egui 交互重新置 true；防止旧状态残留导致 anchor 被反复覆盖。
        *dragStarted = false;
        *dragging = false;
        let bgId = ui.next_auto_id();
        let bgResp = ui.interact(contentRect, bgId, egui::Sense::drag());
        if bgResp.drag_started_by(egui::PointerButton::Primary) {
            *dragStarted = true;
        }
        if bgResp.dragged_by(egui::PointerButton::Primary) {
            *dragging = true;
        }

        let mut zoneRects: [Option<egui::Rect>; 7] = [None; 7];

        let cut = style.cornerCutPx;
        let bw = style.borderWidth;
        let btnSz = egui::vec2(style.transportButtonSize, style.transportButtonSize);
        let playSz = egui::vec2(style.transportButtonSize * style.playButtonScale, style.transportButtonSize * style.playButtonScale);

        // ── 外框辉光 + 切角边框 ──
        let frameRect = contentRect.shrink(4.0);
        if style.glowAlpha > 0.0 {
            drawGlowBorder(ui.painter(), frameRect, cut, accentDim, bw, style.glowRadius, style.glowAlpha);
        } else {
            drawCornerCutBorder(ui.painter(), frameRect, cut, egui::Stroke::new(bw, accentDim), None);
        }

        let innerRect = frameRect.shrink(style.glowRadius.max(6.0) + bw);
        let pad = 8.0;
        let availW = innerRect.width() - pad * 2.0;

        // ── Zone 1: 顶部面板 (左=封面波形, 右=曲目信息) ──
        let topH = style.topInfoHeight;
        let topW = if style.topInfoWidth > 0.0 { style.topInfoWidth } else { availW };
        let topLeftPad = match style.zoneAlignment {
            crate::settings::ZoneAlignment::Left => 0.0,
            crate::settings::ZoneAlignment::Center => (availW - topW) / 2.0,
            crate::settings::ZoneAlignment::Right => availW - topW,
        } + style.topPanelOffsetX;
        ui.add_space(style.topPanelOffsetY.max(0.0));
        let (_topId, topPanelRect) = ui.allocate_space(egui::vec2(availW, topH + 8.0));
        let topInner = egui::Rect::from_min_size(
            egui::pos2(topPanelRect.left() + topLeftPad, topPanelRect.top()),
            egui::vec2(topW, topH),
        );
        drawCornerCutBorder(ui.painter(), topInner, cut, egui::Stroke::new(bw, accentDim), None);
        zoneRects[0] = Some(topInner);

        let leftColW = if style.leftSectionWidth > 0.0 { style.leftSectionWidth } else { 0.0 };
        let artW = style.albumArtWidth;
        let artH = style.albumArtHeight;
        let (artRight, hasLeftCol) = if artW > 0.0 && artH > 0.0 {
            let artX = if leftColW > 0.0 {
                topInner.left() + pad + ((leftColW - artW) / 2.0).max(0.0) + style.albumArtOffsetX
            } else {
                topInner.left() + pad + style.albumArtOffsetX
            };
            let artRect = egui::Rect::from_min_size(
                egui::pos2(artX, topInner.center().y - artH / 2.0 + style.albumArtOffsetY),
                egui::vec2(artW, artH),
            );
            drawCornerCutBorder(ui.painter(), artRect, cut, egui::Stroke::new(bw, accentDim), Some(panelBg));
            let isPlaying = !player.is_paused() && player.current_track_info().is_some();
            drawWaveformPlaceholder(ui.painter(), artRect.shrink(3.0), to_c32(&style.waveformColor), textDim, isPlaying, player.elapsed());
            let right = if leftColW > 0.0 { topInner.left() + pad + leftColW } else { artRect.right() };
            (right, leftColW > 0.0)
        } else {
            let right = if leftColW > 0.0 { topInner.left() + pad + leftColW } else { topInner.left() + pad };
            (right, leftColW > 0.0)
        };
        // 左侧栏分割线
        if hasLeftCol {
            let divX = topInner.left() + pad + leftColW;
            ui.painter().line_segment(
                [egui::pos2(divX, topInner.top() + 4.0), egui::pos2(divX, topInner.bottom() - 4.0)],
                egui::Stroke::new(1.0_f32, accentDim.linear_multiply(0.5)),
            );
        }

        let infoX = artRight + 12.0;
        let infoCenterY = topInner.center().y;
        if let Some((idx, total, name)) = player.current_track_info() {
            let statusStr = if player.is_paused() { "STATUS: PAUSED" } else { "STATUS: PLAYING" };
            let statusColor = if player.is_paused() { warnColor } else { accent };
            ui.painter().text(
                egui::pos2(infoX, topInner.top() + pad + 6.0),
                egui::Align2::LEFT_TOP,
                "NOW PLAYING",
                egui::FontId::monospace(11.0),
                textDim,
            );
            ui.painter().text(
                egui::pos2(infoX, topInner.top() + pad + 24.0),
                egui::Align2::LEFT_TOP,
                name,
                egui::FontId::monospace(15.0),
                accent,
            );
            ui.painter().text(
                egui::pos2(infoX, infoCenterY + 2.0),
                egui::Align2::LEFT_CENTER,
                statusStr,
                egui::FontId::monospace(13.0),
                statusColor,
            );
            let elapsedStr = format_duration(player.elapsed());
            let totalStr = player.current_duration().map(format_duration).unwrap_or_else(|| "--:--".to_string());
            let timeText = format!("{} / {}", elapsedStr, totalStr);
            ui.painter().text(
                egui::pos2(infoX, infoCenterY + 20.0),
                egui::Align2::LEFT_CENTER,
                timeText,
                egui::FontId::monospace(12.0),
                textColor,
            );
            let idxText = format!("TRACK {:02}/{:02}", idx + 1, total);
            ui.painter().text(
                egui::pos2(topInner.right() - pad, topInner.top() + pad + 6.0),
                egui::Align2::RIGHT_TOP,
                idxText,
                egui::FontId::monospace(11.0),
                textDim,
            );
        } else {
            ui.painter().text(
                egui::pos2(infoX, infoCenterY),
                egui::Align2::LEFT_CENTER,
                "NO AUDIO — 将音乐文件放入 music/ 文件夹",
                egui::FontId::monospace(13.0),
                textDim,
            );
        }

        // ── Zone 1→2 分隔线 ──
        ui.add_space(style.zoneSpacing);
        ui.painter().line_segment(
            [egui::pos2(topInner.left(), topPanelRect.bottom()), egui::pos2(topInner.right(), topPanelRect.bottom())],
            egui::Stroke::new(1.0_f32, accentDim.linear_multiply(0.4)),
        );

        // ── Zone 2: 播放模式切换 ──
        let modeH = if style.modeBarHeight > 0.0 { style.modeBarHeight } else { style.modeButtonSize + 8.0 };
        let modeW = if style.modeBarWidth > 0.0 { style.modeBarWidth } else { availW };
        let modeLeftPad = match style.zoneAlignment {
            crate::settings::ZoneAlignment::Left => 0.0,
            crate::settings::ZoneAlignment::Center => (availW - modeW) / 2.0,
            crate::settings::ZoneAlignment::Right => availW - modeW,
        } + style.modeBarOffsetX;
        ui.add_space(style.modeBarOffsetY.max(0.0));
        let (_modeId, modeRect) = ui.allocate_space(egui::vec2(availW, modeH + 4.0));
        {
            let modeInner = egui::Rect::from_min_size(
                egui::pos2(modeRect.left() + modeLeftPad, modeRect.top()),
                egui::vec2(modeW, modeH),
            );
            zoneRects[1] = Some(modeInner);
            let mut modeUi = ui.new_child(egui::UiBuilder::new()
                .max_rect(modeInner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)));
            modeUi.add_space(4.0);
            modeUi.label(egui::RichText::new("模式:").size(11.0).color(textDim));

            let curMode = player.play_mode();
            let modes = [
                (PlayMode::Sequential, ("顺序", "播放")),
                (PlayMode::ListLoop, ("列表", "播放")),
                (PlayMode::SingleLoop, ("单曲", "循环")),
            ];
            let modeBtnSz = egui::vec2(style.modeButtonSize, modeH - 4.0);
            for (m, (line1, line2)) in &modes {
                let isActive = curMode == *m;
                let (r, resp) = modeUi.allocate_exact_size(modeBtnSz, egui::Sense::click());
                let fill = if isActive { accent.linear_multiply(0.15) } else { egui::Color32::TRANSPARENT };
                let strokeC = if isActive { accent } else { accentDim.linear_multiply(0.5) };
                drawCornerCutBorder(modeUi.painter(), r, cut * 0.6, egui::Stroke::new(bw, strokeC), Some(fill));
                let btnColor = if isActive { accent } else { textDim };
                modeUi.painter().text(egui::pos2(r.center().x, r.center().y - 4.0), egui::Align2::CENTER_BOTTOM, line1, egui::FontId::monospace(10.0), btnColor);
                modeUi.painter().text(egui::pos2(r.center().x, r.center().y + 4.0), egui::Align2::CENTER_TOP, line2, egui::FontId::monospace(10.0), btnColor);
                if resp.clicked() {
                    player.set_play_mode(*m);
                }
            }

            modeUi.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // EXIT — 关闭窗口
                let btnW = style.modeButtonSize;
                {
                    let sz = egui::vec2(btnW, modeBtnSz.y);
                    let (r, resp) = ui.allocate_exact_size(sz, egui::Sense::click());
                    let hov = resp.hovered();
                    let stk = if hov { warnColor } else { accentDim.linear_multiply(0.5) };
                    drawCornerCutBorder(ui.painter(), r, cut * 0.6, egui::Stroke::new(bw, stk), if hov { Some(warnColor.linear_multiply(0.10)) } else { None });
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "X", egui::FontId::monospace(14.0), if hov { warnColor } else { textDim });
                    if resp.clicked() {
                        *pendingClose = true;
                    }
                }
                // DIR — 打开音乐文件夹
                {
                    let sz = egui::vec2(btnW, modeBtnSz.y);
                    let (r, resp) = ui.allocate_exact_size(sz, egui::Sense::click());
                    let hov = resp.hovered();
                    let stk = if hov { accent } else { accentDim.linear_multiply(0.5) };
                    drawCornerCutBorder(ui.painter(), r, cut * 0.6, egui::Stroke::new(bw, stk), if hov { Some(accent.linear_multiply(0.10)) } else { None });
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "📁", egui::FontId::proportional(14.0), if hov { accent } else { textDim });
                    if resp.clicked() {
                        openMusicFolder(musicDir);
                    }
                }
                // CFG — 打开独立配置窗口
                {
                    let sz = egui::vec2(btnW, modeBtnSz.y);
                    let (r, resp) = ui.allocate_exact_size(sz, egui::Sense::click());
                    let hov = resp.hovered();
                    let stk = if hov { accent } else { accentDim.linear_multiply(0.5) };
                    let fill = if hov { Some(accent.linear_multiply(0.10)) } else { None };
                    drawCornerCutBorder(ui.painter(), r, cut * 0.6, egui::Stroke::new(bw, stk), fill);
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "外观", egui::FontId::monospace(9.0), if hov { accent } else { textDim });
                    if resp.clicked() {
                        *pendingOpenCfg = true;
                    }
                }
            });

            // ── Zone 2→3 分隔线 ──
            ui.add_space(style.zoneSpacing);
            ui.painter().line_segment(
                [egui::pos2(modeInner.left(), modeRect.bottom()), egui::pos2(modeInner.right(), modeRect.bottom())],
                egui::Stroke::new(1.0_f32, accentDim.linear_multiply(0.4)),
            );
        }

        // ── 主内容区 ──
        let statusH = style.statusBarHeight;

        // ── 公共布局变量（歌单 / 进度条共用）──
        let progH = style.progressBarHeight + 28.0;
        let ctrlH = style.bottomBarHeight + 4.0;
        let volH = if style.volumeSpeedHeight > 0.0 { style.volumeSpeedHeight } else { style.speedButtonSize + 6.0 };
        let bottomReserved = progH + ctrlH + volH + statusH + 28.0;
        let remainingH = (ui.available_height() - bottomReserved).max(60.0);
        let listMaxH = if style.playlistHeight > 0.0 { style.playlistHeight } else { remainingH };

        // Zone 3 宽度 & 对齐
        let listW = if style.playlistWidth > 0.0 { style.playlistWidth } else { availW };
        let listLeftPad = match style.zoneAlignment {
            crate::settings::ZoneAlignment::Left => 0.0,
            crate::settings::ZoneAlignment::Center => (availW - listW) / 2.0,
            crate::settings::ZoneAlignment::Right => availW - listW,
        } + style.playlistOffsetX;

        // ── Zone 3: 歌单 ──
        let order = player.order_snapshot().to_vec();
        let currentDisplay = player.current_track_info().map(|(i, _, _)| i);

        ui.add_space(style.playlistOffsetY.max(0.0));
        let _ = ui.allocate_space(egui::vec2(listLeftPad, 0.0));

        egui::ScrollArea::vertical()
            .max_height(listMaxH)
            .max_width(listW)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.set_min_width(listW);
                ui.add_space(2.0);
                // 仅在播放音乐时渲染歌曲行
                if !files.is_empty() {
                for (displayI, &fileI) in order.iter().enumerate() {
                    let isCurrent = currentDisplay == Some(displayI);
                    let rowH = style.playlistRowHeight;
                    let fontSz = rowH * 0.55;
                    let (rowId, rowRect) = ui.allocate_space(egui::vec2(ui.available_width(), rowH + 2.0));
                    let resp = ui.interact(rowRect, rowId, egui::Sense::click());
                    let hovered = resp.hovered();

                    if resp.clicked() {
                        player.play_from(files, displayI);
                    }

                    let rowBg = if isCurrent { to_c32(&style.rowColorCurrent) }
                    else if hovered { to_c32(&style.rowColorHover) }
                    else { to_c32(&style.rowColorNormal) };
                    let bgW = if style.playlistRowBgWidth > 0.0 { style.playlistRowBgWidth } else { rowRect.width() };
                    let innerR = egui::Rect::from_min_size(
                        egui::pos2(rowRect.min.x + style.playlistRowBgOffsetX, rowRect.min.y + style.playlistRowBgOffsetY),
                        egui::vec2(bgW, rowH),
                    );
                    ui.painter().rect_filled(innerR, 0.0, rowBg);

                    let isPlaying = isCurrent && !player.is_paused();
                    let prefix = if isPlaying { ">>" } else if isCurrent { "> " } else { "  " };
                    let num = format!("{:02}.", displayI + 1);
                    let name = files.get(fileI).map(|f| f.stem.as_str()).unwrap_or("?");
                    let labelText = format!("{} {} {}", prefix, num, name);

                    let mut rowUi = ui.new_child(
                        egui::UiBuilder::new().max_rect(innerR).layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    rowUi.add_space((6.0 + style.playlistRowOffsetX).max(0.0));
                    rowUi.add_space(style.playlistRowOffsetY.max(0.0));
                    let text = egui::RichText::new(labelText).size(fontSz);
                    let text = if isCurrent { text.color(accent) } else { text.color(textColor) };
                    rowUi.label(text);

                    ui.add_space(style.playlistRowSpacing);
                }
                } // if !files.is_empty()
                ui.add_space(2.0);
            });

        // Zone 3 区域矩形（用于高亮）
        {
            let z3Top = zoneRects[1].map(|r| r.bottom() + 4.0).unwrap_or(ui.next_widget_position().y);
            let z3Left = innerRect.left() + listLeftPad;
            let z3Right = z3Left + listW;
            zoneRects[2] = Some(egui::Rect::from_min_max(
                egui::pos2(z3Left, z3Top),
                egui::pos2(z3Right, ui.next_widget_position().y),
            ));
        }

        // ── 分隔线（歌单→进度条）──
        ui.add_space(style.zoneSpacing);
        ui.painter().line_segment(
            [egui::pos2(innerRect.left() + pad, ui.next_widget_position().y), egui::pos2(innerRect.right() - pad, ui.next_widget_position().y)],
            egui::Stroke::new(1.0_f32, accentDim.linear_multiply(0.4)),
        );

        // ── Zone 4: 分段进度条 ──
        {
            let progW = if style.progressBarWidth > 0.0 { style.progressBarWidth } else { availW };
            let progLeftPad = match style.zoneAlignment {
                crate::settings::ZoneAlignment::Left => 0.0,
                crate::settings::ZoneAlignment::Center => (availW - progW) / 2.0,
                crate::settings::ZoneAlignment::Right => availW - progW,
            } + style.progressBarOffsetX;
            ui.add_space(style.progressBarOffsetY.max(0.0));
            let (_pid, progRect) = ui.allocate_space(egui::vec2(availW, progH));
            let progInner = egui::Rect::from_min_size(
                egui::pos2(progRect.left() + progLeftPad, progRect.top()),
                egui::vec2(progW, progH),
            );
            zoneRects[3] = Some(progInner);
            let elapsed = player.elapsed();
            let totalDur = player.current_duration();
            let totalSecs = totalDur.map(|d| d.as_secs_f32()).unwrap_or(1.0);
            let fraction = if totalSecs > 0.0 { (elapsed.as_secs_f32() / totalSecs).clamp(0.0, 1.0) } else { 0.0 };

            let barH = style.progressBarHeight;
            let barRect = egui::Rect::from_min_size(egui::pos2(progInner.left(), progInner.top() + 2.0), egui::vec2(progInner.width(), barH));
            drawSegmentedProgress(ui.painter(), barRect, fraction, style.progressSegments, style.progressSegmentGap, accent, to_c32(&style.progressBgColor));

            // 拖动区域
            let dragRect = barRect.expand2(egui::vec2(0.0, 8.0));
            let resp = ui.interact(dragRect, ui.next_auto_id(), egui::Sense::click_and_drag());
            if resp.dragged() || resp.clicked() {
                if let Some(ptr) = resp.interact_pointer_pos() {
                    let seekFrac = ((ptr.x - barRect.left()) / barRect.width()).clamp(0.0, 1.0);
                    if let Some(dur) = totalDur {
                        player.seek(Duration::from_secs_f32(seekFrac * dur.as_secs_f32()));
                    }
                }
            }

            let elapsedStr = format_duration(elapsed);
            let totalStr = totalDur.map(format_duration).unwrap_or_else(|| "--:--".to_string());
            ui.painter().text(egui::pos2(barRect.left(), barRect.bottom() + 4.0), egui::Align2::LEFT_TOP, &elapsedStr, egui::FontId::monospace(11.0), accent);
            ui.painter().text(egui::pos2(barRect.right(), barRect.bottom() + 4.0), egui::Align2::RIGHT_TOP, &totalStr, egui::FontId::monospace(11.0), textDim);
        }

        // ── 分隔线 ──
        ui.add_space(style.zoneSpacing);
        ui.painter().line_segment(
            [egui::pos2(innerRect.left() + pad, ui.next_widget_position().y), egui::pos2(innerRect.right() - pad, ui.next_widget_position().y)],
            egui::Stroke::new(1.0_f32, accentDim.linear_multiply(0.4)),
        );

        // ── Zone 5: 控制栏（传输按钮居中）──
        {
            let tportW = if style.transportWidth > 0.0 { style.transportWidth } else { availW };
            let tportLeftPad = match style.zoneAlignment {
                crate::settings::ZoneAlignment::Left => 0.0,
                crate::settings::ZoneAlignment::Center => (availW - tportW) / 2.0,
                crate::settings::ZoneAlignment::Right => availW - tportW,
            } + style.transportOffsetX;
            ui.add_space(style.transportOffsetY.max(0.0));
            let (_cid, ctrlRect) = ui.allocate_space(egui::vec2(availW, style.bottomBarHeight + 2.0));
            let ctrlInner = egui::Rect::from_min_size(
                egui::pos2(ctrlRect.left() + tportLeftPad, ctrlRect.top()),
                egui::vec2(tportW, style.bottomBarHeight + 2.0),
            );
            zoneRects[4] = Some(ctrlInner);
            let transportW = btnSz.x * 2.0 + playSz.x + style.buttonGap * 2.0;
            let btnLeftPad = ((ctrlInner.width() - transportW) / 2.0).max(0.0);
            let mut ctrlUi = ui.new_child(egui::UiBuilder::new().max_rect(ctrlInner).layout(egui::Layout::left_to_right(egui::Align::Center)));
            ctrlUi.add_space(btnLeftPad);

            if drawTerminalButton(&mut ctrlUi, "|<", btnSz, accent, textColor, cut, bw).clicked() { player.prev_track(files); }
            ctrlUi.add_space(style.buttonGap);
            let playLabel = if player.is_paused() { ">" } else { "||" };
            if drawTerminalButton(&mut ctrlUi, playLabel, playSz, accent, textColor, cut, bw).clicked() { player.toggle_pause(); }
            ctrlUi.add_space(style.buttonGap);
            if drawTerminalButton(&mut ctrlUi, ">|", btnSz, accent, textColor, cut, bw).clicked() { player.next_track(files); }
        }

        // ── 分隔线 ──
        ui.add_space(style.zoneSpacing);
        ui.painter().line_segment(
            [egui::pos2(innerRect.left() + pad, ui.next_widget_position().y), egui::pos2(innerRect.right() - pad, ui.next_widget_position().y)],
            egui::Stroke::new(1.0_f32, accentDim.linear_multiply(0.4)),
        );

        // ── Zone 6: 音量 (分段) + 倍速 ──
        {
            let volH = if style.volumeSpeedHeight > 0.0 { style.volumeSpeedHeight } else { style.speedButtonSize + 6.0 };
            let rightW = if style.rightSectionWidth > 0.0 { style.rightSectionWidth } else { availW };
            let leftPad = match style.zoneAlignment {
                crate::settings::ZoneAlignment::Left => 0.0,
                crate::settings::ZoneAlignment::Center => (availW - rightW) / 2.0,
                crate::settings::ZoneAlignment::Right => availW - rightW,
            } + style.volumeSpeedOffsetX;
            ui.add_space(style.volumeSpeedOffsetY.max(0.0));
            let (_vid, volRect) = ui.allocate_space(egui::vec2(availW, volH));
            let volInner = egui::Rect::from_min_size(
                egui::pos2(volRect.left() + leftPad, volRect.top()),
                egui::vec2(rightW, volH),
            );
            zoneRects[5] = Some(volInner);
            // 内部居中 — 计算内容总宽度，使对齐/X偏移在窄内容时可见
            let volBarW = 140.0;
            let spdBtnSz = egui::vec2(style.speedButtonSize, volH - 4.0);
            let spdTotalW = style.speedPresets.len() as f32 * spdBtnSz.x;
            // 标签约宽: 音量 22 + volBarGap + 140 条 + volPctGap + 85% 32 + spdSectionGap + 倍速 22 + spdBtnGap + 按钮
            let contentW = 22.0 + style.volBarGap + volBarW + style.volPctGap + 32.0 + style.spdSectionGap + 22.0 + style.spdBtnGap + spdTotalW;
            let contentPad = ((volInner.width() - contentW) / 2.0).max(0.0);

            let volInnerShifted = volInner.translate(egui::vec2(style.volLabelOffsetX, style.volLabelOffsetY));
            let mut r2Ui = ui.new_child(egui::UiBuilder::new().max_rect(volInnerShifted).layout(egui::Layout::left_to_right(egui::Align::Center)));
            r2Ui.add_space(contentPad);

            r2Ui.label(egui::RichText::new("音量").size(11.0).color(textDim));
            r2Ui.add_space(style.volBarGap);

            // 分段音量条（allocate_exact_size 内联 Sense，保证 child UI 内可交互）
            let volBarH = 14.0;
            let vol = player.volume();
            let vsegments = 20u32;
            let (vbRect, vresp) = r2Ui.allocate_exact_size(egui::vec2(volBarW, volBarH), egui::Sense::click_and_drag());
            drawSegmentedProgress(r2Ui.painter(), vbRect, vol, vsegments, 2.0, accent, to_c32(&style.progressBgColor));
            if vresp.dragged() || vresp.clicked() {
                if let Some(ptr) = vresp.interact_pointer_pos() {
                    let vf = ((ptr.x - vbRect.left()) / vbRect.width()).clamp(0.0, 1.0);
                    player.set_volume(vf);
                }
            }

            r2Ui.add_space(style.volPctGap);
            r2Ui.label(egui::RichText::new(format!("{:3.0}%", vol * 100.0)).size(11.0).color(textDim));
            r2Ui.add_space(style.spdSectionGap);

            // 倍速标签 + 按钮
            r2Ui.label(egui::RichText::new("倍速").size(11.0).color(textDim));
            r2Ui.add_space(style.spdBtnGap);

            let curSpeed = player.speed();
            for &s in &style.speedPresets {
                let isActive = (curSpeed - s).abs() < 0.01;
                let (r, resp) = r2Ui.allocate_exact_size(spdBtnSz, egui::Sense::click());
                let fill = if isActive { accent.linear_multiply(0.15) } else { egui::Color32::TRANSPARENT };
                let stk = if isActive { accent } else { accentDim.linear_multiply(0.5) };
                drawCornerCutBorder(r2Ui.painter(), r, cut * 0.5, egui::Stroke::new(bw, stk), Some(fill));
                r2Ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, &format!("{:.1}x", s), egui::FontId::monospace(10.0), if isActive { accent } else { textDim });
                if resp.clicked() { player.set_speed(s); }
            }
        }

        // ── 分隔线 → 状态栏 ──
        ui.add_space(style.zoneSpacing);
        ui.painter().line_segment(
            [egui::pos2(innerRect.left() + pad, ui.next_widget_position().y), egui::pos2(innerRect.right() - pad, ui.next_widget_position().y)],
            egui::Stroke::new(1.0_f32, accentDim.linear_multiply(0.4)),
        );

        // ── Zone 7: 状态栏 ──
        let statusW = if style.statusBarWidth > 0.0 { style.statusBarWidth } else { availW };
        let statusLeftPad = match style.zoneAlignment {
            crate::settings::ZoneAlignment::Left => 0.0,
            crate::settings::ZoneAlignment::Center => (availW - statusW) / 2.0,
            crate::settings::ZoneAlignment::Right => availW - statusW,
        } + style.statusBarOffsetX;
        ui.add_space(style.statusBarOffsetY.max(0.0));
        let (_sid, statusRect) = ui.allocate_space(egui::vec2(availW, statusH));
        let statusInner = egui::Rect::from_min_size(
            egui::pos2(statusRect.left() + statusLeftPad, statusRect.top()),
            egui::vec2(statusW, statusH),
        );
        zoneRects[6] = Some(statusInner);
        drawCornerCutBorder(ui.painter(), statusInner, cut * 0.5, egui::Stroke::new(bw, accentDim), None);

        let modeLabel = match player.play_mode() {
            PlayMode::Sequential => "顺序播放",
            PlayMode::ListLoop => "列表播放",
            PlayMode::SingleLoop => "单曲循环",
        };
        let trackInfo = player
            .current_track_info()
            .map(|(idx, total, _)| format!("TRACK:{:02}/{:02}", idx + 1, total))
            .unwrap_or_else(|| "TRACK:--/--".to_string());
        let statusText = format!("[模式:{}]  [{}]  [系统:正常]", modeLabel, trackInfo);
        ui.painter().text(statusInner.center(), egui::Align2::CENTER_CENTER, &statusText, egui::FontId::monospace(10.0), textDim);

        // ── 高亮覆盖：CFG 窗口 hover 设置时，对应 zone 以半透明 accent 色填充 ──
        if let Some(zone) = highlightZone {
            match zone {
                crate::settings::HighlightZone::GlobalAppearance | crate::settings::HighlightZone::Colors => {
                    ui.painter().rect_filled(contentRect, 0.0, accent.linear_multiply(0.08));
                }
                _ => {
                    let zoneIdx = match zone {
                        crate::settings::HighlightZone::TopPanel => 0,
                        crate::settings::HighlightZone::ModeBar => 1,
                        crate::settings::HighlightZone::Playlist => 2,
                        crate::settings::HighlightZone::ProgressBar => 3,
                        crate::settings::HighlightZone::TransportControls => 4,
                        crate::settings::HighlightZone::VolumeSpeed => 5,
                        crate::settings::HighlightZone::StatusBar => 6,
                        _ => 0,
                    };
                    // Zone 3 未显式记录用 Zone 1→4 之间区域近似
                    if zoneIdx == 2 && zoneRects[2].is_none() {
                        if let (Some(z1b), Some(z4t)) = (zoneRects[0].map(|r| r.bottom()), zoneRects[3].map(|r| r.top())) {
                            let z3Left = if listLeftPad > 0.0 { innerRect.left() + listLeftPad } else { innerRect.left() };
                            let z3Right = if listW < availW { z3Left + listW } else { innerRect.right() };
                            zoneRects[2] = Some(egui::Rect::from_min_max(
                                egui::pos2(z3Left, z1b + 4.0),
                                egui::pos2(z3Right, z4t - 4.0),
                            ));
                        }
                    }
                    if let Some(zr) = zoneRects[zoneIdx] {
                        ui.painter().rect_filled(zr.expand(2.0), 0.0, accent.linear_multiply(0.08));
                        drawCornerCutBorder(ui.painter(), zr.expand(2.0), cut, egui::Stroke::new(1.5_f32, accent.linear_multiply(0.5)), None);
                    }
                }
            }
        }

        // ── CRT 扫描线 ──
        if style.scanlineEnabled {
            drawScanlines(ui.painter(), contentRect, accent, style.scanlineAlpha, style.scanlineSpacing);
        }

        // ESC 关闭窗口
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            *pendingClose = true;
        }

        player.sync_mode_if_needed(files);
        player.restart_if_loop_finished(files);
        ctx.request_repaint();
    });
}

fn format_duration(d: Duration) -> String {
    let total = d.as_secs();
    let mins = total / 60;
    let secs = total % 60;
    format!("{}:{:02}", mins, secs)
}

// ── Sci-Fi HUD 自定义绘制函数 ────────────────────────

/// 切角矩形边框：8 顶点多边形，每个角切掉 cornerCut 像素。
pub(crate) fn drawCornerCutBorder(
    painter: &egui::Painter,
    rect: egui::Rect,
    cut: f32,
    stroke: egui::Stroke,
    fill: Option<egui::Color32>,
) {
    let l = rect.left();
    let r = rect.right();
    let t = rect.top();
    let b = rect.bottom();
    let c = cut.min((r - l) / 2.0).min((b - t) / 2.0);
    let points = vec![
        egui::pos2(l + c, t),
        egui::pos2(r - c, t),
        egui::pos2(r, t + c),
        egui::pos2(r, b - c),
        egui::pos2(r - c, b),
        egui::pos2(l + c, b),
        egui::pos2(l, b - c),
        egui::pos2(l, t + c),
    ];
    let shape = egui::Shape::Path(egui::epaint::PathShape {
        points,
        closed: true,
        fill: fill.unwrap_or(egui::Color32::TRANSPARENT),
        stroke: stroke.into(),
    });
    painter.add(shape);
}

/// 伪辉光边框：用递减 alpha + 递增线宽绘制 3 层切角边框。
fn drawGlowBorder(
    painter: &egui::Painter,
    rect: egui::Rect,
    cut: f32,
    baseColor: egui::Color32,
    baseWidth: f32,
    glowRadius: f32,
    glowAlpha: f32,
) {
    // 3-pass: 外层 → 中层 → 核心
    let passes: [(f32, f32); 3] = [
        (baseWidth + glowRadius, glowAlpha * 0.15),
        (baseWidth + glowRadius * 0.5, glowAlpha * 0.40),
        (baseWidth, 1.0),
    ];
    for (width, alpha) in &passes {
        let mut c = baseColor;
        // 对 glow 层降低 alpha，核心层保持全不透明
        if *alpha < 0.99 {
            c = c.linear_multiply(*alpha);
        }
        drawCornerCutBorder(painter, rect, cut, egui::Stroke::new(*width, c), None);
    }
}

/// CRT 扫描线：从 top 到 bottom 每 spacing px 画一条半透明水平线。
fn drawScanlines(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    alpha: f32,
    spacing: f32,
) {
    if alpha <= 0.0 || spacing < 1.0 {
        return;
    }
    let lineColor = color.linear_multiply(alpha);
    let mut y = rect.top();
    let bottom = rect.bottom();
    while y < bottom {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), y),
                egui::pos2(rect.right(), (y + 1.0).min(bottom)),
            ),
            0.0,
            lineColor,
        );
        y += spacing;
    }
}

/// 终端风格按钮：切角边框 + hover/active 填充反色。
fn drawTerminalButton(
    ui: &mut egui::Ui,
    label: &str,
    size: egui::Vec2,
    accent: egui::Color32,
    textColor: egui::Color32,
    cut: f32,
    borderWidth: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let hovered = resp.hovered();
    let active = resp.is_pointer_button_down_on();

    let fill = if active {
        // 按下：accent 半透明填充
        accent.linear_multiply(0.25)
    } else if hovered {
        accent.linear_multiply(0.10)
    } else {
        egui::Color32::TRANSPARENT
    };

    let strokeColor = if hovered || active { accent } else { accent.linear_multiply(0.5) };

    drawCornerCutBorder(
        ui.painter(),
        rect,
        cut,
        egui::Stroke::new(borderWidth, strokeColor),
        Some(fill),
    );

    let labelColor = if active {
        accent
    } else if hovered {
        accent
    } else {
        textColor
    };

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(size.y * 0.4),
        labelColor,
    );

    resp
}

/// 分段块状进度条（复古血条风格）。
fn drawSegmentedProgress(
    painter: &egui::Painter,
    rect: egui::Rect,
    fraction: f32,
    segments: u32,
    gap: f32,
    filledColor: egui::Color32,
    emptyColor: egui::Color32,
) {
    if segments == 0 {
        return;
    }
    let totalW = rect.width();
    let segW = if segments > 1 {
        (totalW - gap * (segments - 1) as f32) / segments as f32
    } else {
        totalW
    };
    let h = rect.height();
    let filledCount = (fraction.clamp(0.0, 1.0) * segments as f32).round() as u32;

    for i in 0..segments {
        let x = rect.left() + i as f32 * (segW + gap);
        let color = if i < filledCount { filledColor } else { emptyColor };
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(segW, h)),
            0.0,
            color,
        );
    }
}

/// 波形占位图：播放时动画频谱 + "SIGNAL ACTIVE"，暂停/无音源时静态 + "NO SIGNAL"。
fn drawWaveformPlaceholder(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    dimColor: egui::Color32,
    is_playing: bool,
    elapsed: std::time::Duration,
) {
    // 深色背景
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(5, 7, 8));

    // 合成竖条（~30 根）
    let barCount = 30;
    let barW = (rect.width() / barCount as f32) * 0.7;
    let gapW = (rect.width() / barCount as f32) * 0.3;
    let maxH = rect.height() * 0.55;
    let baseY = rect.center().y;

    use std::f32::consts::TAU;
    // 动画相位：播放时随时间变化，暂停时固定
    let timeOffset = if is_playing {
        elapsed.as_secs_f32() * 2.0
    } else {
        0.0
    };

    for i in 0..barCount {
        let basePhase = i as f32 / barCount as f32 * TAU * 3.0;
        let phase = basePhase + timeOffset;
        let h = ((phase.sin() * 0.6 + (phase * 2.1).sin() * 0.3 + (phase * 5.3).sin() * 0.1)
            .abs()
            * maxH)
            .max(4.0);
        let x = rect.left() + i as f32 * (barW + gapW);
        // 播放时使用 accent 色，暂停时使用 dimColor
        let barColor = if is_playing { color } else { dimColor };
        painter.rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(x + barW / 2.0, baseY),
                egui::vec2(barW, h),
            ),
            0.0,
            barColor,
        );
    }

    // 底部状态标签
    let (label, labelColor) = if is_playing {
        ("SIGNAL ACTIVE", color)
    } else {
        ("NO SIGNAL", dimColor)
    };
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 14.0),
        egui::Align2::CENTER_BOTTOM,
        label,
        egui::FontId::monospace(10.0),
        labelColor,
    );
}

#[cfg(windows)]
pub(crate) fn cursorScreenGlobal() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut pt) };
    if ok == 0 { None } else { Some((pt.x, pt.y)) }
}

#[cfg(not(windows))]
fn cursorScreenGlobal() -> Option<(i32, i32)> { None }

fn openMusicFolder(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(path)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}

// ── 辅助函数（从 settingsWindow.rs 复制）─────────────

pub(crate) fn loadIcon() -> Option<winit::window::Icon> {
    const PNG: &[u8] = include_bytes!("../icons/icon.png");
    let img = image::load_from_memory(PNG).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

pub(crate) fn installFonts(ctx: &Context, uiFontFamily: &str) {
    let mut fonts = egui::FontDefinitions::default();
    // 优先使用用户选择的界面字体
    let font_loaded = if !uiFontFamily.is_empty() {
        let font_dir = std::path::PathBuf::from(
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_default(),
        )
        .join("desktopPet")
        .join("fonts");
        if let Some(path) = crate::text::findFontFile(uiFontFamily, &font_dir) {
            std::fs::read(&path).ok().map(|bytes| {
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
                true
            }).unwrap_or(false)
        } else {
            false
        }
    } else {
        false
    };
    // 兜底：加载系统 CJK 字体
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

    // 加载像素字体作为 Monospace（终端风格）
    let font_dir = std::path::PathBuf::from(
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default(),
    )
    .join("desktopPet")
    .join("fonts");
    let pixel_path = font_dir.join("Z工坊像素圆体.ttf");
    if pixel_path.exists() {
        if let Ok(bytes) = std::fs::read(&pixel_path) {
            let key = "pixelTerminal".to_string();
            fonts
                .font_data
                .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, key);
        }
    }

    ctx.set_fonts(fonts);
}

#[cfg(windows)]
fn findCJKFont() -> Option<std::path::PathBuf> {
    let win = std::env::var_os("WINDIR")?;
    let fonts = std::path::Path::new(&win).join("Fonts");
    for name in &["msyh.ttc", "msyh.ttf", "simhei.ttf", "simsun.ttc"] {
        let p = fonts.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[cfg(not(windows))]
fn findCJKFont() -> Option<std::path::PathBuf> {
    None
}
