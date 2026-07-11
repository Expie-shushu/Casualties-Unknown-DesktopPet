// 桌宠设置：缩放 / DPI 模式 / 字体来源 / 穿透。JSON 持久化到 desktopPet/configs/<petId>.json。
#![allow(non_snake_case)]

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::input::PenetrationMode;
use crate::text::FontSourceMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DpiMode {
    System,
    Force1x,
    Force2x,
}

impl Default for DpiMode {
    fn default() -> Self {
        Self::System
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionDurations {
    pub idleToSitSec: f32,
    pub sitToLaySec: f32,
    pub layToPlankSec: f32,
    pub walkSec: f32,
    pub runSec: f32,
}

impl Default for ActionDurations {
    fn default() -> Self {
        Self {
            idleToSitSec: 12.0,
            sitToLaySec: 20.0,
            layToPlankSec: 30.0,
            walkSec: 5.0,
            runSec: 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecayPreset {
    Slow,
    Medium,
    Fast,
}

impl Default for DecayPreset {
    fn default() -> Self {
        Self::Slow
    }
}

impl DecayPreset {
    /// 每秒衰减点数。Slow≈3 小时满→空，Medium≈40 分钟，Fast≈10 分钟（调试）。
    pub fn perSec(self) -> f32 {
        match self {
            Self::Slow => 100.0 / (3.0 * 3600.0),
            Self::Medium => 100.0 / (40.0 * 60.0),
            Self::Fast => 100.0 / (10.0 * 60.0),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NeedsConfig {
    pub decayPreset: DecayPreset,
    pub bubblesEnabled: bool,
    pub barEnabled: bool,
    /// 空闲随机闲聊气泡开关。
    pub chatterEnabled: bool,
    /// 闲聊出现间隔的下/上限（秒）；实际间隔在二者之间随机。
    pub chatterMinSec: f32,
    pub chatterMaxSec: f32,
}

impl Default for NeedsConfig {
    fn default() -> Self {
        Self {
            decayPreset: DecayPreset::Slow,
            bubblesEnabled: true,
            barEnabled: true,
            chatterEnabled: true,
            chatterMinSec: 25.0,
            chatterMaxSec: 60.0,
        }
    }
}

/// 对话气泡文字样式（用户可在设置中调）。
/// `fontFamily` 为空表示用默认 SansSerif；非空则按字体族名匹配（如 "KaiTi"/"SimSun"，
/// 或用户丢进 desktopPet/fonts/ 的自定义字体族名）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BubbleStyle {
    pub fontFamily: String,
    pub fontSizePx: f32,
    /// 底板不透明度（0~1），与 bgColor 的 RGB 结合组成最终背景色。
    pub bgAlpha: f32,
    /// 气泡文字颜色 RGBA（线性空间）。默认近黑色。
    pub textColor: [f32; 4],
    /// 气泡底板颜色 RGB（线性空间），alpha 由 bgAlpha 控制。
    pub bgColor: [f32; 3],
}

fn defaultBubbleTextColor() -> [f32; 4] {
    [0.10, 0.10, 0.13, 1.0]
}
fn defaultBubbleBgColor() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

impl Default for BubbleStyle {
    fn default() -> Self {
        Self {
            fontFamily: String::new(),
            fontSizePx: 20.0,
            bgAlpha: 0.30,
            textColor: defaultBubbleTextColor(),
            bgColor: defaultBubbleBgColor(),
        }
    }
}

/// 头顶状态条文字样式（标题与数字字号分开；面板底板透明度）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BarStyle {
    pub fontFamily: String,
    pub titleSizePx: f32,
    pub digitSizePx: f32,
    /// 半透明底板不透明度（0~1）。
    pub bgAlpha: f32,
}

impl Default for BarStyle {
    fn default() -> Self {
        Self {
            fontFamily: String::new(),
            titleSizePx: 12.0,
            digitSizePx: 15.0,
            bgAlpha: 0.62,
        }
    }
}

fn defaultInventoryScale() -> f32 { 0.8 } // 已定死：用户调好的仓库缩放
fn default_true() -> bool { true }

/// 需求表情包设置（用户在设置面板「需求」页调整，持久化）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct StickerConfig {
    /// 是否启用需求表情包自动弹出。
    pub enabled: bool,
    /// 需求表情包出现间隔下限（秒）。
    pub minIntervalSec: f32,
    /// 需求表情包出现间隔上限（秒）。实际间隔在 [min, max] 之间随机。
    pub maxIntervalSec: f32,
    /// 表情包显示宽度（像素）。
    pub stickerWidth: f32,
    /// 表情包显示高度（像素）。
    pub stickerHeight: f32,
}

impl Default for StickerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            minIntervalSec: 20.0,
            maxIntervalSec: 60.0,
            stickerWidth: 128.0,
            stickerHeight: 128.0,
        }
    }
}

/// 石头剪刀布按钮设置（桌宠右侧的图片按钮，用户在按钮窗小编辑器里调，持久化）。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RpsConfig {
    /// 按钮边长。
    pub buttonSize: f32,
    /// 按钮竖排间距。
    pub buttonGap: f32,
    /// 相对桌宠右侧的水平偏移。
    pub offsetX: f32,
    /// 相对桌宠顶部的垂直偏移。
    pub offsetY: f32,
}

impl Default for RpsConfig {
    fn default() -> Self {
        // 已定死：来自用户在按钮窗调好并保存的参数（2026-06-26）。
        Self { buttonSize: 45.0, buttonGap: 12.0, offsetX: -100.0, offsetY: 60.0 }
    }
}

/// 抽奖转盘窗口布局配置。
/// 转盘配置窗口中 hover 时在转盘主窗口高亮的区域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelHighlightZone {
    Window,
    Appearance,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WheelConfig {
    // ── 窗口 ──
    /// 窗口 X 坐标（逻辑像素），<0 = 首次打开时自动定位到桌宠右侧。
    pub windowPosX: f32,
    /// 窗口 Y 坐标（逻辑像素），<0 = 由系统决定。
    pub windowPosY: f32,
    /// 窗口宽度（逻辑像素）。
    pub windowWidth: f32,
    /// 窗口高度（逻辑像素）。
    pub windowHeight: f32,

    // ── 转盘几何 ──
    /// 转盘圆心 X（相对窗口左上角）。
    pub wheelCenterX: f32,
    /// 转盘圆心 Y（相对窗口左上角）。
    pub wheelCenterY: f32,
    /// 转盘外半径。
    pub wheelOuterR: f32,
    /// 转盘内半径（圆环内圈，中心按钮之外）。
    pub wheelInnerR: f32,
    /// 中心按钮半径。
    pub pressButtonR: f32,
    /// 扇区图标大小倍率（相对环带宽度，默认 0.55）。
    pub iconScale: f32,
    /// 扇区图标径向位置（0=内圈, 0.5=中间, 1=外圈，默认 0.5）。
    pub iconRadiusFrac: f32,

    // ── 指针（转盘顶部）──
    /// 指针图片宽度。
    pub pointerW: f32,
    /// 指针图片高度。
    pub pointerH: f32,
    /// 指针中心 X（相对窗口左上角）。
    pub pointerX: f32,
    /// 指针中心 Y（相对窗口左上角）。
    pub pointerY: f32,

    // ── Token Jar（左上角）──
    pub jarX: f32,
    pub jarY: f32,
    pub jarW: f32,
    pub jarH: f32,

    // ── Coin 图标（Token Jar 内显示，绝对坐标）──
    /// Coin 图片宽度。
    pub coinImgW: f32,
    /// Coin 图片高度。
    pub coinImgH: f32,
    /// Coin 图片 X（相对窗口左上角）。
    pub coinImgX: f32,
    /// Coin 图片 Y（相对窗口左上角）。
    pub coinImgY: f32,

    // ── Insert Token 槽（Jar 下方）──
    pub slotX: f32,
    pub slotY: f32,
    pub slotW: f32,
    pub slotH: f32,

    // ── 右上角按钮 ──
    /// 设置按钮 X（相对窗口左上角）。
    pub settingsBtnX: f32,
    /// 设置按钮 Y（相对窗口左上角）。
    pub settingsBtnY: f32,
    /// 关闭按钮 X（相对窗口左上角）。
    pub closeBtnX: f32,
    /// 关闭按钮 Y（相对窗口左上角）。
    pub closeBtnY: f32,
    /// 右上角按钮尺寸（正方形边长）。
    pub cornerBtnSz: f32,

    // ── 外观 ──
    /// 背景色 [R, G, B]。
    pub bgColor: [u8; 3],
    /// 强调色（高亮绿）[R, G, B]。
    pub accentColor: [u8; 3],
    /// 暗调强调色 [R, G, B]。
    pub accentDimColor: [u8; 3],
    /// 文字色 [R, G, B]。
    pub textColor: [u8; 3],
    /// 暗调文字色 [R, G, B]。
    pub textDimColor: [u8; 3],
    /// PRESS 按钮 hover 时放大倍率。
    pub pressHoverScale: f32,
    /// 背景辉光透明度。
    pub glowAlpha: f32,
    /// 辉光扩散半径。
    pub glowRadius: f32,
    /// 切角像素。
    pub cornerCutPx: f32,
    /// 边框线宽。
    pub borderWidth: f32,
    /// CRT 扫描线间距。
    pub scanlineSpacing: f32,
    /// CRT 扫描线透明度。
    pub scanlineAlpha: f32,
}

impl Default for WheelConfig {
    fn default() -> Self {
        Self {
            windowPosX: 859.0,
            windowPosY: 259.0,
            windowWidth: 600.0,
            windowHeight: 530.0,
            wheelCenterX: 370.0,
            wheelCenterY: 260.0,
            wheelOuterR: 175.0,
            wheelInnerR: 40.0,
            pressButtonR: 57.0,
            iconScale: 0.6,
            iconRadiusFrac: 0.55,
            pointerW: 28.0,
            pointerH: 36.0,
            pointerX: 370.0,
            pointerY: 85.0,
            jarX: 15.0,
            jarY: 50.0,
            jarW: 150.0,
            jarH: 145.0,
            coinImgW: 100.0,
            coinImgH: 100.0,
            coinImgX: 90.0,
            coinImgY: 120.0,
            slotX: 15.0,
            slotY: 260.0,
            slotW: 150.0,
            slotH: 170.0,
            settingsBtnX: 520.0,
            settingsBtnY: 16.0,
            closeBtnX: 560.0,
            closeBtnY: 16.0,
            cornerBtnSz: 32.0,
            bgColor: [5, 7, 8],
            accentColor: [0, 255, 127],
            accentDimColor: [0, 89, 51],
            textColor: [184, 216, 192],
            textDimColor: [90, 122, 106],
            pressHoverScale: 1.0,
            glowAlpha: 0.35,
            glowRadius: 17.0,
            cornerCutPx: 7.0,
            borderWidth: 2.5,
            scanlineSpacing: 5.0,
            scanlineAlpha: 0.005,
        }
    }
}

/// 仓库圆环装备盘外观样式（用户在仓库窗「🎨 外观」面板实时调，持久化到 JSON）。
/// 颜色用 [u8;4] RGBA（egui 侧 `Color32::from_rgba_unmultiplied`）。默认值=改版前硬编码观感。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct InventoryStyle {
    /// 整体大小倍率：圆环/背包栏/子仓库的所有几何与字号统一乘以它。
    pub overallScale: f32,
    // ── 圆环几何 ──
    /// 环带内半径。
    pub ringInnerR: f32,
    /// 环带外半径。
    pub ringOuterR: f32,
    /// 扇区内图标所在半径。
    pub iconRadius: f32,
    /// 外侧标签文字所在半径。
    pub labelRadius: f32,
    /// 相邻扇区之间的角度间隙（度）。
    pub sectorGapDeg: f32,
    // ── 颜色（RGBA）──
    /// 空扇区填充。
    pub sectorEmpty: [u8; 4],
    /// 有物扇区填充。
    pub sectorOccupied: [u8; 4],
    /// 悬停扇区填充。
    pub sectorHover: [u8; 4],
    /// 扇区描边色。
    pub strokeColor: [u8; 4],
    /// 扇区描边粗细。
    pub strokeWidth: f32,
    /// 外侧标签文字色。
    pub labelColor: [u8; 4],
    /// 圆心文字色。
    pub centerColor: [u8; 4],
    // ── 字号 ──
    /// 标签字号。
    pub labelFontPx: f32,
    /// 圆心文字字号。
    pub centerFontPx: f32,
    // ── 背包栏 ──
    /// 背包格子边长。
    pub bpCellSize: f32,
    /// 背包格子间距。
    pub bpCellGap: f32,
    /// 已装备背包底色。
    pub bpEquipped: [u8; 4],
    /// 未装备背包底色。
    pub bpUnequipped: [u8; 4],
    // ── 子仓库网格（打开背包后的格子）──
    /// 子仓库格子边长。
    pub subCellSize: f32,
    /// 子仓库格子间距（横/纵）。
    pub subCellGap: f32,
    /// 子仓库格子底色。
    pub subCellBg: [u8; 4],
    // ── 面板 ──
    /// 子仓库面板底色（含 alpha）。
    pub subPanelBg: [u8; 4],
    // ── 槽位背景图 ──
    /// 6 个主槽位背景图透明度（0~1），下标=主槽索引 0主手 1副手 2上背 3下背 4中背 5口腔。
    pub slotBgAlpha: [f32; 6],
}

