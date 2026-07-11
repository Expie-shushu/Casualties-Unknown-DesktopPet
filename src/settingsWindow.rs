// 独立设置 GUI 窗口：winit 窗口 + 独立 wgpu surface + egui 0.30；左侧分类导航 + 右侧详情。
#![allow(non_snake_case)]

use std::sync::Arc;

use anyhow::{anyhow, Result};
use egui::{Context, FontData, ViewportId};
use egui_wgpu::{wgpu, ScreenDescriptor};
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::input::PenetrationMode;
use crate::settings::{DecayPreset, DpiMode, FontModeKey, PenetrationKey, PetSettings};

const WINDOW_W: u32 = 720;
const WINDOW_H: u32 = 480;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsCategory {
    General,
    Display,
    Font,
    Penetration,
    Actions,
    Needs,
    Guide,
    About,
}

impl SettingsCategory {
    fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Display => "显示",
            Self::Font => "字体",
            Self::Penetration => "鼠标穿透",
            Self::Actions => "动作",
            Self::Needs => "需求",
            Self::Guide => "使用说明",
            Self::About => "关于",
        }
    }
    const ALL: [Self; 8] = [
        Self::General,
        Self::Display,
        Self::Font,
        Self::Penetration,
        Self::Actions,
        Self::Needs,
        Self::Guide,
        Self::About,
    ];
}

#[derive(Clone, Debug, Default)]
pub struct SettingsChange {
    pub stageScale: Option<f32>,
    /// 仓库窗口缩放比例变更。
    pub inventoryScale: Option<f32>,
    pub dpiMode: Option<DpiMode>,
    pub fontMode: Option<FontModeKey>,
    pub penetration: Option<(PenetrationMode, PenetrationKey)>,
    pub actionDurations: bool,
    pub needsConfig: bool,
    pub needsReset: bool,
    /// 对话气泡样式改动（字体/字号/底板透明度）。
    pub bubbleStyle: bool,
    /// 状态条样式改动（需重建字形缓存）。
    pub barStyle: bool,
    /// 需求表情包设置改动。
    pub stickerConfig: bool,
    /// 开机自启动开关改动。
    pub autoStart: bool,
    /// 全屏检测开关改动（游戏时自动隐藏桌宠）。
    pub fullscreenHide: bool,
    /// 皮肤切换（需重载场景）。
    pub skinChanged: bool,
    pub closed: bool,
}

impl SettingsChange {
    fn isEmpty(&self) -> bool {
        self.stageScale.is_none()
            && self.inventoryScale.is_none()
            && self.dpiMode.is_none()
            && self.fontMode.is_none()
            && self.penetration.is_none()
            && !self.actionDurations
            && !self.needsConfig
            && !self.needsReset
            && !self.bubbleStyle
            && !self.barStyle
            && !self.stickerConfig
            && !self.autoStart
            && !self.fullscreenHide
            && !self.skinChanged
            && !self.closed
    }
}

pub struct SettingsWindow {
    pub window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    category: SettingsCategory,
    pendingClose: bool,
    /// 仅当本帧需要重绘时才 present；空闲时为 false，避免阻塞式 present 拖慢主循环 / 桌宠动画。
    needsRedraw: bool,
    /// 当前已加载的字体族名列表（供下拉框动态展示，启动时从 TextSystem 获取）。
    availableFonts: Vec<String>,
    /// 可用的皮肤列表（data/skin/ 下的子目录）。
    availableSkins: Vec<String>,
}