impl Default for InventoryStyle {
    fn default() -> Self {
        Self {
            // 已定死：来自用户在仓库窗实时调好并保存的参数（2026-06-26）。
            overallScale: 1.0,
            ringInnerR: 105.0,
            ringOuterR: 158.0,
            iconRadius: 127.0,
            labelRadius: 180.0,
            sectorGapDeg: 2.0,
            sectorEmpty: [27, 27, 27, 223],
            sectorOccupied: [50, 60, 50, 255],
            sectorHover: [249, 247, 247, 255],
            strokeColor: [200, 200, 200, 255],
            strokeWidth: 2.5,
            labelColor: [230, 230, 230, 255],
            centerColor: [255, 255, 255, 255],
            labelFontPx: 12.5,
            centerFontPx: 24.0,
            bpCellSize: 57.0,
            bpCellGap: 14.5,
            bpEquipped: [7, 57, 50, 200],
            bpUnequipped: [5, 5, 5, 205],
            subCellSize: 58.0,
            subCellGap: 1.0,
            subCellBg: [50, 45, 45, 194],
            subPanelBg: [0, 0, 0, 0],
            slotBgAlpha: [0.4, 0.4, 0.6, 0.6, 0.6, 0.4],
        }
    }
}

/// 音乐播放器外观样式 — Sci-Fi HUD 终端风格。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicPlayerStyle {
    // ── 窗口尺寸 ──
    pub windowWidth: f32,
    pub windowHeight: f32,
    /// 初始窗口 X 坐标（逻辑像素），<0 = 由系统决定。
    pub windowPosX: f32,
    /// 初始窗口 Y 坐标（逻辑像素），<0 = 由系统决定。
    pub windowPosY: f32,

    // ── 核心颜色 (linear RGB) ──
    /// 窗口背景最深色。
    pub bgColor: [f32; 3],
    /// 荧光绿强调色。
    pub accentColor: [f32; 3],
    /// 暗强调色（边框等）。
    pub accentDimColor: [f32; 3],
    /// 主文字浅绿色。
    pub textColor: [f32; 3],
    /// 次级文字暗绿色。
    pub textDimColor: [f32; 3],
    /// 警告/状态琥珀色。
    pub warnColor: [f32; 3],
    /// 歌单普通行底色。
    pub rowColorNormal: [f32; 3],
    /// 歌单当前播放行底色。
    pub rowColorCurrent: [f32; 3],
    /// 歌单鼠标悬停行底色。
    pub rowColorHover: [f32; 3],
    /// 进度条未播放部分底色。
    pub progressBgColor: [f32; 3],
    /// 波形占位图颜色。
    pub waveformColor: [f32; 3],

    // ── 边框 & 辉光 ──
    /// 边框线宽（像素）。
    pub borderWidth: f32,
    /// 切角大小（像素）。
    pub cornerCutPx: f32,
    /// 辉光扩散半径（像素）。
    pub glowRadius: f32,
    /// 辉光峰值透明度。
    pub glowAlpha: f32,

    // ── CRT 扫描线 ──
    /// 是否绘制 CRT 扫描线。
    pub scanlineEnabled: bool,
    /// 扫描线透明度 (推荐 0.02~0.06)。
    pub scanlineAlpha: f32,
    /// 扫描线间距（像素）。
    pub scanlineSpacing: f32,

    // ── 进度条 ──
    /// 进度条区域高度（像素）。
    pub progressBarHeight: f32,
    /// 分段数量（如 40 段）。
    pub progressSegments: u32,
    /// 段间间隙（像素）。
    pub progressSegmentGap: f32,

    // ── 控制区 ──
    /// 底部控制栏高度（像素）。
    pub bottomBarHeight: f32,
    /// 模式栏按钮尺寸（像素）。
    pub modeButtonSize: f32,
    /// 传输按钮（prev/play/next）基准尺寸（像素）。
    pub transportButtonSize: f32,
    /// 倍速预设按钮尺寸（像素）。
    pub speedButtonSize: f32,
    /// 播放按钮放大倍率（相对 transportButtonSize）。
    pub playButtonScale: f32,
    /// 按钮间距（像素）。
    pub buttonGap: f32,

    // ── 布局尺寸 ──
    /// 左侧封面区宽度（像素）。
    pub leftSectionWidth: f32,
    /// 右侧音量/倍速区宽度（像素），0 = 撑满。
    pub rightSectionWidth: f32,
    /// 音量/倍速区高度（像素），0 = 由按钮尺寸推导。
    pub volumeSpeedHeight: f32,
    /// 顶部信息区高度（像素）。
    pub topInfoHeight: f32,
    /// 顶部信息区宽度（像素），0 = 撑满。
    pub topInfoWidth: f32,
    /// 歌单行基准高度（像素）。
    pub playlistRowHeight: f32,
    /// 歌单行内文字 X 偏移（像素）。
    pub playlistRowOffsetX: f32,
    /// 歌单行内文字 Y 偏移（像素）。
    pub playlistRowOffsetY: f32,
    /// 歌单行之间额外间距（像素）。
    pub playlistRowSpacing: f32,
    /// 歌单行背景矩形 X 偏移（像素）。
    pub playlistRowBgOffsetX: f32,
    /// 歌单行背景矩形 Y 偏移（像素）。
    pub playlistRowBgOffsetY: f32,
    /// 歌单行背景矩形宽度（像素），0 = 撑满。
    pub playlistRowBgWidth: f32,
    /// 歌单容器高度（像素），0 = 自动填充剩余空间。
    pub playlistHeight: f32,
    /// 歌单容器宽度（像素），0 = 撑满。
    pub playlistWidth: f32,
    /// 模式栏高度（像素），0 = 由按钮尺寸推导。
    pub modeBarHeight: f32,
    /// 模式栏宽度（像素），0 = 撑满。
    pub modeBarWidth: f32,
    /// 底部状态栏高度（像素）。
    pub statusBarHeight: f32,
    /// 底部状态栏宽度（像素），0 = 撑满。
    pub statusBarWidth: f32,
    /// 进度条宽度（像素），0 = 撑满。
    pub progressBarWidth: f32,
    /// 底部控制栏宽度（像素），0 = 撑满。
    pub transportWidth: f32,
    /// 宽度受限区域的对齐方式。
    pub zoneAlignment: ZoneAlignment,

    // ── Zone 位置微调与间距 ──
    /// Zone 之间的垂直间距（像素）。
    pub zoneSpacing: f32,
    pub topPanelOffsetX: f32,       pub topPanelOffsetY: f32,
    pub modeBarOffsetX: f32,        pub modeBarOffsetY: f32,
    pub playlistOffsetX: f32,       pub playlistOffsetY: f32,
    pub progressBarOffsetX: f32,    pub progressBarOffsetY: f32,
    pub transportOffsetX: f32,      pub transportOffsetY: f32,
    pub volumeSpeedOffsetX: f32,    pub volumeSpeedOffsetY: f32,
    /// Zone 6 内 VOL 标签 X 偏移（像素）。
    pub volLabelOffsetX: f32,
    /// Zone 6 内 VOL 标签 Y 偏移（像素）。
    pub volLabelOffsetY: f32,
    /// VOL 标签 → 音量条 间距（像素）。
    pub volBarGap: f32,
    /// 音量条 → 百分比 间距（像素）。
    pub volPctGap: f32,
    /// 百分比 → SPD 段 间距（像素）。
    pub spdSectionGap: f32,
    /// SPD 标签 → 倍速按钮 间距（像素）。
    pub spdBtnGap: f32,
    pub statusBarOffsetX: f32,      pub statusBarOffsetY: f32,

    // ── 封面占位图 ──
    /// 封面波形图宽度（像素），0 = 隐藏。
    pub albumArtWidth: f32,
    /// 封面波形图高度（像素）。
    pub albumArtHeight: f32,
    /// 封面波形图 X 偏移（像素），相对自动计算位置。
    pub albumArtOffsetX: f32,
    /// 封面波形图 Y 偏移（像素），相对自动计算位置。
    pub albumArtOffsetY: f32,

    // ── 倍速预设 ──
    /// 可选倍速档位，默认 [0.5, 1.0, 1.5, 2.0]。
    pub speedPresets: Vec<f32>,
}