impl SettingsWindow {
    pub fn create(el: &ActiveEventLoop, settings: &PetSettings, availableFonts: Vec<String>, availableSkins: Vec<String>, uiFontFamily: &str) -> Result<Self> {
        let _ = settings;
        let mut attrs = Window::default_attributes()
            .with_title("Casualties Unknown：desktopPet · 设置")
            .with_inner_size(LogicalSize::new(WINDOW_W, WINDOW_H))
            .with_resizable(true)
            .with_visible(false); // 首帧渲染成功后再显示，避免白闪
        if let Some(icon) = loadIcon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        let window: Arc<Window> = Arc::new(
            el.create_window(attrs)
                .map_err(|e| anyhow!("settings window create: {e}"))?,
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| anyhow!("settings surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow!("settings adapter not found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("settings-device"),
                required_features: wgpu::Features::empty(),
                // 用硬件真实上限而非 downlevel 的 2048，避免窗口放大后 surface 越界触发 wgpu panic。
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| anyhow!("settings device: {e}"))?;

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
            category: SettingsCategory::General,
            pendingClose: false,
            needsRedraw: true,
            availableFonts,
            availableSkins,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// 处理窗口事件；CloseRequested 由调用方关窗，其余交 egui。
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

    /// 本帧是否需要重绘 present。空闲时为 false，调用方据此跳过 frame()。
    pub fn wantsRedraw(&self) -> bool {
        self.needsRedraw
    }

    /// 跑一帧：采输入 → run UI → 写 settings 改动 → 提交渲染。返回累计改动。
    pub fn frame(&mut self, settings: &mut PetSettings) -> SettingsChange {
        let raw = self.egui.take_egui_input(&self.window);
        let mut change = SettingsChange::default();
        let category = &mut self.category;
        let full = self.egui.egui_ctx().clone().run(raw, |ctx| {
            drawUi(ctx, category, settings, &mut change, &self.availableFonts, &self.availableSkins);
        });
        // repaint_delay==ZERO 表示 egui 正在动画 / 需立即重绘 → 下帧继续；否则转入空闲不再 present。
        self.needsRedraw = full
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|v| v.repaint_delay.is_zero())
            .unwrap_or(false);
        self.egui
            .handle_platform_output(&self.window, full.platform_output);

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
                        log::warn!("settings surface frame: {e:?}");
                        return change;
                    }
                }
            }
            match f {
                Some(frame) => frame,
                None => {
                    log::warn!("settings surface keeps returning Outdated");
                    return change;
                }
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("settings-encoder"),
            });
        for (id, delta) in &full.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let pixelsPerPoint = full.pixels_per_point;
        let primitives = self.egui.egui_ctx().tessellate(full.shapes, pixelsPerPoint);
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: pixelsPerPoint,
        };
        self.renderer
            .update_buffers(&self.device, &self.queue, &mut encoder, &primitives, &screen);
        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("settings-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.10,
                                g: 0.10,
                                b: 0.12,
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
        change
    }

    pub fn requestRedraw(&self) {
        self.window.request_redraw();
    }
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

fn drawUi(
    ctx: &Context,
    category: &mut SettingsCategory,
    settings: &mut PetSettings,
    change: &mut SettingsChange,
    availableFonts: &[String],
    availableSkins: &[String],
) {
    egui::SidePanel::left("sidebar")
        .resizable(false)
        .exact_width(160.0)
        .show(ctx, |ui| {
            ui.add_space(12.0);
            ui.heading("设置");
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
            for cat in SettingsCategory::ALL {
                let selected = *category == cat;
                let resp = ui.add_sized(
                    [ui.available_width(), 32.0],
                    egui::SelectableLabel::new(selected, cat.label()),
                );
                if resp.clicked() {
                    *category = cat;
                }
            }
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading(category.label());
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);
            match category {
                SettingsCategory::General => drawGeneral(ui, settings, change, availableSkins),
                SettingsCategory::Display => drawDisplay(ui, settings, change),
                SettingsCategory::Font => drawFont(ui, settings, change, availableFonts),
                SettingsCategory::Penetration => drawPenetration(ui, settings, change),
                SettingsCategory::Actions => drawActions(ui, settings, change),
                SettingsCategory::Needs => drawNeeds(ui, settings, change),
                SettingsCategory::Guide => drawGuide(ui),
                SettingsCategory::About => drawAbout(ui),
            }
        });
    });

    if !change.isEmpty() {
        log::debug!("settings ui change pending");
    }
}