impl Default for MusicPlayerStyle {
    fn default() -> Self {
        Self {
            windowWidth: 500.0,
            windowHeight: 800.0,
            windowPosX: -1.0,
            windowPosY: -1.0,
            bgColor: [0.0196, 0.0275, 0.0314],    // #050708
            accentColor: [0.0, 1.0, 0.498],        // #00FF7F
            accentDimColor: [0.0, 0.349, 0.200],   // #005933
            textColor: [0.7216, 0.8471, 0.7529],   // #B8D8C0
            textDimColor: [0.3529, 0.4784, 0.4157],// #5A7A6A
            warnColor: [1.0, 0.549, 0.0],          // #FF8C00
            rowColorNormal: [0.0392, 0.0588, 0.0667],
            rowColorCurrent: [0.0196, 0.1490, 0.0980],
            rowColorHover: [0.0588, 0.0980, 0.0824],
            progressBgColor: [0.0588, 0.1020, 0.0784],
            waveformColor: [0.0, 0.549, 0.278],
            borderWidth: 1.0,
            cornerCutPx: 8.0,
            glowRadius: 12.0,
            glowAlpha: 0.15,
            scanlineEnabled: true,
            scanlineAlpha: 0.04,
            scanlineSpacing: 3.0,
            progressBarHeight: 20.0,
            progressSegments: 50,
            progressSegmentGap: 2.0,
            bottomBarHeight: 50.0,
            modeButtonSize: 50.0,
            transportButtonSize: 36.0,
            speedButtonSize: 36.0,
            playButtonScale: 1.2,
            buttonGap: 36.0,
            leftSectionWidth: 120.0,
            rightSectionWidth: 470.0,
            volumeSpeedHeight: 40.0,
            topInfoHeight: 120.0,
            topInfoWidth: 474.0,
            playlistRowHeight: 20.0,
            playlistRowOffsetX: -44.0,
            playlistRowOffsetY: 6.0,
            playlistRowSpacing: 0.0,
            playlistRowBgOffsetX: 6.0,
            playlistRowBgOffsetY: 0.0,
            playlistRowBgWidth: 0.0,
            playlistHeight: 400.0,
            playlistWidth: 470.0,
            modeBarHeight: 50.0,
            modeBarWidth: 470.0,
            statusBarHeight: 31.0,
            statusBarWidth: 470.0,
            progressBarWidth: 466.0,
            transportWidth: 466.0,
            zoneAlignment: ZoneAlignment::Left,
            zoneSpacing: 0.0,
            topPanelOffsetX: 6.0,       topPanelOffsetY: 4.0,
            modeBarOffsetX: 6.0,        modeBarOffsetY: 0.0,
            playlistOffsetX: -6.0,      playlistOffsetY: 0.0,
            progressBarOffsetX: 6.0,    progressBarOffsetY: 0.0,
            transportOffsetX: 6.0,      transportOffsetY: 0.0,
            volumeSpeedOffsetX: 6.0,    volumeSpeedOffsetY: 0.0,
            volLabelOffsetX: -26.0,     volLabelOffsetY: 0.0,
            volBarGap: 2.0,             volPctGap: 0.0,
            spdSectionGap: 20.0,        spdBtnGap: 8.0,
            statusBarOffsetX: 6.0,      statusBarOffsetY: 0.0,
            albumArtWidth: 130.0,
            albumArtHeight: 120.0,
            albumArtOffsetX: -10.0,
            albumArtOffsetY: 0.0,
            speedPresets: vec![0.5, 1.0, 1.5, 2.0],
        }
    }
}