fn drawGeneral(ui: &mut egui::Ui, settings: &mut PetSettings, change: &mut SettingsChange, availableSkins: &[String]) {
    ui.label("舞台缩放");
    let mut scale = settings.stageScale;
    if ui
        .add(egui::Slider::new(&mut scale, 1.0..=8.0).step_by(0.5).text("×"))
        .changed()
    {
        settings.stageScale = scale;
        change.stageScale = Some(scale);
    }
    ui.separator();
    ui.label("仓库缩放");
    let mut invScale = settings.inventoryScale;
    if ui
        .add(egui::Slider::new(&mut invScale, 0.8..=1.5).step_by(0.05).text("×"))
        .changed()
    {
        settings.inventoryScale = invScale;
        change.inventoryScale = Some(invScale);
    }
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut settings.autoStart, "开机自启动")
            .changed()
        {
            change.autoStart = true;
        }
        ui.label("将程序添加到系统启动项");
    });
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut settings.fullscreenHideEnabled, "游戏时自动隐藏")
            .changed()
        {
            change.fullscreenHide = true;
        }
        ui.label("检测到全屏应用（游戏/办公）时自动隐藏桌宠");
    });
    ui.add_space(8.0);
    ui.separator();
    ui.separator();
    ui.label("桌宠皮肤（data/skin/ 下的文件夹）");
    if availableSkins.is_empty() {
        ui.label("未扫描到皮肤文件夹");
    } else {
        egui::ComboBox::from_id_salt("skin_select")
            .selected_text(settings.skin.as_str())
            .width(160.0)
            .show_ui(ui, |ui| {
                for s in availableSkins {
                    if ui.selectable_label(settings.skin == *s, s.as_str()).clicked() {
                        settings.skin = s.clone();
                        change.skinChanged = true;
                    }
                }
            });
        ui.label("（切换皮肤即时生效）");
    }
    ui.label(format!("初始姿态：{}", settings.poseName));
    ui.label(format!(
        "初始动作：{}",
        crate::motion::motionLabelZh(&settings.initialMotion)
    ));
}

fn drawDisplay(ui: &mut egui::Ui, settings: &mut PetSettings, change: &mut SettingsChange) {
    ui.label("DPI 模式");
    let mut mode = settings.dpiMode;
    let prev = mode;
    egui::ComboBox::from_id_salt("dpi")
        .selected_text(dpiLabel(mode))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut mode, DpiMode::System, dpiLabel(DpiMode::System));
            ui.selectable_value(&mut mode, DpiMode::Force1x, dpiLabel(DpiMode::Force1x));
            ui.selectable_value(&mut mode, DpiMode::Force2x, dpiLabel(DpiMode::Force2x));
        });
    if mode != prev {
        settings.dpiMode = mode;
        change.dpiMode = Some(mode);
    }
}

/// 内置常见字体族（族名, 中文标签）。空族名 = 系统默认无衬线。
/// 用户自定义字体：把字体文件放进 desktopPet/fonts/，重启后自动出现在下拉框中。
const COMMON_FONTS: &[(&str, &str)] = &[
    ("", "默认（系统无衬线）"),
    ("KaiTi", "楷体"),
    ("SimSun", "宋体"),
    ("NSimSun", "新宋体"),
    ("SimHei", "黑体"),
    ("Microsoft YaHei", "微软雅黑"),
    ("FangSong", "仿宋"),
];