/// 区域宽度 < 窗口宽度时的对齐方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneAlignment {
    #[default]
    Left,
    Center,
    Right,
}

/// 音乐播放器 UI 区域标识：CFG 窗口 hover 设置行时，在播放器主窗口高亮对应区域。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightZone {
    TopPanel,          // Zone 1: 封面波形 + 曲目信息
    ModeBar,           // Zone 2: 模式切换 + DIR/CFG/EXIT
    Playlist,          // Zone 3: 歌单
    ProgressBar,       // Zone 4: 分段进度条
    TransportControls, // Zone 5: 传输按钮 (prev/play/next)
    VolumeSpeed,       // Zone 6: 音量 + 倍速
    StatusBar,         // Zone 7: 状态栏
    GlobalAppearance,  // 边框/辉光/CRT/窗口尺寸
    Colors,            // 所有颜色字段
}

impl HighlightZone {
    /// 根据 MusicPlayerStyle 字段名映射到 UI 区域。
    pub fn for_setting(field_name: &str) -> Self {
        match field_name {
            "topInfoHeight" | "topInfoWidth" | "albumArtWidth" | "albumArtHeight" | "albumArtOffsetX" | "albumArtOffsetY" | "waveformColor"
            | "topPanelOffsetX" | "topPanelOffsetY" => Self::TopPanel,
            "playlistRowHeight" | "playlistRowOffsetX" | "playlistRowOffsetY" | "playlistRowSpacing" | "playlistRowBgOffsetX" | "playlistRowBgOffsetY" | "playlistRowBgWidth" | "playlistHeight" | "playlistWidth" | "rowColorNormal" | "rowColorCurrent" | "rowColorHover"
            | "playlistOffsetX" | "playlistOffsetY" => Self::Playlist,
            "modeBarHeight" | "modeBarWidth" | "modeBarOffsetX" | "modeBarOffsetY" | "modeButtonSize" => Self::ModeBar,
            "progressBarHeight" | "progressBarWidth" | "progressSegments" | "progressSegmentGap" | "progressBgColor"
            | "progressBarOffsetX" | "progressBarOffsetY" => Self::ProgressBar,
            "bottomBarHeight" | "transportWidth" | "transportButtonSize" | "playButtonScale" | "buttonGap"
            | "transportOffsetX" | "transportOffsetY" => Self::TransportControls,
            "statusBarHeight" | "statusBarWidth" | "statusBarOffsetX" | "statusBarOffsetY" => Self::StatusBar,
            "speedPresets" | "rightSectionWidth" | "volumeSpeedHeight" | "zoneAlignment" | "speedButtonSize"
            | "volumeSpeedOffsetX" | "volumeSpeedOffsetY" | "volLabelOffsetX" | "volLabelOffsetY"
            | "volBarGap" | "volPctGap" | "spdSectionGap" | "spdBtnGap" => Self::VolumeSpeed,
            "leftSectionWidth" => Self::TopPanel,
            "borderWidth" | "cornerCutPx" | "glowRadius" | "glowAlpha"
            | "scanlineEnabled" | "scanlineAlpha" | "scanlineSpacing"
            | "windowWidth" | "windowHeight" | "windowPosX" | "windowPosY"
            | "zoneSpacing" => Self::GlobalAppearance,
            "bgColor" | "accentColor" | "accentDimColor" | "textColor"
            | "textDimColor" | "warnColor" => Self::Colors,
            _ => Self::GlobalAppearance,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PetSettings {
    pub skin: String,
    pub poseName: String,
    pub initialMotion: String,
    pub stageScale: f32,
    pub dpiMode: DpiMode,
    pub fontMode: FontModeKey,
    pub penetration: PenetrationKey,
    pub actionDurations: ActionDurations,
    pub needs: crate::needs::Needs,
    pub needsConfig: NeedsConfig,
    pub inventory: crate::inventory::Inventory,
    pub bubbleStyle: BubbleStyle,
    pub barStyle: BarStyle,
    /// 仓库窗口显示缩放（0.8~1.5），整体放大/缩小圆环与格子。
    #[serde(default = "defaultInventoryScale")]
    pub inventoryScale: f32,
    /// 仓库圆环外观样式（用户可在仓库窗实时调）。
    pub inventoryStyle: InventoryStyle,
    /// 石头剪刀布按钮设置。
    pub rps: RpsConfig,
    /// 抽奖转盘窗口布局设置。
    pub wheel: WheelConfig,
    /// 石头剪刀布获胜获得的代币数量（后续用于启动转盘）。
    #[serde(default)]
    pub rpsCoins: u32,
    /// 需求表情包弹出设置。
    pub stickerConfig: StickerConfig,
    /// 开机自启动（Windows 注册表 Run 键）。
    #[serde(default)]
    pub autoStart: bool,
    /// 全屏应用检测：游戏/全屏办公时自动隐藏桌宠。
    #[serde(default = "default_true")]
    pub fullscreenHideEnabled: bool,
    /// 界面字体族名（空 = 系统 CJK 兜底）。
    #[serde(default)]
    pub uiFontFamily: String,
    /// 音乐播放器外观。
    #[serde(default)]
    pub musicPlayerStyle: MusicPlayerStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontModeKey {
    Embedded,
    System,
    Both,
}

impl Default for FontModeKey {
    fn default() -> Self {
        Self::Both
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PenetrationKey {
    Always,
    Never,
    Smart,
}

impl Default for PenetrationKey {
    fn default() -> Self {
        Self::Smart
    }
}

impl Default for PetSettings {
    fn default() -> Self {
        Self {
            skin: "exp".into(),
            poseName: "stand".into(),
            initialMotion: "idle".into(),
            stageScale: 2.0,
            dpiMode: DpiMode::default(),
            fontMode: FontModeKey::default(),
            penetration: PenetrationKey::default(),
            actionDurations: ActionDurations::default(),
            needs: Default::default(),
            needsConfig: Default::default(),
            inventory: Default::default(),
            bubbleStyle: Default::default(),
            barStyle: Default::default(),
            inventoryScale: 0.8,
            inventoryStyle: Default::default(),
            rps: Default::default(),
            wheel: Default::default(),
            rpsCoins: 0,
            stickerConfig: Default::default(),
            autoStart: false,
            fullscreenHideEnabled: true,
            uiFontFamily: String::new(),
            musicPlayerStyle: MusicPlayerStyle::default(),
        }
    }
}

impl PetSettings {
    pub fn fontModeAsEnum(&self) -> FontSourceMode {
        match self.fontMode {
            FontModeKey::Embedded => FontSourceMode::Embedded,
            FontModeKey::System => FontSourceMode::System,
            FontModeKey::Both => FontSourceMode::Both,
        }
    }
    pub fn penetrationAsEnum(&self) -> PenetrationMode {
        match self.penetration {
            PenetrationKey::Always => PenetrationMode::Always,
            PenetrationKey::Never => PenetrationMode::Never,
            PenetrationKey::Smart => PenetrationMode::Smart,
        }
    }
}

pub fn loadSettings(petConfigDir: &Path, petId: &str) -> PetSettings {
    let p = petConfigDir.join(format!("{petId}.json"));
    if !p.exists() {
        return PetSettings::default();
    }
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            let mut s = serde_json::from_str::<PetSettings>(&text).unwrap_or_default();
            s.inventory.normalize();
            s
        }
        Err(_) => PetSettings::default(),
    }
}

pub fn saveSettings(petConfigDir: &Path, petId: &str, settings: &PetSettings) -> std::io::Result<()> {
    std::fs::create_dir_all(petConfigDir)?;
    let p = petConfigDir.join(format!("{petId}.json"));
    let text = serde_json::to_string_pretty(settings).unwrap_or_default();
    std::fs::write(&p, text)
}