/// 字体选择：内置常见下拉 + 动态扫描到的字体 + 自定义族名输入框。返回是否改动。
fn fontFamilyPicker(
    ui: &mut egui::Ui,
    id: &str,
    family: &mut String,
    availableFonts: &[String],
) -> bool {
    let mut changed = false;
    // 合并内置预设 + 动态扫描字体（去重，内置优先于动态）
    let mut all: Vec<(&str, &str)> = COMMON_FONTS.to_vec();
    for f in availableFonts {
        if !COMMON_FONTS.iter().any(|(v, _)| v == f) {
            // 动态字体：族名即标签（用户放的字体通常无中文标签）
            // 用 leak 把 String 转为 &'static str 仅用于展示（短期生命周期可接受）
            all.push((f.as_str(), f.as_str()));
        }
    }
    let curLabel = all
        .iter()
        .find(|(v, _)| *v == family.as_str())
        .map(|(_, l)| *l)
        .unwrap_or("自定义");
    ui.horizontal(|ui| {
        ui.label("字体");
        egui::ComboBox::from_id_salt(id)
            .selected_text(curLabel)
            .show_ui(ui, |ui| {
                for (val, label) in &all {
                    if ui
                        .selectable_label(family.as_str() == *val, *label)
                        .clicked()
                    {
                        *family = val.to_string();
                        changed = true;
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("自定义字体名");
        if ui.text_edit_singleline(family).changed() {
            changed = true;
        }
    });
    ui.label(
        egui::RichText::new("把字体文件放入 desktopPet/fonts/ 后重启桌宠，即可在下拉框中选取")
            .small()
            .weak(),
    );
    changed
}

fn drawFont(ui: &mut egui::Ui, settings: &mut PetSettings, change: &mut SettingsChange, availableFonts: &[String]) {
    // ── 字体来源（决定可用字体集合：内嵌 desktopPet/fonts + 系统字体）──
    ui.label("字体来源");
    let mut mode = settings.fontMode;
    let prev = mode;
    egui::ComboBox::from_id_salt("font")
        .selected_text(fontLabel(mode))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut mode, FontModeKey::Both, fontLabel(FontModeKey::Both));
            ui.selectable_value(&mut mode, FontModeKey::Embedded, fontLabel(FontModeKey::Embedded));
            ui.selectable_value(&mut mode, FontModeKey::System, fontLabel(FontModeKey::System));
        });
    if mode != prev {
        settings.fontMode = mode;
        change.fontMode = Some(mode);
    }

    ui.add_space(16.0);
    ui.separator();

    // ── 界面字体 ──
    ui.heading("界面字体");
    ui.label("（设置 / 仓库 / 音乐播放器等窗口的文字字体）");
    if fontFamilyPicker(ui, "uiFont", &mut settings.uiFontFamily, availableFonts) {
        change.bubbleStyle = true; // 触发保存
    }
    ui.label("(修改后重新打开对应窗口即可看到字体变化)");

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(12.0);

    // ── 对话气泡样式 ──
    ui.heading("对话气泡");
    if fontFamilyPicker(ui, "bubbleFont", &mut settings.bubbleStyle.fontFamily, availableFonts) {
        change.bubbleStyle = true;
    }
    ui.horizontal(|ui| {
        ui.label("字号");
        if ui
            .add(egui::Slider::new(&mut settings.bubbleStyle.fontSizePx, 10.0..=48.0).step_by(1.0))
            .changed()
        {
            change.bubbleStyle = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("背景透明度");
        if ui
            .add(egui::Slider::new(&mut settings.bubbleStyle.bgAlpha, 0.0..=1.0).step_by(0.01))
            .changed()
        {
            change.bubbleStyle = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("文字颜色");
        let mut rgb = [settings.bubbleStyle.textColor[0], settings.bubbleStyle.textColor[1], settings.bubbleStyle.textColor[2]];
        if ui.color_edit_button_rgb(&mut rgb).changed() {
            settings.bubbleStyle.textColor[0] = rgb[0];
            settings.bubbleStyle.textColor[1] = rgb[1];
            settings.bubbleStyle.textColor[2] = rgb[2];
            change.bubbleStyle = true;
        }
        ui.label("透明度");
        let mut alpha = settings.bubbleStyle.textColor[3];
        if ui.add(egui::Slider::new(&mut alpha, 0.0..=1.0).step_by(0.01)).changed() {
            settings.bubbleStyle.textColor[3] = alpha;
            change.bubbleStyle = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("背景颜色");
        if ui
            .color_edit_button_rgb(&mut settings.bubbleStyle.bgColor)
            .changed()
        {
            change.bubbleStyle = true;
        }
    });

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(12.0);

    // ── 状态条样式 ──
    ui.heading("状态条");
    if fontFamilyPicker(ui, "barFont", &mut settings.barStyle.fontFamily, availableFonts) {
        change.barStyle = true;
    }
    ui.horizontal(|ui| {
        ui.label("标题字号");
        if ui
            .add(egui::Slider::new(&mut settings.barStyle.titleSizePx, 8.0..=24.0).step_by(1.0))
            .changed()
        {
            change.barStyle = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("数字字号");
        if ui
            .add(egui::Slider::new(&mut settings.barStyle.digitSizePx, 8.0..=28.0).step_by(1.0))
            .changed()
        {
            change.barStyle = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("面板透明度");
        if ui
            .add(egui::Slider::new(&mut settings.barStyle.bgAlpha, 0.0..=1.0).step_by(0.01))
            .changed()
        {
            change.barStyle = true;
        }
    });
    ui.label(
        egui::RichText::new("状态条字体/字号改动会重建字形缓存后即时生效")
            .small()
            .weak(),
    );
}

fn drawPenetration(ui: &mut egui::Ui, settings: &mut PetSettings, change: &mut SettingsChange) {
    ui.label("鼠标穿透模式");
    let mut key = settings.penetration;
    let prev = key;
    ui.radio_value(&mut key, PenetrationKey::Smart, "智能（精灵区不穿透）");
    ui.radio_value(&mut key, PenetrationKey::Never, "始终不穿透");
    ui.radio_value(&mut key, PenetrationKey::Always, "始终穿透");
    if key != prev {
        settings.penetration = key;
        let mode = match key {
            PenetrationKey::Always => PenetrationMode::Always,
            PenetrationKey::Never => PenetrationMode::Never,
            PenetrationKey::Smart => PenetrationMode::Smart,
        };
        change.penetration = Some((mode, key));
    }
}

fn drawActions(ui: &mut egui::Ui, settings: &mut PetSettings, change: &mut SettingsChange) {
    ui.label("自动动作序列时长（秒）");
    ui.add_space(8.0);
    let d = &mut settings.actionDurations;
    let mut dirty = false;
    ui.horizontal(|ui| {
        ui.label("待机 → 坐下");
        if ui.add(egui::Slider::new(&mut d.idleToSitSec, 1.0..=120.0).step_by(1.0)).changed() {
            dirty = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("坐 → 躺下");
        if ui.add(egui::Slider::new(&mut d.sitToLaySec, 1.0..=120.0).step_by(1.0)).changed() {
            dirty = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("躺 → 趴下");
        if ui.add(egui::Slider::new(&mut d.layToPlankSec, 1.0..=120.0).step_by(1.0)).changed() {
            dirty = true;
        }
    });
    ui.add_space(12.0);
    ui.label("行走 / 奔跑动作单次持续秒数（行为决策器使用）");
    ui.horizontal(|ui| {
        ui.label("行走 单次");
        if ui.add(egui::Slider::new(&mut d.walkSec, 1.0..=20.0).step_by(0.5)).changed() {
            dirty = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("奔跑 单次");
        if ui.add(egui::Slider::new(&mut d.runSec, 1.0..=20.0).step_by(0.5)).changed() {
            dirty = true;
        }
    });
    if dirty {
        change.actionDurations = true;
    }
}

fn drawNeeds(ui: &mut egui::Ui, settings: &mut PetSettings, change: &mut SettingsChange) {
    // 当前数值预览。
    let n = &settings.needs;
    ui.label(format!(
        "当前：心情 {:.0} / 饥饿 {:.0} / 口渴 {:.0}",
        n.mood, n.hunger, n.thirst
    ));
    ui.add_space(12.0);

    ui.label("衰减速率");
    let mut preset = settings.needsConfig.decayPreset;
    let prev = preset;
    ui.radio_value(&mut preset, DecayPreset::Slow, "慢（约 3 小时）");
    ui.radio_value(&mut preset, DecayPreset::Medium, "中（约 40 分钟）");
    ui.radio_value(&mut preset, DecayPreset::Fast, "快（约 10 分钟）");
    if preset != prev {
        settings.needsConfig.decayPreset = preset;
        change.needsConfig = true;
    }

    ui.add_space(12.0);
    if ui
        .checkbox(&mut settings.needsConfig.bubblesEnabled, "低值时弹出抱怨气泡")
        .changed()
    {
        change.needsConfig = true;
    }
    if ui
        .checkbox(&mut settings.needsConfig.barEnabled, "鼠标悬停时显示状态条")
        .changed()
    {
        change.needsConfig = true;
    }

    ui.add_space(12.0);
    if ui
        .checkbox(&mut settings.needsConfig.chatterEnabled, "空闲时随机闲聊气泡")
        .changed()
    {
        change.needsConfig = true;
    }
    ui.add_enabled_ui(settings.needsConfig.chatterEnabled, |ui| {
        ui.label("闲聊出现间隔（秒，区间内随机）");
        ui.horizontal(|ui| {
            ui.label("最短");
            if ui
                .add(egui::Slider::new(
                    &mut settings.needsConfig.chatterMinSec,
                    5.0..=300.0,
                ).step_by(1.0))
                .changed()
            {
                // 保证 min ≤ max。
                if settings.needsConfig.chatterMinSec > settings.needsConfig.chatterMaxSec {
                    settings.needsConfig.chatterMaxSec = settings.needsConfig.chatterMinSec;
                }
                change.needsConfig = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("最长");
            if ui
                .add(egui::Slider::new(
                    &mut settings.needsConfig.chatterMaxSec,
                    5.0..=300.0,
                ).step_by(1.0))
                .changed()
            {
                if settings.needsConfig.chatterMaxSec < settings.needsConfig.chatterMinSec {
                    settings.needsConfig.chatterMinSec = settings.needsConfig.chatterMaxSec;
                }
                change.needsConfig = true;
            }
        });
        ui.label(
            egui::RichText::new("台词在 desktopPet/configs/chatter.json 中编辑")
                .small()
                .weak(),
        );
    });

    // ── 需求表情包 ──
    ui.add_space(16.0);
    ui.separator();
    ui.heading("需求表情包");
    if ui
        .checkbox(&mut settings.stickerConfig.enabled, "根据需求状态自动弹出表情包")
        .changed()
    {
        change.stickerConfig = true;
    }
    ui.add_enabled_ui(settings.stickerConfig.enabled, |ui| {
        ui.label("表情包出现间隔（秒，区间内随机）");
        ui.horizontal(|ui| {
            ui.label("最短");
            if ui
                .add(egui::Slider::new(
                    &mut settings.stickerConfig.minIntervalSec,
                    5.0..=300.0,
                ).step_by(1.0))
                .changed()
            {
                if settings.stickerConfig.minIntervalSec > settings.stickerConfig.maxIntervalSec {
                    settings.stickerConfig.maxIntervalSec = settings.stickerConfig.minIntervalSec;
                }
                change.stickerConfig = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("最长");
            if ui
                .add(egui::Slider::new(
                    &mut settings.stickerConfig.maxIntervalSec,
                    5.0..=300.0,
                ).step_by(1.0))
                .changed()
            {
                if settings.stickerConfig.maxIntervalSec < settings.stickerConfig.minIntervalSec {
                    settings.stickerConfig.minIntervalSec = settings.stickerConfig.maxIntervalSec;
                }
                change.stickerConfig = true;
            }
        });
        ui.label(
            egui::RichText::new("表情包放在 desktopPet/stickers/ 对应子目录中")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        ui.label("表情包显示尺寸（像素）");
        ui.horizontal(|ui| {
            ui.label("宽度");
            if ui
                .add(egui::Slider::new(
                    &mut settings.stickerConfig.stickerWidth,
                    60.0..=400.0,
                ).step_by(10.0))
                .changed()
            {
                change.stickerConfig = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("高度");
            if ui
                .add(egui::Slider::new(
                    &mut settings.stickerConfig.stickerHeight,
                    60.0..=400.0,
                ).step_by(10.0))
                .changed()
            {
                change.stickerConfig = true;
            }
        });
        ui.label(
            egui::RichText::new(
                "moodHigh / moodLow / hungerHigh / hungerLow / thirstHigh / thirstLow"
            )
                .small()
                .weak(),
        );
    });
}

fn drawGuide(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("基础操作").strong().size(15.0));
    ui.add_space(4.0);
    ui.label("• 左键拖拽桌宠 → 移动桌宠位置");
    ui.label("• 双击桌宠身体 → 打招呼");
    ui.label("• 右键桌宠 → 弹出功能菜单");
    ui.label("• 鼠标悬停桌宠 → 显示状态条");

    ui.add_space(16.0);
    ui.label(egui::RichText::new("喂食").strong().size(15.0));
    ui.add_space(4.0);
    ui.label("• 鼠标右键可拖动食物，将其投喂到桌宠身上即可投喂");
    ui.label("• 喂食成功播放喂食动画");
    ui.label("• 口腔槽有物品时，气泡文字会口齿不清");

    ui.add_space(16.0);
    ui.label(egui::RichText::new("仓库/背包").strong().size(15.0));
    ui.add_space(4.0);
    ui.label("• 右键菜单 →「仓库/背包」打开仓库窗口");
    ui.label("• 中间圆环 = 6 个主槽位：主手、副手、上背部、下背部、中背部、口腔");
    ui.label("• 圆环下方横条 = 已拥有的背包（绿色=已装备，灰色=未装备）");
    ui.label("• 左键背包图标 → 切换装备/卸下（装备后渲染在桌宠身上）");
    ui.label("• 右键已装备背包 → 打开子仓库面板，可存取物品");
    ui.label("• 右键仓库物品 → 拿起（跟随鼠标）；左键/拖到空位 → 放下");
    ui.label("• 背包通过游戏赢取，获得后 24 小时过期（含内容物）");

    ui.add_space(16.0);
    ui.label(egui::RichText::new("小游戏").strong().size(15.0));
    ui.add_space(4.0);
    ui.label("•「与exp猜拳」→ 打开石头剪刀布对战窗口，赢一局 +1 硬币");
    ui.label("•「抽奖轮盘」→ 消耗硬币转轮盘抽奖，随机掉落食物或背包");
    ui.label("  - 扇区比例：食品 40% / 饮品 40% / 背包 20%");
    ui.label("  - 拖拽硬币图标到投币口 → 点击 PRESS 按钮开始旋转");
    ui.label("  - 转完自动掉落奖励，窗口不自动关闭");

    ui.add_space(16.0);
    ui.label(egui::RichText::new("音乐").strong().size(15.0));
    ui.add_space(4.0);
    ui.label("•「听/停音乐」→ 切换听歌 / 停止（戴耳机 / 摘耳机）");
    ui.label("•「音乐播放器」→ 打开播放器，管理歌单和播放控制");
    ui.label("• 播放模式：顺序播放（播完停）/ 列表循环 / 单曲循环");
    ui.label("• 点击歌单曲目切歌，拖动进度条跳转，音量/速度可调");
    ui.label("• CFG 按钮 → 播放器外观自定义（颜色/尺寸/布局）");
    ui.label("• 歌曲文件放入 desktopPet/music/ 目录自动识别（mp3/wav/flac/ogg）");

    ui.add_space(16.0);
    ui.label(egui::RichText::new("设置").strong().size(15.0));
    ui.add_space(4.0);
    ui.label("•「通用」→ 皮肤切换、舞台缩放、仓库缩放、开机自启、全屏检测");
    ui.label("•「显示」→ 桌宠大小（1~8 倍）、DPI 模式");
    ui.label("•「字体」→ 全局字体来源、气泡样式、状态条样式");
    ui.label("•「鼠标穿透」→ Smart（智能穿透）/ Never（永不穿透）/ Always（始终穿透）");
    ui.label("•「动作」→ 待机→坐下→躺下→趴下的自动切换时长");
    ui.label("•「需求」→ 衰减速度、气泡/状态条/贴纸开关及参数");

    ui.add_space(16.0);
    ui.label(egui::RichText::new("备注").strong().size(15.0));
    ui.add_space(4.0);
    ui.label("• ESC 键可关闭大多数弹窗（仓库/播放器/CFG/转盘/猜拳）");
    ui.label("• 系统托盘右键 → 显示/隐藏桌宠、打开设置、退出程序");
    ui.label("• 全屏检测开启后，打游戏/看视频时桌宠自动隐藏");
    ui.label("• 状态条和气泡可在「需求」标签页分别开关");
    ui.label("• 配置文件位于 desktopPet/configs/<petId>.json，可手动编辑");
    ui.label("• 表情贴纸放在 desktopPet/stickers/<分类>/ 下自动加载");
    ui.label("• Chatter 台词可在 desktopPet/configs/chatter.json 自定义");
}

fn drawAbout(ui: &mut egui::Ui) {
    ui.label("Casualties Unknown 桌宠");
    ui.label(format!("版本 v{}", env!("CARGO_PKG_VERSION")));
    ui.label("作者：huanxin996、Expie鼠鼠");
    ui.add_space(8.0);
    ui.label("独立 Rust + winit + wgpu + egui 实现的桌宠运行时。");
    ui.label("配套于 Casualties Unknown 皮肤编辑器。");
    ui.hyperlink_to(
        "Github - huanxin996",
        "https://github.com/huanxin996",
    );
    ui.hyperlink_to(
        "Github - Expie鼠鼠",
        "https://github.com/Expie-shushu",
    );
    ui.add_space(8.0);
}

fn dpiLabel(m: DpiMode) -> &'static str {
    match m {
        DpiMode::System => "跟随系统",
        DpiMode::Force1x => "强制 1×",
        DpiMode::Force2x => "强制 2×",
    }
}

fn fontLabel(m: FontModeKey) -> &'static str {
    match m {
        FontModeKey::Both => "内嵌 + 系统",
        FontModeKey::Embedded => "仅内嵌",
        FontModeKey::System => "仅系统",
    }
}
