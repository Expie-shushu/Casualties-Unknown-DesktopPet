// 桌宠应用主循环：winit ApplicationHandler + wgpu 渲染 + 静态 stand pose 加载。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use crate::animClip::{loadClipsFromDir, AnimClip};
use crate::animController::loadController;
use crate::animPlayer::AnimPlayer;
use crate::animator::Animator;
use crate::asset::SpriteAsset;
use crate::behavior::{tick as behaviorTick, BehaviorState};
use crate::bubble::{appendBubbleDraws, say as makeBubble, Bubble};
use crate::bus::{readNeighbors, removeOwnState, writeOwnState, PetState as BusState};
use crate::cli::CliOptions;
use crate::contextMenu::{buildMenu, parseMenuId, showContextMenu, MenuAction};
use crate::hitTest::anyLimbBboxContains;
use crate::paths::{appRoot, configsDir, dataDir, skinDir};
use crate::input::{applyPenetration, InputState, PenetrationMode};
use crate::interact::{loadInteractions, tick as interactTick, InteractionState, TriggeredAction};
use crate::mood::{moodToEyeSprite, updateMood, Mood, MoodInputs};
use crate::physics::{step as physicsStep, PhysicsConfig, PhysicsState, ScreenBounds};
use crate::plugin::{PluginCmd, PluginHost};
use crate::pose::{loadPose, LimbPose, PetPose};
use crate::renderer::spritePipeline::buildSpriteMatrix;
use crate::renderer::{Renderer, SpriteDraw};
use crate::settings::{loadSettings, saveSettings, DpiMode, PetSettings};
use crate::skeleton::{limbSide, resolveSidedSprite};
use crate::tail::{tickTail, TailState};
use crate::text::TextSystem;
use crate::wings::{self, pieceBaseAngle, pieceLocalOffset, WingDynAngles, WingPiece, WingsConfig, WingsLayout, WingsState};

pub const PET_W: u32 = 360;
pub const PET_H: u32 = 360;
const PIXELS_PER_UNIT: f32 = 8.0;
const STAGE_SCALE: f32 = 4.0;
const CENTER_Y_RATIO: f32 = 0.5;
/// 头顶估算：身体中心上方约 3.5 单位 ≈ 头顶位置（气泡 / 状态条均锚定于此上方）。
const HEAD_TOP_UNITS: f32 = 3.5;
/// 脚底估算：身体中心下方约 3.5 单位 ≈ 脚底位置（状态条面板锚定于此下方）。
const FEET_BOTTOM_UNITS: f32 = 3.5;
// 状态条数字/标题字号现由 settings.barStyle 控制（见 ensureNeedsAssets）。
const TAIL_OFFSET: (f32, f32) = (-0.041, -0.479);

const CLIMB_SPEED: f32 = 70.0;
const CLIMB_MIN_DIST: f32 = 120.0;
const CLIMB_MAX_DIST: f32 = 320.0;
const CLIMB_TRIGGER_CHANCE: f32 = 0.7;
const CLIMB_DROP_PER_FRAME: f32 = 0.012;
const CLIMB_COOLDOWN_SEC: f32 = 2.0;

pub struct Scene {
    pub pose: PetPose,
    pub sprites: HashMap<String, SpriteAsset>,
    pub skinName: String,
    pub animator: Animator,
    pub bodyPlayer: Option<AnimPlayer>,
    pub armsPlayer: Option<AnimPlayer>,
    pub tail: TailState,
    pub wings: WingsState,
    /// 配饰定义表（accessories.json）：渲染已装备背包/配饰到桌宠身上。
    pub accessoryDefs: Vec<crate::accessory::AccessoryDef>,
    pub lastTickAt: std::time::Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionStage {
    None,
    Sit,
    Lay,
    Plank,
}

impl ActionStage {
    fn bodyState(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Sit => Some("ExperimentSit"),
            Self::Lay => Some("ExperimentLayDown"),
            // 趴下：用躺下变体（ExperimentPlank 实为平板支撑，不像趴）。
            Self::Plank => Some("ExperimentLayDownAlt"),
        }
    }
    fn armsState(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Sit => Some("ArmsSit"),
            Self::Lay => Some("ArmsLayDown"),
            Self::Plank => Some("ArmsLayDownAlt"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClimbState {
    None,
    Active { side: i32, climbedPx: f32 },
}

pub struct PetApp {
    pub opts: CliOptions,
    pub window: Option<Arc<Window>>,
    pub renderer: Option<Renderer>,
    pub scene: Option<Scene>,
    pub physics: PhysicsState,
    pub physicsCfg: PhysicsConfig,
    pub bounds: ScreenBounds,
    pub behavior: BehaviorState,
    pub input: InputState,
    pub penetration: PenetrationMode,
    pub pickedMotion: Option<(String, std::time::Instant)>,
    pub menu: Option<muda::Menu>,
    pub tray: Option<crate::trayIcon::TrayHandle>,
    pub settingsWindow: Option<crate::settingsWindow::SettingsWindow>,
    pub pendingOpenSettings: bool,
    pub lastTooltipStatus: String,
    pub lastTooltipAt: Option<std::time::Instant>,
    pub shouldExit: bool,
    pub textSystem: Option<TextSystem>,
    pub bubble: Option<Bubble>,
    pub bubbleBg: Option<SpriteAsset>,
    pub dpiScale: f32,
    pub stageScale: f32,
    pub settings: PetSettings,
    pub neighbors: Vec<BusState>,
    pub lastBeaconAt: Option<std::time::Instant>,
    pub interaction: Option<InteractionState>,
    pub pluginHost: Option<PluginHost>,
    pub lastRenderedLimbs: Vec<LimbPose>,
    pub lastWindowPos: (f32, f32),
    pub cursorHittest: bool,
    pub lastSmartCheckAt: Option<std::time::Instant>,
    pub idleTimeSec: f32,
    /// 上次实际 playState 的休息姿势：仅在 actionStage 变化沿触发一次播放，
    /// 避免每帧把已 transition 走的 state 拉回重播（Lay 播完会自动返回 → 鬼畜）。
    pub lastPlayedStage: Option<ActionStage>,
    pub mood: Mood,
    pub actionStage: ActionStage,
    pub actionStageTimer: f32,
    pub climb: ClimbState,
    pub lastClimbEndAt: Option<std::time::Instant>,
    // ── 三系统（心情 / 饥饿 / 口渴）─────────────────────────────
    /// 状态条悬停淡入淡出当前不透明度。
    pub needsBarAlpha: f32,
    /// 极低值强制浮现剩余时间（秒）。
    pub needsForceShowSec: f32,
    /// 喂食开始时刻（用于计算已过时间，驱动张嘴→微张嘴切换）。
    pub feedingStartedAt: Option<std::time::Instant>,
    /// 喂食后强制开心表情的截止时刻。
    pub feedingHappyUntil: Option<std::time::Instant>,
    /// needs 节流写盘计时。
    pub needsSaveTimer: f32,
    /// 低值气泡冷却计时（饥饿 / 口渴 / 心情）。
    pub bubbleCooldown: [f32; 3],
    /// 纯白 1×1 sprite，用于状态条着色复用。
    pub solidWhite: Option<SpriteAsset>,
    /// 状态条左侧表情图标（心情/饥饿/口渴），从 desktopPet/needs/<key>.png 懒加载。
    pub needsIcons: [Option<SpriteAsset>; 3],
    /// 0~9 数字字形缓存，用于状态条数值显示。
    pub digitSprites: [Option<SpriteAsset>; 10],
    /// 百分号字形（与数字同字号），拼在数值末尾。
    pub percentSprite: Option<SpriteAsset>,
    /// 行标题字形（心情值 / 饥饿值 / 口渴值），各合成为一张 sprite。
    pub titleSprites: [Option<SpriteAsset>; 3],
    /// 爱心格 PNG（desktopPet/needs/heart.png，可选）：心情条用其形状染色填充；缺图回退方格。
    pub heartSprite: Option<SpriteAsset>,
    /// 状态条图标 / 数字资源是否已加载（文本系统+渲染器就绪后首帧加载）。
    pub needsAssetsLoaded: bool,
    /// 仓库窗口。
    pub inventoryWindow: Option<crate::inventoryWindow::InventoryWindow>,
    pub pendingOpenInventory: bool,
    /// 喂食拖拽状态机 + 跟随小窗。
    pub feedDrag: crate::feedDrag::FeedDragState,
    pub feedDragWindow: Option<crate::feedDrag::FeedDragWindow>,
    /// 喂食投放：上一帧全局左键是否按下（用于检测松手边沿）。
    pub feedDragMouseWasDown: bool,
    /// 上次拖拽渲染时间，用于 ~60fps 节流。
    lastFeedDragRender: std::time::Instant,
    /// 拖出物品的来源格，未命中投放时归还。
    pub feedSource: Option<FeedSource>,
    /// 正在拖拽的物品 kind，结算时区分喂食/使用。
    pub feedDragItemKind: Option<crate::item::ItemKind>,
    /// 随机闲聊台词池（desktopPet/configs/chatter.json）。
    pub chatter: crate::chatter::ChatterConfig,
    /// 距下次闲聊的剩余秒数；到 0 时随机说一句并重置。
    pub chatterTimer: f32,
    /// 气泡静默截止时刻：此刻之前不出「自动气泡」（需求/闲聊）。
    /// 每个气泡显示 3s + 间隔 3s，故 = 气泡结束时刻 + 3s。交互气泡无视此限可抢占。
    pub bubbleBlockedUntil: std::time::Instant,
    /// 屏幕上所有待拾取掉落物（上限 8）。
    pub droppedItems: Vec<crate::dropItem::DroppedItem>,
    /// 右键按下状态（上帧），用于检测按下/松开沿。
    pub dropRightWasDown: bool,
    /// 待生成掉落物队列（避免 el 线程问题，在 tick 里排空）。
    pub pendingDrops: Vec<crate::item::Item>,
    /// 掉落物落体计时基准（上次 tick 时间），用于算落体 dt。
    pub lastDropTickAt: std::time::Instant,
    /// 石头剪刀布游戏窗口。
    pub rpsWindow: Option<crate::rpsGame::RpsWindow>,
    /// 上次给 RPS 按钮窗定位时的桌宠位置；仅当桌宠移动时才重定位，避免高帧率下狂调 SetWindowPos 卡死 DWM。
    pub rpsLastPos: Option<(f32, f32)>,
    /// 右键菜单触发后在 about_to_wait 里安全创建 rpsWindow。
    pub pendingOpenGame: bool,
    /// 抽奖转盘窗口。
    pub wheelWindow: Option<crate::rewardWheel::RewardWheelWindow>,
    /// 上次给抽奖转盘窗口定位时的桌宠位置。
    pub wheelLastPos: Option<(f32, f32)>,
    /// 右键菜单触发后安全创建 wheelWindow。
    pub pendingOpenWheel: bool,
    /// 表情包管理器：idle 随机弹出 / 游戏结果弹出。
    pub stickerManager: crate::sticker::StickerManager,
    /// 表情包 tick 节流计时基准。
    lastStickerTickAt: std::time::Instant,
    /// 耳机配饰是否显示。
    pub headsetEquipped: bool,
    /// 音乐播放中。
    pub musicPlaying: bool,
    /// 扫描到的音乐文件列表。
    pub musicFiles: Vec<crate::music::MusicFile>,
    /// 音乐播放器（rodio）。
    pub musicPlayer: Option<crate::music::MusicPlayer>,
    /// 音乐播放器窗口。
    pub musicPlayerWindow: Option<crate::musicPlayerWindow::MusicPlayerWindow>,
    pub pendingOpenMusicPlayer: bool,
    /// 音乐播放器配置窗口。
    pub musicPlayerCfgWindow: Option<crate::musicPlayerCfgWindow::MusicPlayerCfgWindow>,
    pub pendingOpenMusicPlayerCfg: bool,
    /// 抽奖转盘配置窗口。
    pub wheelCfgWindow: Option<crate::wheelCfgWindow::WheelCfgWindow>,
    pub pendingOpenWheelCfg: bool,
    /// 全屏检测：因全屏应用而隐藏桌宠。
    fullscreenHidden: bool,
    lastFullscreenCheckAt: Option<std::time::Instant>,
}

/// 拖出物品的来源格，未命中投放时归还。
#[derive(Clone, Debug)]
pub enum FeedSource {
    Main(usize),
    Pack(String, usize),
}

/// 由拖拽中记录的 id + kind 重建 Item。
fn itemFromIdKind(id: &str, kind: Option<crate::item::ItemKind>) -> crate::item::Item {
    match kind {
        Some(crate::item::ItemKind::Backpack) => crate::item::Item::backpack(id),
        Some(crate::item::ItemKind::Accessory) => crate::item::Item::accessory(id),
        _ => crate::item::Item::food(id),
    }
}

impl PetApp {
    pub fn new(opts: CliOptions) -> Self {
        Self {
            opts,
            window: None,
            renderer: None,
            scene: None,
            physics: PhysicsState::new(200.0, 200.0),
            physicsCfg: PhysicsConfig::default(),
            bounds: ScreenBounds {
                minX: 0.0,
                maxX: 1920.0,
                groundY: 1000.0,
            },
            behavior: BehaviorState::default(),
            input: InputState::default(),
            penetration: PenetrationMode::Never,
            pickedMotion: None,
            menu: None,
            tray: None,
            settingsWindow: None,
            pendingOpenSettings: false,
            lastTooltipStatus: String::new(),
            lastTooltipAt: None,
            shouldExit: false,
            textSystem: None,
            bubble: None,
            bubbleBg: None,
            dpiScale: 1.0,
            stageScale: STAGE_SCALE,
            settings: PetSettings::default(),
            neighbors: Vec::new(),
            lastBeaconAt: None,
            interaction: None,
            pluginHost: None,
            lastRenderedLimbs: Vec::new(),
            lastWindowPos: (-1.0e9, -1.0e9),
            cursorHittest: true,
            lastSmartCheckAt: None,
            idleTimeSec: 0.0,
            lastPlayedStage: None,
            mood: Mood::Neutral,
            actionStage: ActionStage::None,
            actionStageTimer: 0.0,
            climb: ClimbState::None,
            lastClimbEndAt: None,
            needsBarAlpha: 0.0,
            needsForceShowSec: 0.0,
            feedingStartedAt: None,
            feedingHappyUntil: None,
            needsSaveTimer: 0.0,
            bubbleCooldown: [0.0; 3],
            solidWhite: None,
            needsIcons: [None, None, None],
            digitSprites: Default::default(),
            percentSprite: None,
            titleSprites: [None, None, None],
            heartSprite: None,
            needsAssetsLoaded: false,
            inventoryWindow: None,
            pendingOpenInventory: false,
            feedDrag: crate::feedDrag::FeedDragState::default(),
            feedDragWindow: None,
            feedDragMouseWasDown: false,
            lastFeedDragRender: std::time::Instant::now(),
            feedSource: None,
            feedDragItemKind: None,
            chatter: crate::chatter::ChatterConfig::default(),
            chatterTimer: 20.0,
            bubbleBlockedUntil: std::time::Instant::now(),
            droppedItems: Vec::new(),
            dropRightWasDown: false,
            pendingDrops: Vec::new(),
            lastDropTickAt: std::time::Instant::now(),
            rpsWindow: None,
            rpsLastPos: None,
            pendingOpenGame: false,
            wheelWindow: None,
            wheelLastPos: None,
            pendingOpenWheel: false,
            stickerManager: crate::sticker::StickerManager::new(),
            lastStickerTickAt: std::time::Instant::now(),
            headsetEquipped: false,
            musicPlaying: false,
            musicFiles: Vec::new(),
            musicPlayer: None,
            musicPlayerWindow: None,
            pendingOpenMusicPlayer: false,
            musicPlayerCfgWindow: None,
            pendingOpenMusicPlayerCfg: false,
            wheelCfgWindow: None,
            pendingOpenWheelCfg: false,
            fullscreenHidden: false,
            lastFullscreenCheckAt: None,
        }
    }

    fn loadScene(&mut self) {
        let renderer = match self.renderer.as_ref() {
            Some(r) => r,
            None => return,
        };
        let root = appRoot(self.opts.configRoot.as_deref());
        let skinName = &self.settings.skin;
        let dir = skinDir(&root, skinName);
        let mut sprites = match renderer.loadSpritesFromDir(&dir) {
            Ok(s) => s,
            Err(e) => {
                log::error!("load skin {} failed: {e:?}", dir.display());
                return;
            }
        };
        // 配饰贴图从 data/Accessories/ 加载（跨皮肤共用）
        let accDir = dataDir(&root).join("Accessories");
        if let Ok(accSprites) = renderer.loadSpritesFromDir(&accDir) {
            sprites.extend(accSprites);
        }
        let pose = match loadPose(&root, "stand") {
            Ok(p) => p,
            Err(e) => {
                log::error!("load pose stand failed: {e:?}");
                return;
            }
        };
        log::info!("loaded {} sprites + pose limbs={}", sprites.len(), pose.limbs.len());
        let animator = Animator::fromPose(&pose, "idle");
        self.physics.facing = 1;
        let animDir = root.join("desktopPet").join("anim");
        let clipsDir = animDir.join("clips");
        let clipsByName = loadClipsFromDir(&clipsDir).unwrap_or_default();
        let bodyPlayer = makePlayer(&animDir.join("Anim.controller.json"), &clipsByName);
        let armsPlayer = makePlayer(&animDir.join("Arms.controller.json"), &clipsByName);
        let accessoryDefs = crate::accessory::loadAccessoryDefs(&root);
        log::info!("loaded {} accessory defs", accessoryDefs.len());
        self.scene = Some(Scene {
            pose,
            sprites,
            skinName: skinName.into(),
            animator,
            bodyPlayer,
            armsPlayer,
            tail: TailState::default(),
            wings: WingsState::default(),
            accessoryDefs,
            lastTickAt: std::time::Instant::now(),
        });
    }

    fn buildDrawsFrom<'a>(
        sprites: &'a HashMap<String, SpriteAsset>,
        limbs: &'a [LimbPose],
        isRight: bool,
        screenW: f32,
        screenH: f32,
        unitToPx: f32,
        eyeSpriteName: &str,
        feedingMouthHead: Option<&str>,
    ) -> Vec<SpriteDraw<'a>> {
        let centerX = screenW * 0.5;
        let centerY = screenH * CENTER_Y_RATIO;
        let facingSign = if isRight { 1.0 } else { -1.0 };
        let pxPerSpritePxRatio = unitToPx / PIXELS_PER_UNIT;
        let mut sorted: Vec<&LimbPose> = limbs.iter().filter(|l| l.visible).collect();
        sorted.sort_by_key(|l| l.sortingOrder);
        let mut out: Vec<SpriteDraw<'a>> = Vec::with_capacity(sorted.len() + 8);
        for limb in sorted {
            let side = limbSide(&limb.name);
            // 头部 sprite 选择优先级：喂食张嘴 > 高兴回头(headback) > pose 默认头(head)
            let spriteName: &str = if limb.name == "Head" {
                if let Some(mh) = feedingMouthHead {
                    mh
                } else if eyeSpriteName == "experimentEyeHappy" {
                    "experimentHeadBack"
                } else {
                    &limb.spriteName
                }
            } else {
                &limb.spriteName
            };
            let asset = match resolveSidedSprite(sprites, spriteName, side, isRight) {
                Some(a) => a,
                None => continue,
            };
            let cx = centerX + limb.px * unitToPx * facingSign;
            let cy = centerY - limb.py * unitToPx;
            let sw = asset.width as f32 * pxPerSpritePxRatio * limb.scaleX;
            let sh = asset.height as f32 * pxPerSpritePxRatio * limb.scaleY;
            let rotDeg = if isRight { limb.rotZ } else { -limb.rotZ };
            let m = buildSpriteMatrix(screenW, screenH, cx, cy, sw, sh, rotDeg, facingSign);
            out.push(SpriteDraw {
                asset,
                matrix: m,
                color: [1.0, 1.0, 1.0, 1.0],
                uvRect: [0.0, 0.0, 1.0, 1.0],
            });
            if limb.name == "Head" {
                if let Some(eye) = sprites.get(eyeSpriteName).or_else(|| sprites.get("experimentEyeOpen")) {
                    let ew = eye.width as f32 * pxPerSpritePxRatio;
                    let eh = eye.height as f32 * pxPerSpritePxRatio;
                    let mEye = buildSpriteMatrix(screenW, screenH, cx, cy, ew, eh, rotDeg, facingSign);
                    out.push(SpriteDraw {
                        asset: eye,
                        matrix: mEye,
                        color: [1.0, 1.0, 1.0, 1.0],
                        uvRect: [0.0, 0.0, 1.0, 1.0],
                    });
                }
            }
        }
        out
    }

    fn appendWingDraws<'a>(
        sprites: &'a HashMap<String, SpriteAsset>,
        limbs: &'a [LimbPose],
        dyns: &WingDynAngles,
        layout: WingsLayout,
        isRight: bool,
        screenW: f32,
        screenH: f32,
        unitToPx: f32,
        out: &mut Vec<SpriteDraw<'a>>,
    ) {
        let upTorso = match limbs.iter().find(|l| l.name == "UpTorso") {
            Some(l) => l,
            None => return,
        };
        let pxPerSpritePxRatio = unitToPx / PIXELS_PER_UNIT;
        let parentUnitX = upTorso.px;
        let parentUnitY = upTorso.py;
        let parentUnitRot = upTorso.rotZ;

        let upperUL = sprites.get("wingUL");
        let upperUR = sprites.get("wingUR");
        let lowerDL = sprites.get("wingDL");
        let lowerDR = sprites.get("wingDR");

        let upperHeightUL = upperUL.map(|a| a.height as f32).unwrap_or(32.0);
        let upperHeightUR = upperUR.map(|a| a.height as f32).unwrap_or(32.0);

        let upperUlPos = computeWingPose(layout.wingUL, false, 0.0, parentUnitX, parentUnitY, parentUnitRot, dyns.wingUL);
        let upperUrPos = computeWingPose(layout.wingUR, false, 0.0, parentUnitX, parentUnitY, parentUnitRot, dyns.wingUR);
        let lowerDlPos = computeWingPose(layout.wingDL, true, upperHeightUL, upperUlPos.0, upperUlPos.1, upperUlPos.2, dyns.wingDL);
        let lowerDrPos = computeWingPose(layout.wingDR, true, upperHeightUR, upperUrPos.0, upperUrPos.1, upperUrPos.2, dyns.wingDR);

        let mut entries: Vec<(i32, &SpriteAsset, (f32, f32, f32))> = Vec::with_capacity(4);
        if let Some(a) = upperUL { entries.push((layout.wingUL.zOrder, a, upperUlPos)); }
        if let Some(a) = upperUR { entries.push((layout.wingUR.zOrder, a, upperUrPos)); }
        if let Some(a) = lowerDL { entries.push((layout.wingDL.zOrder, a, lowerDlPos)); }
        if let Some(a) = lowerDR { entries.push((layout.wingDR.zOrder, a, lowerDrPos)); }
        entries.sort_by_key(|e| e.0);

        let centerX = screenW * 0.5;
        let centerY = screenH * CENTER_Y_RATIO;
        let facingSign = if isRight { 1.0 } else { -1.0 };
        for (_, asset, (unitX, unitY, unitRot)) in entries {
            let cx = centerX + unitX * unitToPx * facingSign;
            let cy = centerY - unitY * unitToPx;
            let rotDeg = if isRight { unitRot } else { -unitRot };
            let sw = asset.width as f32 * pxPerSpritePxRatio;
            let sh = asset.height as f32 * pxPerSpritePxRatio;
            let m = buildSpriteMatrix(screenW, screenH, cx, cy, sw, sh, rotDeg, facingSign);
            out.push(SpriteDraw {
                asset,
                matrix: m,
                color: [1.0, 1.0, 1.0, 1.0],
                uvRect: [0.0, 0.0, 1.0, 1.0],
            });
        }
    }

    /// 把已装备的背包/配饰叠加渲染到桌宠身上：按配饰 def 找父肢体 posed 变换，
    /// FK 合成 offX/offY/rot，画到对应世界位置。本期只渲染 equipped 背包。
    #[allow(clippy::too_many_arguments)]
    fn appendAccessoryDraws<'a>(
        sprites: &'a HashMap<String, SpriteAsset>,
        limbs: &'a [LimbPose],
        defs: &'a [crate::accessory::AccessoryDef],
        inventory: &crate::inventory::Inventory,
        headsetEquipped: bool,
        isRight: bool,
        screenW: f32,
        screenH: f32,
        unitToPx: f32,
        out: &mut Vec<SpriteDraw<'a>>,
    ) {
        // 收集要渲染的配饰 id：已装备的背包 + 手动开关的耳机。
        let mut ids: Vec<&str> = Vec::new();
        for bp in &inventory.backpacks {
            if bp.equipped {
                ids.push(bp.id.as_str());
            }
        }
        if headsetEquipped {
            ids.push("headset");
        }
        if ids.is_empty() {
            return;
        }

        let centerX = screenW * 0.5;
        let centerY = screenH * CENTER_Y_RATIO;
        let facingSign = if isRight { 1.0 } else { -1.0 };
        let pxPerSpritePxRatio = unitToPx / PIXELS_PER_UNIT;

        // 收集 (排序值, asset, 世界位姿) 后统一按排序值绘制。
        let mut entries: Vec<(i32, &SpriteAsset, (f32, f32, f32))> = Vec::new();
        for id in ids {
            let def = match crate::accessory::accessoryById(defs, id) {
                Some(d) => d,
                None => continue,
            };
            let asset = match sprites.get(&def.sprite) {
                Some(a) => a,
                None => continue,
            };
            let parent = match limbs.iter().find(|l| l.name == def.limb) {
                Some(l) => l,
                None => continue,
            };
            // FK：父肢体局部偏移 (offX,offY) 经父 rotZ 旋转后加到父世界位置。
            let rad = parent.rotZ.to_radians();
            let (c, s) = (rad.cos(), rad.sin());
            let unitX = parent.px + c * def.offX - s * def.offY;
            let unitY = parent.py + s * def.offX + c * def.offY;
            let unitRot = parent.rotZ + def.rot;
            let order = parent.sortingOrder + def.z;
            entries.push((order, asset, (unitX, unitY, unitRot)));
        }
        entries.sort_by_key(|e| e.0);

        for (_, asset, (unitX, unitY, unitRot)) in entries {
            let cx = centerX + unitX * unitToPx * facingSign;
            let cy = centerY - unitY * unitToPx;
            let rotDeg = if isRight { unitRot } else { -unitRot };
            let sw = asset.width as f32 * pxPerSpritePxRatio;
            let sh = asset.height as f32 * pxPerSpritePxRatio;
            let m = buildSpriteMatrix(screenW, screenH, cx, cy, sw, sh, rotDeg, facingSign);
            out.push(SpriteDraw {
                asset,
                matrix: m,
                color: [1.0, 1.0, 1.0, 1.0],
                uvRect: [0.0, 0.0, 1.0, 1.0],
            });
        }
    }

    fn initBoundsAndPosition(&mut self, window: &Window) {
        if let Ok(p) = window.outer_position() {
            self.physics.x = p.x as f32;
            self.physics.y = p.y as f32;
        }
        let monitor = window.current_monitor().or_else(|| window.primary_monitor());
        if let Some(monitor) = monitor {
            let sf = monitor.scale_factor() as f32;
            let size = monitor.size();
            let w = size.width as f32 / sf;
            let h = size.height as f32 / sf;
            self.bounds.minX = 0.0;
            self.bounds.maxX = w;
            let footBottomInWindow =
                CENTER_Y_RATIO * self.physicsCfg.windowH + 2.9 * PIXELS_PER_UNIT * self.stageScale;
            self.bounds.groundY = h - 60.0 - footBottomInWindow;
            self.physics.y = self.bounds.groundY;
            self.physics.grounded = true;
            let _ = window.set_outer_position(winit::dpi::LogicalPosition::new(
                self.physics.x,
                self.physics.y,
            ));
        }
    }

    fn initTextSystem(&mut self) {
        let renderer = match self.renderer.as_ref() {
            Some(r) => r,
            None => return,
        };
        let root = appRoot(self.opts.configRoot.as_deref());
        let fontDir = root.join("desktopPet").join("fonts");
        let mode = if self.opts.forceSystemFont {
            crate::text::FontSourceMode::System
        } else {
            self.settings.fontModeAsEnum()
        };
        self.textSystem = Some(TextSystem::new(mode, Some(&fontDir)));
        if self.bubbleBg.is_none() {
            match renderer.createSolidSprite([255, 255, 255, 255]) {
                Ok(a) => self.bubbleBg = Some(a),
                Err(e) => log::warn!("create bubble bg failed: {e:?}"),
            }
        }
        if self.solidWhite.is_none() {
            match renderer.createSolidSprite([255, 255, 255, 255]) {
                Ok(a) => self.solidWhite = Some(a),
                Err(e) => log::warn!("create solid white failed: {e:?}"),
            }
        }
    }

    /// 气泡固定显示时长（毫秒）。所有气泡统一 3s，之后再静默 3s 才允许下一个自动气泡。
    const BUBBLE_SHOW_MS: u64 = 3000;
    /// 气泡结束后的静默间隔（毫秒）：自动气泡需等这段后才再出。
    const BUBBLE_GAP_MS: u64 = 3000;

    /// 显示气泡（交互气泡入口：立即抢占当前气泡）。统一显示 3s，并设静默期。
    /// `_durationMs` 已废弃（统一 3s），保留参数避免大量调用点改动。
    fn sayText(&mut self, text: &str, _durationMs: u64) {
        self.showBubble(text);
    }

    /// 实际显示气泡：渲染 + 刷新静默截止（结束后再加 GAP 才允许自动气泡）。
    fn showBubble(&mut self, text: &str) {
        // 先取出气泡样式（family 需 clone、size 复制），避免与 textSystem/renderer 的借用冲突。
        let family = self.settings.bubbleStyle.fontFamily.clone();
        let sizePx = self.settings.bubbleStyle.fontSizePx;
        // 口腔含物 → 口齿不清：对全部气泡生效。
        let shown = if self.settings.inventory.mouthOccupied() {
            crate::garble::garble(text)
        } else {
            text.to_string()
        };
        if let (Some(ts), Some(r)) = (self.textSystem.as_mut(), self.renderer.as_ref()) {
            let factory = r.factory();
            if let Some(b) = makeBubble(&factory, ts, &shown, Self::BUBBLE_SHOW_MS, &family, sizePx) {
                self.bubble = Some(b);
                // 静默到「气泡结束 + 间隔」之后，自动气泡才允许再出。
                self.bubbleBlockedUntil = std::time::Instant::now()
                    + std::time::Duration::from_millis(Self::BUBBLE_SHOW_MS + Self::BUBBLE_GAP_MS);
            }
        }
    }

    /// 首帧懒加载状态条资源：数字字形（0~9）+ 表情图标（desktopPet/needs/<key>.png）。
    /// 文本系统与渲染器就绪后调用；缺图静默跳过（渲染时回退占位）。
    fn ensureNeedsAssets(&mut self) {
        if self.needsAssetsLoaded {
            return;
        }
        if self.textSystem.is_none() || self.renderer.is_none() {
            return; // 资源未就绪，下帧再试。
        }
        let mut digits: [Option<SpriteAsset>; 10] = Default::default();
        let mut icons: [Option<SpriteAsset>; 3] = [None, None, None];
        let mut titles: [Option<SpriteAsset>; 3] = [None, None, None];
        let percent: Option<SpriteAsset>;
        let mut heart: Option<SpriteAsset> = None;
        let root = appRoot(self.opts.configRoot.as_deref());
        let needsDir = root.join("desktopPet").join("needs");
        // 状态条字体样式（family clone、字号复制），避免与 textSystem/renderer 借用冲突。
        let barFamily = self.settings.barStyle.fontFamily.clone();
        let titlePx = self.settings.barStyle.titleSizePx;
        let digitPx = self.settings.barStyle.digitSizePx;
        {
            let ts = self.textSystem.as_mut().unwrap();
            let r = self.renderer.as_ref().unwrap();
            let factory = r.factory();
            // 数字 0~9：各栅格化一次，上传为 sprite 缓存。
            for d in 0u8..10 {
                let ch = (b'0' + d) as char;
                let glyphs = crate::text::rasterizeLine(ts, &ch.to_string(), digitPx, &barFamily);
                if let Some(g) = glyphs.into_iter().next() {
                    match factory.fromRgba(&g.rgba, g.widthPx, g.heightPx, "needs-digit") {
                        Ok(a) => digits[d as usize] = Some(a),
                        Err(e) => log::warn!("needs digit {d} upload failed: {e:?}"),
                    }
                }
            }
            // 百分号（与数字同字号）。
            percent = rasterizeComposite(ts, &factory, "%", digitPx, &barFamily, "needs-percent");
            // 行标题：心情值 / 饥饿值 / 口渴值，各合成为一张 sprite。
            for (i, label) in ["心情值", "饥饿值", "口渴值"].iter().enumerate() {
                titles[i] = rasterizeComposite(ts, &factory, label, titlePx, &barFamily, "needs-title");
            }
            // 表情图标：心情/饥饿/口渴。
            for (i, key) in ["mood", "hunger", "thirst"].iter().enumerate() {
                let p = needsDir.join(format!("{key}.png"));
                if let Ok(bytes) = std::fs::read(&p) {
                    match factory.fromPng(&bytes, key) {
                        Ok(a) => icons[i] = Some(a),
                        Err(e) => log::warn!("needs icon {key} decode failed: {e:?}"),
                    }
                }
            }
            // 可选爱心格 PNG（心情条用）。
            let heartPath = needsDir.join("heart.png");
            if let Ok(bytes) = std::fs::read(&heartPath) {
                match factory.fromPng(&bytes, "heart") {
                    Ok(a) => heart = Some(a),
                    Err(e) => log::warn!("needs heart decode failed: {e:?}"),
                }
            }
        }
        self.digitSprites = digits;
        self.needsIcons = icons;
        self.percentSprite = percent;
        self.titleSprites = titles;
        self.heartSprite = heart;
        self.needsAssetsLoaded = true;
    }
}

/// 把一行文本（可含 CJK）栅格化并合成为单张 sprite（白色掩码，绘制时再染色）。
/// glyph 为白色掩码，合成时取各像素的最大 alpha 即可。返回 None 表示空串/上传失败。
fn rasterizeComposite(
    ts: &mut crate::text::TextSystem,
    factory: &crate::asset::SpriteFactory<'_>,
    text: &str,
    sizePx: f32,
    family: &str,
    label: &str,
) -> Option<SpriteAsset> {
    let glyphs = crate::text::rasterizeLine(ts, text, sizePx, family);
    if glyphs.is_empty() {
        return None;
    }
    let (mut minX, mut minY) = (f32::MAX, f32::MAX);
    let (mut maxX, mut maxY) = (f32::MIN, f32::MIN);
    for g in &glyphs {
        minX = minX.min(g.xPx);
        minY = minY.min(g.yPx);
        maxX = maxX.max(g.xPx + g.widthPx as f32);
        maxY = maxY.max(g.yPx + g.heightPx as f32);
    }
    let originX = minX.floor() as i32;
    let originY = minY.floor() as i32;
    let w = (maxX.ceil() as i32 - originX).max(1) as usize;
    let h = (maxY.ceil() as i32 - originY).max(1) as usize;
    let mut buf = vec![0u8; w * h * 4];
    for g in &glyphs {
        let dx = g.xPx.round() as i32 - originX;
        let dy = g.yPx.round() as i32 - originY;
        for row in 0..g.heightPx as i32 {
            let ty = dy + row;
            if ty < 0 || ty as usize >= h {
                continue;
            }
            for col in 0..g.widthPx as i32 {
                let tx = dx + col;
                if tx < 0 || tx as usize >= w {
                    continue;
                }
                let sa = g.rgba[((row * g.widthPx as i32 + col) * 4 + 3) as usize];
                if sa == 0 {
                    continue;
                }
                let di = (ty as usize * w + tx as usize) * 4;
                buf[di] = 255;
                buf[di + 1] = 255;
                buf[di + 2] = 255;
                if sa > buf[di + 3] {
                    buf[di + 3] = sa;
                }
            }
        }
    }
    match factory.fromRgba(&buf, w as u32, h as u32, label) {
        Ok(a) => Some(a),
        Err(e) => {
            log::warn!("composite '{label}' upload failed: {e:?}");
            None
        }
    }
}

impl ApplicationHandler for PetApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = buildWindowAttributes("Casualties Unknown：desktopPet");
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("create window failed: {e}");
                el.exit();
                return;
            }
        };
        let _ = window.set_cursor_hittest(true);
        applyPlatformPostCreate(&window);
        match Renderer::new(window.clone()) {
            Ok(r) => self.renderer = Some(r),
            Err(e) => {
                log::error!("renderer init failed: {e:?}");
                el.exit();
                return;
            }
        }
        // 首帧清屏（透明）后立即显示，消除加载期间的白块。
        if let Some(r) = self.renderer.as_mut() {
            let _ = r.render();
            let _ = window.set_visible(true);
        }
        window.request_redraw();
        self.window = Some(window.clone());
        self.loadScene();
        self.initBoundsAndPosition(&window);
        applyPenetration(&window, self.penetration);
        let root = appRoot(self.opts.configRoot.as_deref());
        let cfgDir = configsDir(&root);
        self.settings = loadSettings(&cfgDir, &self.opts.petId);
        // 启动即检查背包时限：关机期间过期的背包立刻移除（真实时间计时）。
        let expired = self.settings.inventory.expireBackpacks(nowUnixMs());
        if !expired.is_empty() {
            log::info!("启动时背包过期移除: {expired:?}");
            let _ = saveSettings(&cfgDir, &self.opts.petId, &self.settings);
        }
        // 启动时同步注册表自启动状态（exe 路径可能变了，或用户手动清理了注册表）。
        applyAutoStart(self.settings.autoStart);
        self.chatter = crate::chatter::loadChatter(&cfgDir);
        self.musicFiles = crate::music::scanMusicFiles(&crate::paths::musicDir(&root));
        self.musicPlayer = crate::music::MusicPlayer::try_new().ok();
        if self.musicPlayer.is_some() {
            log::info!("music: player initialized");
        } else {
            log::warn!("music: no audio device available");
        }
        self.stageScale = self.settings.stageScale;
        self.penetration = self.settings.penetrationAsEnum();
        applyPenetration(&window, self.penetration);
        self.dpiScale = window.scale_factor() as f32;
        self.applyDpiMode();
        match buildMenu() {
            Ok(m) => self.menu = Some(m),
            Err(e) => log::warn!("build context menu failed: {e:?}"),
        }
        match crate::trayIcon::createTray(&self.opts.petId) {
            Ok(t) => self.tray = Some(t),
            Err(e) => log::warn!("tray icon init failed: {e:?}"),
        }
        self.initTextSystem();
        // 表情包加载（依赖 renderer→factory 上传纹理，需在 renderer 就绪后）。
        {
            let root = appRoot(self.opts.configRoot.as_deref());
            let stickersDir = crate::paths::stickersDir(&root);
            if let Some(r) = self.renderer.as_ref() {
                let factory = r.factory();
                self.stickerManager.loadAndReplace(&stickersDir, &factory);
            }
        }
        match PluginHost::new() {
            Ok(host) => self.pluginHost = Some(host),
            Err(e) => log::warn!("plugin host init failed: {e:?}"),
        }
        log::info!("window + renderer + scene ready");
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(sw) = self.settingsWindow.as_mut() {
            if sw.id() == _id {
                let _ = sw.handleEvent(&event);
                if sw.pendingClose() {
                    self.settingsWindow = None;
                }
                return;
            }
        }
        if let Some(iw) = self.inventoryWindow.as_mut() {
            if iw.id() == _id {
                let _ = iw.handleEvent(&event);
                if iw.pendingClose() {
                    self.inventoryWindow = None;
                }
                return;
            }
        }
        // 石头剪刀布游戏窗口事件路由。
        if let Some(rw) = self.rpsWindow.as_mut() {
            if rw.id() == _id {
                let _ = rw.handleEvent(&event);
                if rw.pendingClose() {
                    self.rpsWindow = None;
                }
                return;
            }
        }
        // 抽奖转盘窗口事件路由。
        if let Some(ww) = self.wheelWindow.as_mut() {
            if ww.id() == _id {
                let _ = ww.handleEvent(&event);
                if ww.pendingClose() {
                    self.wheelWindow = None;
                    self.wheelLastPos = None;
                }
                return;
            }
        }
        // 音乐播放器窗口事件路由。
        if let Some(mw) = self.musicPlayerWindow.as_mut() {
            if mw.id() == _id {
                let _ = mw.handleEvent(&event);
                if mw.pendingClose() {
                    self.musicPlayerWindow = None;
                }
                return;
            }
        }
        // 音乐播放器配置窗口事件路由。
        if let Some(cw) = self.musicPlayerCfgWindow.as_mut() {
            if cw.id() == _id {
                let _ = cw.handleEvent(&event);
                if cw.pendingClose() {
                    if let Some(mw) = self.musicPlayerWindow.as_mut() {
                        mw.setHighlight(None);
                    }
                    self.musicPlayerCfgWindow = None;
                }
                return;
            }
        }
        // 转盘配置窗口事件路由。
        if let Some(cw) = self.wheelCfgWindow.as_mut() {
            if cw.id() == _id {
                let _ = cw.handleEvent(&event);
                if cw.pendingClose() {
                    self.wheelCfgWindow = None;
                }
                return;
            }
        }
        // 喂食跟随小窗：穿透 + 无交互，仅吞掉自身重绘 / 尺寸事件，不参与主窗逻辑。
        if let Some(fw) = self.feedDragWindow.as_ref() {
            if fw.id() == _id {
                return;
            }
        }
        // 掉落物窗：set_cursor_hittest(true) 会捕获落在它上面的鼠标事件（含右键），
        // 但掉落物的拖动/结算完全由 about_to_wait 的 tickDroppedItems 全局轮询驱动。
        // 必须在此吞掉它的所有 winit 事件并 return，否则右键会穿透到下方主窗 default 分支，
        // 误触发 showContextMenu 打开桌宠右键菜单（并因模态循环卡住动画）。渲染靠 tick/spawn。
        if self.droppedItems.iter().any(|d| d.id() == _id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.dpiScale = scale_factor as f32;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if matches!(button, winit::event::MouseButton::Left) {
                    match state {
                        winit::event::ElementState::Pressed => {
                            if self.hitTestCursor() {
                                let isDouble = self.input.onLeftPress();
                                if isDouble {
                                    self.pickedMotion = Some((
                                        "paw".into(),
                                        std::time::Instant::now() + std::time::Duration::from_millis(1200),
                                    ));
                                    if let Some(line) =
                                        self.chatter.pickGreeting(crate::behavior::rand01())
                                    {
                                        let line = line.to_string();
                                        self.sayText(&line, 2000);
                                    }
                                }
                            }
                        }
                        winit::event::ElementState::Released => self.input.onLeftRelease(),
                    }
                } else if matches!(button, winit::event::MouseButton::Right)
                    && matches!(state, winit::event::ElementState::Pressed)
                {
                    if let (Some(menu), Some(window)) = (self.menu.as_ref(), self.window.as_ref()) {
                        if let Err(e) = showContextMenu(menu, window) {
                            log::warn!("show menu failed: {e:?}");
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((dx, dy)) = self.input.onCursorMove(position.x as f32, position.y as f32) {
                    self.physics.x += dx;
                    self.physics.y += dy;
                    self.physics.vx = 0.0;
                    self.physics.vy = 0.0;
                    if let Some(w) = self.window.as_ref() {
                        w.set_outer_position(winit::dpi::LogicalPosition::new(
                            self.physics.x,
                            self.physics.y,
                        ));
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.ensureNeedsAssets();
                if let (Some(r), Some(scene)) = (self.renderer.as_mut(), self.scene.as_mut()) {
                    let w = r.config.width as f32;
                    let h = r.config.height as f32;
                    let now = std::time::Instant::now();
                    let dt = (now - scene.lastTickAt).as_secs_f32().min(0.05);
                    scene.lastTickAt = now;
                    // 心情低于 40 时禁止随机奔跑与爬墙（走动/拖动/下落/休息链不受影响）。
                    let allowRunClimb = self.settings.needs.mood >= 40.0;
                    let newBehavior = behaviorTick(
                        &mut self.behavior,
                        &mut self.physics,
                        &self.physicsCfg,
                        self.input.dragging,
                        self.settings.actionDurations.walkSec,
                        self.settings.actionDurations.runSec,
                        allowRunClimb,
                    );
                    let _ = newBehavior;
                    let climbActive = tickClimb(
                        &mut self.physics,
                        &self.physicsCfg,
                        &self.bounds,
                        &self.behavior,
                        &mut self.climb,
                        &mut self.lastClimbEndAt,
                        self.input.dragging,
                        dt,
                        allowRunClimb,
                    );
                    let isRight = self.physics.facing > 0;
                    let facingNum = if isRight { 1.0 } else { -1.0 };
                    let vxUnit = self.physics.vx / 8.0 * facingNum;
                    let vyUnit = -self.physics.vy / 8.0;

                    let isStandingStill = self.physics.vx.abs() < 1.0
                        && self.physics.grounded
                        && !self.input.dragging
                        && self.pickedMotion.is_none();
                    if isStandingStill {
                        self.idleTimeSec += dt;
                    } else {
                        self.idleTimeSec = 0.0;
                    }
                    let falling = !self.physics.grounded && self.physics.vy.abs() > 30.0;

                    // 三系统推进：饥渴衰减 + 心情驱动。互动 = 拖拽 / paw。
                    let interaction = self.input.dragging
                        || matches!(self.pickedMotion.as_ref().map(|(s, _)| s.as_str()), Some("paw"));
                    let decay = self.settings.needsConfig.decayPreset.perSec();
                    self.settings.needs.tick(dt, decay, interaction, self.musicPlaying);
                    let feedingHappy = self
                        .feedingStartedAt
                        .map(|t| std::time::Instant::now().duration_since(t).as_secs_f32() < 1.0)
                        .unwrap_or(false);

                    self.mood = updateMood(MoodInputs {
                        dragging: self.input.dragging,
                        grounded: self.physics.grounded,
                        idleTimeSec: self.idleTimeSec,
                        pickedMotion: self.pickedMotion.as_ref().map(|(s, _)| s.as_str()).map(|s| match s {
                            "paw" => "paw",
                            "happy" => "happy",
                            _ => "",
                        }).filter(|s| !s.is_empty()),
                        vyDownPositive: self.physics.vy,
                        falling,
                        moodValue: self.settings.needs.mood,
                        feedingHappy,
                    });
                    // needs 节流写盘标记（此处仅算标记，不调方法以免与 renderer 借用冲突）。
                    self.needsSaveTimer += dt;
                    let doNeedsSave = if self.needsSaveTimer >= 10.0 {
                        self.needsSaveTimer = 0.0;
                        true
                    } else {
                        false
                    };

                    let canRest = isStandingStill;
                    let dur = self.settings.actionDurations.clone();
                    if !canRest && self.actionStage != ActionStage::None {
                        self.actionStage = ActionStage::None;
                        self.actionStageTimer = 0.0;
                    } else if canRest {
                        self.actionStageTimer += dt;
                        // 歇息超时链：待机 → 坐 → 躺 → 趴 逐级推进。
                        let next = match self.actionStage {
                            ActionStage::None if self.idleTimeSec > dur.idleToSitSec => Some(ActionStage::Sit),
                            ActionStage::Sit if self.actionStageTimer > dur.sitToLaySec => Some(ActionStage::Lay),
                            ActionStage::Lay if self.actionStageTimer > dur.layToPlankSec => Some(ActionStage::Plank),
                            _ => None,
                        };
                        if let Some(s) = next {
                            self.actionStage = s;
                            self.actionStageTimer = 0.0;
                        }
                    }
                    let stage = self.actionStage;
                    // 仅在休息姿势「变化沿」触发一次 playState：避免每帧把已自动 transition
                    // 走的 state（如 Lay 播完返回站立）反复拉回重播 → 鬼畜。
                    let stageChanged = canRest && self.lastPlayedStage != Some(stage);
                    if !canRest {
                        self.lastPlayedStage = None;
                    } else if stageChanged {
                        self.lastPlayedStage = Some(stage);
                    }

                    let exerciseMotion = matches!(
                        self.pickedMotion.as_ref().map(|(s, _)| s.as_str()),
                        Some("pushup" | "squat" | "plank")
                    );
                    let exercising = stage == ActionStage::Plank || exerciseMotion;

                    if let Some(player) = scene.bodyPlayer.as_mut() {
                        feedBodyParams(player, vxUnit, vyUnit, self.physics.grounded, exercising, climbActive);
                        if climbActive {
                            if player.currentStateName(0) != Some("Climb") {
                                player.playState(0, "Climb");
                            }
                        } else {
                            applyPickedMotion(player, &self.pickedMotion, false);
                            let curName = player.currentStateName(0).map(|s| s.to_string());
                            let want = stage.bodyState();
                            if !canRest && curName.as_deref() != Some("Grounded") && curName.as_deref() != Some("Air") {
                                player.playState(0, "Grounded");
                            } else if let (true, Some(target)) = (stageChanged, want) {
                                // 仅姿势变化沿播放一次；之后让动画自然推进 / 过渡，不每帧拉回。
                                player.playState(0, target);
                            }
                        }
                        player.update(dt);
                    }
                    if let Some(player) = scene.armsPlayer.as_mut() {
                        feedArmsParams(player, vxUnit, vyUnit, self.physics.grounded, exercising, climbActive);
                        if climbActive {
                            if player.currentStateName(0) != Some("Climb") {
                                player.playState(0, "Climb");
                            }
                        } else {
                            applyPickedMotion(player, &self.pickedMotion, true);
                            let curName = player.currentStateName(0).map(|s| s.to_string());
                            let want = stage.armsState();
                            if !canRest && curName.as_deref() != Some("Grounded") && curName.as_deref() != Some("Air") {
                                player.playState(0, "Grounded");
                            } else if let (true, Some(target)) = (stageChanged, want) {
                                player.playState(0, target);
                            }
                        }
                        player.update(dt);
                    }
                    let now = std::time::Instant::now();
                    if let Some((_, until)) = self.pickedMotion.as_ref() {
                        if now >= *until {
                            self.pickedMotion = None;
                            // 点击动作结束：清除已播姿势标记，使休息姿势在下一帧变化沿重新触发，
                            // 从临时动作末态恢复到当前 actionStage。
                            self.lastPlayedStage = None;
                        }
                    }

                    let bodySamples = scene
                        .bodyPlayer
                        .as_ref()
                        .map(|p| p.evaluate())
                        .unwrap_or_default();
                    let armsSamples = scene
                        .armsPlayer
                        .as_ref()
                        .map(|p| p.evaluate())
                        .unwrap_or_default();
                    let mut limbs: Vec<LimbPose> =
                        crate::bodyAssembly::assembleLimbs(&scene.pose.limbs, &bodySamples, &armsSamples);
                    let dx = (self.physics.x - self.lastWindowPos.0).abs();
                    let dy = (self.physics.y - self.lastWindowPos.1).abs();
                    if dx > 0.5 || dy > 0.5 {
                        if let Some(win) = self.window.as_ref() {
                            win.set_outer_position(winit::dpi::LogicalPosition::new(
                                self.physics.x,
                                self.physics.y,
                            ));
                        }
                        self.lastWindowPos = (self.physics.x, self.physics.y);
                    }
                    let flap = wings::step(
                        &mut scene.wings,
                        WingsConfig::default(),
                        self.physics.grounded,
                        self.physics.vy,
                        false,
                        isRight,
                        dt,
                    );
                    for limb in limbs.iter_mut() {
                        if limb.name.starts_with("wing") {
                            limb.visible = false;
                        }
                    }
                    let tailLocalRot = tickTail(
                        &mut scene.tail,
                        self.physics.vx,
                        self.physics.vy,
                        isRight,
                        dt,
                    );
                    if let Some((dpx, dpy, drz)) = limbs
                        .iter()
                        .find(|l| l.name == "DownTorso")
                        .map(|l| (l.px, l.py, l.rotZ))
                    {
                        let r = drz.to_radians();
                        let (c, s) = (r.cos(), r.sin());
                        if let Some(tail) = limbs.iter_mut().find(|l| l.name == "Tail") {
                            tail.px = dpx + c * TAIL_OFFSET.0 - s * TAIL_OFFSET.1;
                            tail.py = dpy + s * TAIL_OFFSET.0 + c * TAIL_OFFSET.1;
                            tail.rotZ = drz + tailLocalRot;
                        }
                    }
                    let unitToPx = PIXELS_PER_UNIT * self.stageScale * self.dpiScale;
                    let facingSign = if isRight { 1.0 } else { -1.0 };
                    let headPx = limbs.iter().find(|l| l.name == "Head").map(|l| l.px).unwrap_or(0.0);
                    let headX = w * 0.5 + headPx * unitToPx * facingSign;
                    let eyeSprite = match getCursorScreen() {
                        Some((sx, _)) => pickEyeSprite(self.mood, sx - self.physics.x * self.dpiScale, headX, isRight),
                        None => moodToEyeSprite(self.mood),
                    };
                    // 喂食 1.0s 内张嘴动画：0~0.6s HeadBackMouth，0.6~1.0s HeadBackMouthMini
                    let feedingMouthHead = self.feedingStartedAt.and_then(|start| {
                        let elapsed = std::time::Instant::now().duration_since(start).as_secs_f32();
                        if elapsed < 1.0 {
                            Some(if elapsed < 0.6 { "experimentHeadBackMouth" } else { "experimentHeadBackMouthMini" })
                        } else {
                            None
                        }
                    });
                    let mut draws = Self::buildDrawsFrom(
                        &scene.sprites,
                        &limbs,
                        isRight,
                        w,
                        h,
                        unitToPx,
                        eyeSprite,
                        feedingMouthHead,
                    );
                    Self::appendWingDraws(
                        &scene.sprites,
                        &limbs,
                        &flap,
                        WingsLayout::default(),
                        isRight,
                        w,
                        h,
                        unitToPx,
                        &mut draws,
                    );
                    // 已装备背包/配饰叠加到桌宠身上。
                    Self::appendAccessoryDraws(
                        &scene.sprites,
                        &limbs,
                        &scene.accessoryDefs,
                        &self.settings.inventory,
                        self.headsetEquipped,
                        isRight,
                        w,
                        h,
                        unitToPx,
                        &mut draws,
                    );
                    // 气泡贴近头顶上方：底边落在头顶稍上，越界则下限保护到窗口顶。
                    let headTopY = h * CENTER_Y_RATIO - HEAD_TOP_UNITS * unitToPx;
                    let bubbleActive = self
                        .bubble
                        .as_ref()
                        .map_or(false, |b| std::time::Instant::now() < b.expireAt);
                    if let (Some(b), Some(bg)) = (self.bubble.as_ref(), self.bubbleBg.as_ref()) {
                        if std::time::Instant::now() < b.expireAt {
                            let anchorTopY = (headTopY - 6.0 - b.height).max(2.0);
                            let bgAlpha = self.settings.bubbleStyle.bgAlpha;
                            let textColor = self.settings.bubbleStyle.textColor;
                            let bgColor = self.settings.bubbleStyle.bgColor;
                            appendBubbleDraws(b, bg, w * 0.5, anchorTopY, bgAlpha, textColor, bgColor, w, h, &mut draws);
                        }
                    }

                    // 表情包：绘制在宠物身体左侧或右侧，避开头顶气泡区。
                    let petCenterX = w * 0.5;
                    let petCenterY = h * CENTER_Y_RATIO;
                    let stickerDraws = self.stickerManager.buildDraws(
                        w, h, petCenterX, petCenterY,
                        self.settings.stickerConfig.stickerWidth,
                        self.settings.stickerConfig.stickerHeight,
                    );
                    draws.extend(stickerDraws);

                    // 脚底三条状态条：悬停浮现 + 极低值强制短暂浮现。
                    if self.settings.needsConfig.barEnabled {
                        // 内联悬停判定（避免借用方法与 renderer 冲突）。
                        let (cxh, cyh) = self.input.lastCursorClient;
                        let hovering = cxh >= 0.0
                            && cyh >= 0.0
                            && cxh < w
                            && cyh < h
                            && !self.lastRenderedLimbs.is_empty()
                            && anyLimbBboxContains(
                                &self.lastRenderedLimbs,
                                cxh,
                                cyh,
                                w * 0.5,
                                h * CENTER_Y_RATIO,
                                facingSign,
                                unitToPx,
                                unitToPx / PIXELS_PER_UNIT,
                            );
                        let n = &self.settings.needs;
                        let critical = n.mood <= crate::needs::CRITICAL
                            || n.hunger <= crate::needs::CRITICAL
                            || n.thirst <= crate::needs::CRITICAL;
                        if critical {
                            self.needsForceShowSec = 2.5;
                        } else if self.needsForceShowSec > 0.0 {
                            self.needsForceShowSec = (self.needsForceShowSec - dt).max(0.0);
                        }
                        // 桌宠说话时状态条让位（同处脚底区，避免重叠）。
                        let target = if (hovering || self.needsForceShowSec > 0.0) && !bubbleActive {
                            1.0
                        } else {
                            0.0
                        };
                        // 指数趋近实现淡入淡出。
                        self.needsBarAlpha += (target - self.needsBarAlpha) * (1.0 - (-dt * 8.0).exp());
                        if let Some(solid) = self.solidWhite.as_ref() {
                            // 面板顶边定在脚底下方约 2px；面板自 groupBottomY 向上堆叠。
                            // 面板总高度 ≈ groupH(86.5) + padding(8) ≈ 95px。
                            let feetBottomY = h * CENTER_Y_RATIO + FEET_BOTTOM_UNITS * unitToPx;
                            let groupBottomY = feetBottomY + 93.0;
                            let icons = [
                                self.needsIcons[0].as_ref(),
                                self.needsIcons[1].as_ref(),
                                self.needsIcons[2].as_ref(),
                            ];
                            let titles = [
                                self.titleSprites[0].as_ref(),
                                self.titleSprites[1].as_ref(),
                                self.titleSprites[2].as_ref(),
                            ];
                            crate::needsBar::appendNeedsBarDraws(
                                &self.settings.needs,
                                solid,
                                icons,
                                titles,
                                self.percentSprite.as_ref(),
                                self.heartSprite.as_ref(),
                                &self.digitSprites,
                                self.needsBarAlpha,
                                self.settings.barStyle.bgAlpha,
                                headX,
                                groupBottomY,
                                w,
                                h,
                                &mut draws,
                            );
                        }
                    }
                    if let Err(e) = r.renderFrame(&draws) {
                        log::warn!("render error: {e:?}");
                    }
                    self.lastRenderedLimbs = limbs;
                    // renderer 借用此处已结束：消费本帧 needs 副作用。
                    // 自动气泡（需求/闲聊）受静默期约束：气泡显示 3s + 间隔 3s 内不出新的（也不推进计时器，
                    // 避免吞掉机会）。pick* 内部还各自检查「当前有气泡不抢」。交互气泡可随时抢占。
                    if std::time::Instant::now() >= self.bubbleBlockedUntil {
                        if let Some(text) = self.pickNeedsBubble(dt) {
                            self.showBubble(&text);
                        } else if let Some(text) = self.pickChatterBubble(dt) {
                            self.showBubble(&text);
                        }
                    }
                    if doNeedsSave {
                        // 每 10s 顺带检查背包 24h 时限：过期的背包（连同内容）移除。
                        let expired = self.settings.inventory.expireBackpacks(nowUnixMs());
                        if !expired.is_empty() {
                            log::info!("背包过期移除: {expired:?}");
                        }
                        self.persistSettings();
                    }
                } else if let Some(r) = self.renderer.as_mut() {
                    if let Err(e) = r.render() {
                        log::warn!("render error: {e:?}");
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // Poll 模式：事件循环不等待 WM_PAINT，保证穿透（WS_EX_TRANSPARENT）状态下动画也能持续运行。
        el.set_control_flow(winit::event_loop::ControlFlow::Poll);
        if let Ok(evt) = muda::MenuEvent::receiver().try_recv() {
            let id = evt.id().0.as_str().to_string();
            match id.as_str() {
                crate::trayIcon::ID_TRAY_QUIT => self.shouldExit = true,
                crate::trayIcon::ID_TRAY_SETTINGS => self.openSettingsPanel(),
                crate::trayIcon::ID_TRAY_TOGGLE => self.toggleWindowVisible(),
                _ => {
                    if let Some(action) = parseMenuId(&id) {
                        self.handleMenuAction(action);
                    }
                }
            }
        }
        self.tickTrayTooltip();
        self.tickSettingsWindow(el);
        self.tickInventoryWindow(el);
        self.tickRpsWindow(el);
        self.tickWheelWindow(el);
        self.tickMusicPlayerWindow(el);
        self.tickMusicPlayerCfgWindow(el);
        self.tickWheelCfgWindow(el);
        self.tickFeedDrag(el);
        self.tickDroppedItems(el);
        self.tickBeacon();
        self.tickPlugins();
        self.tickSmartPenetration();
        self.tickFullscreenDetection();
        self.tickStickers();
        if self.shouldExit {
            self.cancelFeedDrag(); // 归还在途拖拽物品，避免退出时物品丢失。
            self.persistSettings();
            let root = appRoot(self.opts.configRoot.as_deref());
            let petRoot = root.join("desktopPet");
            removeOwnState(&petRoot, &self.opts.petId);
            el.exit();
            return;
        }
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

impl PetApp {
    /// 表情包计时推进：~60fps 节流，idle 随机弹出 + 活跃表情到期清除。
    fn tickStickers(&mut self) {
        let now = std::time::Instant::now();
        let dt = (now - self.lastStickerTickAt).as_secs_f32();
        if dt < 0.016 {
            return; // ~60fps 节流
        }
        self.lastStickerTickAt = now;
        let dt = dt.min(0.05); // 防大跳
        // 需要 clone settings 中的两个字段以避免借用冲突。
        let needs = self.settings.needs.clone();
        let stickerCfg = self.settings.stickerConfig.clone();
        self.stickerManager.tick(dt, &needs, &stickerCfg);
    }

    fn tickPlugins(&mut self) {
        let host = match self.pluginHost.as_mut() {
            Some(h) => h,
            None => return,
        };
        let root = appRoot(self.opts.configRoot.as_deref());
        let pluginsDir = root.join("desktopPet").join("plugins");
        host.tick(&pluginsDir);
        let cmds = host.drainCommands();
        for cmd in cmds {
            self.applyPluginCmd(cmd);
        }
    }

    fn applyPluginCmd(&mut self, cmd: PluginCmd) {
        match cmd {
            PluginCmd::PlayMotion { name, durationMs } => {
                self.pickedMotion = Some((
                    name,
                    std::time::Instant::now() + std::time::Duration::from_millis(durationMs),
                ));
            }
            PluginCmd::Say { text, durationMs } => {
                self.sayText(&text, durationMs);
            }
        }
    }

    fn hitTestCursor(&self) -> bool {
        let r = match self.renderer.as_ref() {
            Some(r) => r,
            None => return false,
        };
        let scrW = r.config.width as f32;
        let scrH = r.config.height as f32;
        let (cx, cy) = self.input.lastCursorClient;
        if cx < 0.0 || cy < 0.0 || cx >= scrW || cy >= scrH {
            return false;
        }
        let isRight = self.physics.facing > 0;
        let facingSign = if isRight { 1.0 } else { -1.0 };
        let unitToPx = PIXELS_PER_UNIT * self.stageScale * self.dpiScale;
        let pxRatio = unitToPx / PIXELS_PER_UNIT;
        anyLimbBboxContains(
            &self.lastRenderedLimbs,
            cx,
            cy,
            scrW * 0.5,
            scrH * CENTER_Y_RATIO,
            facingSign,
            unitToPx,
            pxRatio,
        )
    }

    fn tickSmartPenetration(&mut self) {
        if !matches!(self.penetration, PenetrationMode::Smart) {
            return;
        }
        let now = std::time::Instant::now();
        if let Some(prev) = self.lastSmartCheckAt {
            if (now - prev).as_millis() < 30 {
                return;
            }
        }
        self.lastSmartCheckAt = Some(now);
        let cursor = match getCursorScreen() {
            Some(c) => c,
            None => return,
        };
        let renderer = match self.renderer.as_ref() {
            Some(r) => r,
            None => return,
        };
        let dpi = self.dpiScale.max(0.001);
        let winPhysX = self.physics.x * dpi;
        let winPhysY = self.physics.y * dpi;
        let scrW = renderer.config.width as f32;
        let scrH = renderer.config.height as f32;
        let cxp = cursor.0 - winPhysX;
        let cyp = cursor.1 - winPhysY;
        let mut hit = false;
        if cxp >= 0.0 && cyp >= 0.0 && cxp < scrW && cyp < scrH && !self.lastRenderedLimbs.is_empty() {
            let isRight = self.physics.facing > 0;
            let facingSign = if isRight { 1.0 } else { -1.0 };
            let unitToPx = PIXELS_PER_UNIT * self.stageScale * dpi;
            let pxRatio = unitToPx / PIXELS_PER_UNIT;
            let centerX = scrW * 0.5;
            let centerY = scrH * CENTER_Y_RATIO;
            hit = anyLimbBboxContains(
                &self.lastRenderedLimbs,
                cxp,
                cyp,
                centerX,
                centerY,
                facingSign,
                unitToPx,
                pxRatio,
            );
        }
        if hit != self.cursorHittest {
            if let Some(w) = self.window.as_ref() {
                let _ = w.set_cursor_hittest(hit);
            }
            self.cursorHittest = hit;
        }
    }

    /// 全屏检测：当有应用全屏（游戏 / 办公软件）时自动隐藏桌宠，退出全屏后恢复。
    fn tickFullscreenDetection(&mut self) {
        if !self.settings.fullscreenHideEnabled {
            return;
        }
        let now = std::time::Instant::now();
        if let Some(prev) = self.lastFullscreenCheckAt {
            if (now - prev).as_millis() < 500 {
                return;
            }
        }
        self.lastFullscreenCheckAt = Some(now);

        let fullscreen = isForegroundFullscreen();
        if fullscreen && !self.fullscreenHidden {
            if let Some(w) = self.window.as_ref() {
                if w.is_visible().unwrap_or(true) {
                    let _ = w.set_visible(false);
                    self.fullscreenHidden = true;
                    log::info!("fullscreen detected → hiding pet");
                }
            }
        } else if !fullscreen && self.fullscreenHidden {
            if let Some(w) = self.window.as_ref() {
                let _ = w.set_visible(true);
                self.fullscreenHidden = false;
                log::info!("fullscreen ended → showing pet");
            }
        }
    }

    fn tickBeacon(&mut self) {
        let now = std::time::Instant::now();
        if let Some(prev) = self.lastBeaconAt {
            if (now - prev).as_millis() < 800 {
                return;
            }
        }
        self.lastBeaconAt = Some(now);
        let root = appRoot(self.opts.configRoot.as_deref());
        let petRoot = root.join("desktopPet");
        let state = BusState {
            petId: self.opts.petId.clone(),
            pid: std::process::id(),
            screenX: self.physics.x,
            screenY: self.physics.y,
            facing: self.physics.facing,
            behavior: self.behavior.current.asStr().to_string(),
            timestampMs: crate::bus::nowMillis(),
        };
        if let Err(e) = writeOwnState(&petRoot, &state) {
            log::warn!("write own state failed: {e:?}");
        }
        self.neighbors = readNeighbors(&petRoot, &self.opts.petId);
        if self.interaction.is_none() {
            let cfg = loadInteractions(&petRoot, "default");
            self.interaction = Some(InteractionState::new(cfg));
        }
        if let Some(intr) = self.interaction.as_mut() {
            let triggered = interactTick(intr, &state, &self.neighbors);
            for a in triggered {
                self.applyTriggeredAction(a);
            }
        }
    }

    fn applyTriggeredAction(&mut self, a: TriggeredAction) {
        match a.kind.as_str() {
            "playMotion" => {
                if let Some(name) = a.motion {
                    self.pickedMotion = Some((
                        name,
                        std::time::Instant::now() + std::time::Duration::from_millis(a.durationMs),
                    ));
                }
            }
            "say" => {
                if let Some(text) = a.text {
                    self.sayText(&text, a.durationMs);
                }
            }
            "moveToward" => {
                let dir = if a.targetX > self.physics.x { 1 } else { -1 };
                self.physics.facing = dir;
                self.physics.vx = dir as f32 * a.speed;
                self.pickedMotion = Some((
                    "walk".into(),
                    std::time::Instant::now() + std::time::Duration::from_millis(a.durationMs),
                ));
            }
            _ => {}
        }
    }
    fn tickTrayTooltip(&mut self) {
        let now = std::time::Instant::now();
        if let Some(prev) = self.lastTooltipAt {
            if (now - prev).as_millis() < 500 {
                return;
            }
        }
        let status = if self.input.dragging {
            "拖拽中"
        } else if self.climb != ClimbState::None {
            "爬墙中"
        } else if !self.physics.grounded {
            "落下中"
        } else if self.pickedMotion.is_some() {
            "动作播放"
        } else if self.physics.vx.abs() > 1.0 {
            match self.behavior.current {
                crate::behavior::BehaviorName::Run => "奔跑",
                _ => "走动",
            }
        } else if self.actionStage == ActionStage::Plank {
            "趴着"
        } else if self.actionStage == ActionStage::Lay {
            "躺着"
        } else if self.actionStage == ActionStage::Sit {
            "坐着"
        } else {
            "站立"
        };
        if status != self.lastTooltipStatus {
            if let Some(t) = self.tray.as_ref() {
                crate::trayIcon::updateTooltip(t, &self.opts.petId, status);
            }
            self.lastTooltipStatus = status.to_string();
        }
        self.lastTooltipAt = Some(now);
    }

    fn handleMenuAction(&mut self, action: MenuAction) {
        match action {
            MenuAction::PickMotion(name) => {
                self.pickedMotion = Some((
                    name,
                    std::time::Instant::now() + std::time::Duration::from_secs(2),
                ));
            }
            MenuAction::SetPenetration(m, key) => {
                self.penetration = m;
                self.settings.penetration = key;
                if let Some(w) = self.window.as_ref() {
                    applyPenetration(w, m);
                }
                self.persistSettings();
            }
            MenuAction::SetScale(v) => {
                self.stageScale = v;
                self.settings.stageScale = v;
                self.persistSettings();
            }
            MenuAction::SetDpi(mode) => {
                self.settings.dpiMode = mode;
                self.applyDpiMode();
                self.persistSettings();
            }
            MenuAction::SetFont(key) => {
                self.settings.fontMode = key;
                self.initTextSystem();
                self.persistSettings();
            }
            MenuAction::Close => self.shouldExit = true,
            MenuAction::OpenSettings => self.openSettingsPanel(),
            MenuAction::OpenInventory => self.openInventoryPanel(),
            MenuAction::OpenGame => self.pendingOpenGame = true,
            MenuAction::OpenWheel => self.pendingOpenWheel = true,
            MenuAction::OpenMusicPlayer => self.openMusicPlayerPanel(),
            MenuAction::ToggleMusic => {
                self.musicPlaying = !self.musicPlaying;
                self.headsetEquipped = self.musicPlaying;
                log::info!("music: {}", if self.musicPlaying { "on" } else { "off" });
                // 音乐时间切换 -> 播放器窗口歌单即时刷新
                if let Some(ref mw) = self.musicPlayerWindow {
                    mw.requestRedraw();
                }
                let text = if self.musicPlaying {
                    if let Some(ref mut p) = self.musicPlayer {
                        p.start(&self.musicFiles);
                    }
                    self.chatter
                        .pickMusicOn(crate::behavior::rand01())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "开始听音乐~ 🎵".to_string())
                } else {
                    if let Some(ref mut p) = self.musicPlayer {
                        p.stop();
                    }
                    self.chatter
                        .pickMusicOff(crate::behavior::rand01())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "音乐关掉啦".to_string())
                };
                self.sayText(&text, 2000);
            }
        }
    }

    fn toggleWindowVisible(&mut self) {
        if let Some(w) = self.window.as_ref() {
            w.set_visible(!w.is_visible().unwrap_or(true));
            // 用户手动操作 → 清除全屏自动隐藏标记
            self.fullscreenHidden = false;
        }
    }

    fn tickSettingsWindow(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        if self.pendingOpenSettings && self.settingsWindow.is_none() {
            self.pendingOpenSettings = false;
            let root = appRoot(self.opts.configRoot.as_deref());
            let fontDir = root.join("desktopPet").join("fonts");
            let availableFonts = crate::text::scanDirFamilies(&fontDir);
            let availableSkins = crate::paths::listSkins(&root);
            match crate::settingsWindow::SettingsWindow::create(el, &self.settings, availableFonts, availableSkins, &self.settings.uiFontFamily) {
                Ok(w) => {
                    w.requestRedraw();
                    self.settingsWindow = Some(w);
                }
                Err(e) => log::warn!("settings window create failed: {e:?}"),
            }
        }
        let (close, change) = match self.settingsWindow.as_mut() {
            // 仅在需要重绘时 present，空闲时跳过 → 不拖慢主循环 / 桌宠动画。
            Some(sw) if sw.wantsRedraw() => {
                let change = sw.frame(&mut self.settings);
                // 首帧渲染成功后显示窗口，消除加载白块。
                if !sw.window.is_visible().unwrap_or(false) {
                    sw.window.set_visible(true);
                }
                (sw.pendingClose(), change)
            }
            Some(sw) => (sw.pendingClose(), crate::settingsWindow::SettingsChange::default()),
            None => return,
        };
        if change.stageScale.is_some() {
            self.stageScale = self.settings.stageScale;
            log::info!("settings change: stageScale -> {}", self.stageScale);
        }
        if let Some(s) = change.inventoryScale {
            self.settings.inventoryScale = s;
            if let Some(iw) = self.inventoryWindow.as_mut() {
                iw.setScale(s);
            }
            self.persistSettings();
            log::info!("settings change: inventoryScale -> {}", s);
        }
        if change.dpiMode.is_some() {
            self.applyDpiMode();
            log::info!("settings change: dpiMode -> {:?}", self.settings.dpiMode);
        }
        if change.fontMode.is_some() {
            self.initTextSystem();
            // 字体来源变了，可用字体集合变化，状态条字形缓存一并重建。
            self.needsAssetsLoaded = false;
            log::info!("settings change: fontMode -> {:?}", self.settings.fontMode);
        }
        if let Some((m, _)) = change.penetration {
            self.penetration = m;
            if let Some(w) = self.window.as_ref() {
                applyPenetration(w, m);
            }
            log::info!("settings change: penetration -> {:?}", m);
        }
        if change.barStyle {
            // 状态条字体/字号变了，缓存的标题/数字字形作废，下帧重建。
            self.needsAssetsLoaded = false;
            log::info!("settings change: barStyle -> rebuild needs glyph cache");
        }
        if change.stickerConfig {
            log::info!("settings change: stickerConfig updated");
        }
        if change.bubbleStyle {
            log::info!("settings change: bubbleStyle updated");
        }
        if change.autoStart {
            applyAutoStart(self.settings.autoStart);
            log::info!("settings change: autoStart -> {}", self.settings.autoStart);
        }
        if change.fullscreenHide {
            log::info!("settings change: fullscreenHideEnabled -> {}", self.settings.fullscreenHideEnabled);
        }
        if change.skinChanged {
            self.loadScene();
            log::info!("settings change: skin -> {}", self.settings.skin);
        }
        if change.stageScale.is_some()
            || change.dpiMode.is_some()
            || change.fontMode.is_some()
            || change.penetration.is_some()
            || change.actionDurations
            || change.needsConfig
            || change.needsReset
            || change.bubbleStyle
            || change.barStyle
            || change.stickerConfig
            || change.autoStart
            || change.fullscreenHide
            || change.skinChanged
        {
            self.persistSettings();
        }
        if close {
            self.settingsWindow = None;
        }
    }

    /// 创建 / 驱动 RPS 游戏窗口；胜利时把奖励推入 pendingDrops 由 tickDroppedItems 生成掉落物窗。
    fn tickRpsWindow(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        if self.pendingOpenGame && self.rpsWindow.is_none() {
            self.pendingOpenGame = false;
            let root = appRoot(self.opts.configRoot.as_deref());
            let rpsDir = crate::paths::rpsDir(&root);
            match crate::rpsGame::RpsWindow::create(el, &rpsDir) {
                Ok(w) => {
                    w.requestRedraw();
                    self.rpsWindow = Some(w);
                    // 开场气泡：桌宠喊一句（无对应台词则不弹气泡）。
                    if let Some(line) = self.chatter.pickGameStart(crate::behavior::rand01()) {
                        let line = line.to_string();
                        self.sayText(&line, 2200);
                    }
                }
                Err(e) => log::warn!("rps window create failed: {e:?}"),
            }
        }
        // 把按钮窗贴到桌宠右侧——仅当桌宠位置变化时才 set_outer_position。
        // 主窗 Mailbox 高帧率空转，若每帧都调 SetWindowPos 会打爆 DWM 合成器 → 桌宠动画卡死。
        if let Some(rw) = self.rpsWindow.as_ref() {
            let pos = (self.physics.x, self.physics.y);
            if self.rpsLastPos != Some(pos) {
                let cfg = &self.settings.rps;
                rw.setPos(pos.0 + PET_W as f32 + cfg.offsetX, pos.1 + cfg.offsetY);
                // 定位完成后显示，避免创建时 (0,0) 闪现白窗。
                rw.window.set_visible(true);
                self.rpsLastPos = Some(pos);
            }
        }
        let (mut close, change) = match self.rpsWindow.as_mut() {
            // 仅在需要重绘时 present，空闲时跳过 → 不拖慢主循环 / 桌宠动画。
            Some(rw) if rw.wantsRedraw() => {
                let change = rw.frame(&self.settings.rps);
                (rw.pendingClose() || change.closed, Some(change))
            }
            Some(rw) => (rw.pendingClose(), None),
            None => return,
        };
        if let Some(change) = change {
            // 一次只玩一局：出拳后自动关窗。
            if change.play.is_some() {
                close = true;
            }
            // 出招：判胜负 → 结果气泡 → 胜利掉落奖励。
            if let Some(hand) = change.play {
                let cpu = crate::rpsGame::computerHand(crate::behavior::rand01());
                let out = crate::rpsGame::judge(hand, cpu);
                // 结果台词从 chatter.json 对应池随机抽取；池空则使用简短硬回退。
                let resultStr = match out {
                    crate::rpsGame::Outcome::Win =>
                        self.chatter.pickGameWin(crate::behavior::rand01()).unwrap_or("游戏结果：Aibo，你是这场的赢家！"),
                    crate::rpsGame::Outcome::Draw =>
                        self.chatter.pickGameDraw(crate::behavior::rand01()).unwrap_or("游戏结果：Aibo与exp皆获得胜利！"),
                    crate::rpsGame::Outcome::Lose =>
                        self.chatter.pickGameLose(crate::behavior::rand01()).unwrap_or("游戏结果：Aibo，这场的胜利就由我拿下了！"),
                };
                let msg = format!(
                    "exp出{}，你出{}\n{}",
                    crate::rpsGame::handLabel(cpu),
                    crate::rpsGame::handLabel(hand),
                    resultStr,
                );
                self.sayText(&msg, 1800);
                // 触发对应分类的表情包。
                let stickerCat = match out {
                    crate::rpsGame::Outcome::Win => crate::sticker::StickerCategory::Win,
                    crate::rpsGame::Outcome::Draw => crate::sticker::StickerCategory::Draw,
                    crate::rpsGame::Outcome::Lose => crate::sticker::StickerCategory::Lose,
                };
                self.stickerManager.trigger(
                    stickerCat,
                    crate::behavior::rand01(),
                    crate::behavior::rand01(),
                );
                if out == crate::rpsGame::Outcome::Win {
                    self.settings.rpsCoins += 1;
                }
            }
        }
        if close {
            self.rpsWindow = None;
            self.rpsLastPos = None;
        }
    }

    /// 创建 / 驱动抽奖转盘窗口；投币消耗代币，停止后产出奖励掉落。
    fn tickWheelWindow(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        // 如果轮盘已存在（可能被拖到屏幕外），重新定位并显示
        if self.pendingOpenWheel && self.wheelWindow.is_some() {
            self.pendingOpenWheel = false;
            if let Some(ww) = self.wheelWindow.as_mut() {
                ww.setPos(self.settings.wheel.windowPosX, self.settings.wheel.windowPosY);
                ww.window.set_visible(true);
                ww.window.focus_window();
                ww.requestRedraw();
            }
        }
        if self.pendingOpenWheel && self.wheelWindow.is_none() {
            self.pendingOpenWheel = false;
            let root = crate::paths::appRoot(self.opts.configRoot.as_deref());
            let wheelDir = crate::paths::wheelDir(&root);
            let cfg = &self.settings.wheel;
            match crate::rewardWheel::RewardWheelWindow::create(el, &wheelDir, cfg) {
                Ok(w) => {
                    w.requestRedraw();
                    w.setPos(cfg.windowPosX, cfg.windowPosY);
                    w.window.set_visible(true);
                    self.wheelWindow = Some(w);
                    if let Some(line) = self.chatter.pickWheelStart(crate::behavior::rand01()) {
                        let line = line.to_string();
                        self.sayText(&line, 2200);
                    }
                }
                Err(e) => log::warn!("wheel window create failed: {e:?}"),
            }
        }
        // 打开配置窗口
        let mut openCfg = false;
        if let Some(ww) = self.wheelWindow.as_mut() {
            if ww.wantsOpenCfg() {
                ww.takeOpenCfg();
                openCfg = true;
            }
        }
        if openCfg {
            self.persistSettings();
            if self.wheelCfgWindow.is_none() {
                self.pendingOpenWheelCfg = true;
            }
        }
        let (close, change) = match self.wheelWindow.as_mut() {
            Some(ww) if ww.wantsRedraw() => {
                let change = ww.frame(self.settings.rpsCoins);
                (ww.pendingClose() || change.closed, Some(change))
            }
            Some(ww) => (ww.pendingClose(), None),
            None => return,
        };
        if let Some(change) = change {
            if change.coinSpent {
                self.settings.rpsCoins = self.settings.rpsCoins.saturating_sub(1);
                self.persistSettings();
            }
            if let Some(item) = change.reward {
                self.pendingDrops.push(item);
                // 结果气泡
                let msg = "转盘奖励! 看看掉了什么~".to_string();
                self.sayText(&msg, 2000);
                self.stickerManager.trigger(
                    crate::sticker::StickerCategory::Win,
                    crate::behavior::rand01(),
                    crate::behavior::rand01(),
                );
            }
        }
        if close {
            self.wheelWindow = None;
            self.wheelLastPos = None;
        }
    }

    fn openSettingsPanel(&mut self) {
        if self.settingsWindow.is_some() {
            if let Some(sw) = self.settingsWindow.as_ref() {
                sw.window.set_visible(true);
                sw.window.focus_window();
            }
            return;
        }
        self.pendingOpenSettings = true;
    }

    fn openInventoryPanel(&mut self) {
        if let Some(iw) = self.inventoryWindow.as_ref() {
            iw.window.set_visible(true);
            iw.window.focus_window();
            return;
        }
        self.pendingOpenInventory = true;
    }

    fn openMusicPlayerPanel(&mut self) {
        if let Some(mw) = self.musicPlayerWindow.as_ref() {
            mw.window.set_visible(true);
            mw.window.focus_window();
            return;
        }
        self.pendingOpenMusicPlayer = true;
    }

    /// 创建 / 驱动音乐播放器窗口。
    fn tickMusicPlayerWindow(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        if self.pendingOpenMusicPlayer && self.musicPlayerWindow.is_none() {
            self.pendingOpenMusicPlayer = false;
            let root = appRoot(self.opts.configRoot.as_deref());
            let musicDir = crate::paths::musicDir(&root);
            match crate::musicPlayerWindow::MusicPlayerWindow::create(el, musicDir, &self.settings.uiFontFamily, &self.settings.musicPlayerStyle) {
                Ok(w) => {
                    w.requestRedraw();
                    self.musicPlayerWindow = Some(w);
                }
                Err(e) => log::warn!("musicPlayer window create failed: {e:?}"),
            }
        }
        // 播放器内 CFG 按钮被点击 → 打开/聚焦配置窗口
        if let Some(mw) = self.musicPlayerWindow.as_mut() {
            if mw.wantsOpenCfg() {
                mw.takeOpenCfg();
                if let Some(cw) = self.musicPlayerCfgWindow.as_ref() {
                    cw.window.set_visible(true);
                    cw.window.focus_window();
                } else {
                    self.pendingOpenMusicPlayerCfg = true;
                }
            }
        }
        let mut styleChanged = false;
        let close = match self.musicPlayerWindow.as_mut() {
            Some(mw) if mw.wantsRedraw() => {
                if let Some(ref mut p) = self.musicPlayer {
                    let files: &[crate::music::MusicFile] = if self.musicPlaying {
                        &self.musicFiles
                    } else {
                        &[]
                    };
                    mw.frame(p, files);
                }
                // 反向同步：用户可能在播放器内修改了外观设置
                if let Some(newStyle) = mw.take_style_changes() {
                    self.settings.musicPlayerStyle = newStyle;
                    styleChanged = true;
                }
                if !mw.window.is_visible().unwrap_or(false) {
                    mw.window.set_visible(true);
                }
                mw.pendingClose()
            }
            Some(mw) => mw.pendingClose(),
            None => return,
        };
        if styleChanged {
            self.persistSettings();
        }
        if close {
            self.musicPlayerWindow = None;
        }
    }

    /// 创建 / 驱动音乐播放器配置窗口；高亮转发 + 样式同步 + 关闭。
    fn tickMusicPlayerCfgWindow(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        if self.pendingOpenMusicPlayerCfg && self.musicPlayerCfgWindow.is_none() {
            self.pendingOpenMusicPlayerCfg = false;
            match crate::musicPlayerCfgWindow::MusicPlayerCfgWindow::create(
                el,
                &self.settings.uiFontFamily,
                &self.settings.musicPlayerStyle,
            ) {
                Ok(w) => {
                    w.requestRedraw();
                    self.musicPlayerCfgWindow = Some(w);
                }
                Err(e) => log::warn!("musicPlayer cfg window create failed: {e:?}"),
            }
        }

        let (close, change) = match self.musicPlayerCfgWindow.as_mut() {
            Some(cw) if cw.wantsRedraw() => {
                let change = cw.frame();
                if !cw.window.is_visible().unwrap_or(false) {
                    cw.window.set_visible(true);
                }
                (cw.pendingClose(), change)
            }
            Some(cw) => (
                cw.pendingClose(),
                crate::musicPlayerCfgWindow::CfgChange::default(),
            ),
            None => return,
        };

        // 样式变更 → 写回播放器 + 持久化
        if let Some(newStyle) = change.style {
            self.settings.musicPlayerStyle = newStyle;
            if let Some(mw) = self.musicPlayerWindow.as_mut() {
                mw.setStyle(&self.settings.musicPlayerStyle);
                mw.requestRedraw();
            }
            self.persistSettings();
        }

        // 高亮区域转发到播放器
        if let Some(mw) = self.musicPlayerWindow.as_mut() {
            mw.setHighlight(change.hoveredZone);
            mw.requestRedraw();
        }

        if close {
            if let Some(mw) = self.musicPlayerWindow.as_mut() {
                mw.setHighlight(None);
            }
            self.musicPlayerCfgWindow = None;
        }
    }

    /// 创建 / 驱动转盘配置窗口；布局变更实时同步到转盘窗口 + 持久化。
    fn tickWheelCfgWindow(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        if self.pendingOpenWheelCfg && self.wheelCfgWindow.is_none() {
            self.pendingOpenWheelCfg = false;
            match crate::wheelCfgWindow::WheelCfgWindow::create(
                el,
                &self.settings.uiFontFamily,
                &self.settings.wheel,
            ) {
                Ok(w) => {
                    w.requestRedraw();
                    self.wheelCfgWindow = Some(w);
                }
                Err(e) => log::warn!("wheel cfg window create failed: {e:?}"),
            }
        }

        let (close, change) = match self.wheelCfgWindow.as_mut() {
            Some(cw) if cw.wantsRedraw() => {
                let change = cw.frame();
                if !cw.window.is_visible().unwrap_or(false) {
                    cw.window.set_visible(true);
                }
                (cw.pendingClose(), change)
            }
            Some(cw) => (
                cw.pendingClose(),
                crate::wheelCfgWindow::CfgChange::default(),
            ),
            None => return,
        };

        // 外观变更 → 写回转盘窗口 + 持久化（窗口位置/大小不持久化）
        if let Some(mut newCfg) = change.cfg {
            // 保持位置/大小为默认值，不写入配置文件
            let def = crate::settings::WheelConfig::default();
            newCfg.windowPosX = def.windowPosX;
            newCfg.windowPosY = def.windowPosY;
            newCfg.windowWidth = def.windowWidth;
            newCfg.windowHeight = def.windowHeight;
            self.settings.wheel = newCfg;
            if let Some(ww) = self.wheelWindow.as_mut() {
                ww.setConfig(&self.settings.wheel);
                ww.requestRedraw();
            }
            self.persistSettings();
        }

        // 高亮区域转发到转盘窗口
        if let Some(ww) = self.wheelWindow.as_mut() {
            ww.setHighlight(change.hoveredZone);
            ww.requestRedraw();
        }

        if close {
            if let Some(ww) = self.wheelWindow.as_mut() {
                ww.setHighlight(None);
            }
            self.wheelCfgWindow = None;
        }
    }

    /// 创建 / 驱动仓库窗口；处理拾取 / 调试加库存 / 关闭。
    fn tickInventoryWindow(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        if self.pendingOpenInventory && self.inventoryWindow.is_none() {
            self.pendingOpenInventory = false;
            let root = appRoot(self.opts.configRoot.as_deref());
            let foodsDir = crate::paths::foodsDir(&root);
            let accDir = crate::paths::inventoryDir(&root);
            let slotBgDir = crate::paths::slotBgDir(&root);
            match crate::inventoryWindow::InventoryWindow::create(el, &foodsDir, &accDir, &slotBgDir, self.settings.inventoryScale, &self.settings.uiFontFamily) {
                Ok(w) => {
                    w.requestRedraw();
                    // 在桌宠周围弹出：让圆环中心≈桌宠中心（仓库窗 760×600，圆环居中）。
                    let ix = (self.physics.x + PET_W as f32 * 0.5 - 380.0).max(0.0);
                    let iy = (self.physics.y + PET_H as f32 * 0.5 - 300.0).max(0.0);
                    let _ = w.window.set_outer_position(winit::dpi::LogicalPosition::new(ix, iy));
                    self.inventoryWindow = Some(w);
                }
                Err(e) => log::warn!("inventory window create failed: {e:?}"),
            }
        }
        let (close, change) = match self.inventoryWindow.as_mut() {
            // 仅在需要重绘时 present（阻塞式 Fifo），空闲时跳过 → 不拖慢主循环 / 桌宠动画。
            Some(iw) if iw.wantsRedraw() => {
                // 同时可变借 settings 的两个不相交字段（inventory / inventoryStyle），Rust 允许。
                let change = iw.frame(&mut self.settings.inventory, &mut self.settings.inventoryStyle);
                // 首帧渲染成功后显示窗口，消除加载白块。
                if !iw.window.is_visible().unwrap_or(false) {
                    iw.window.set_visible(true);
                }
                (iw.pendingClose() || change.closed, Some(change))
            }
            Some(iw) => (iw.pendingClose(), None),
            None => return,
        };
        if let Some(change) = change {
            if let Some(id) = change.toggleEquip {
                let equipped = self
                    .settings
                    .inventory
                    .backpackById(&id)
                    .map_or(false, |b| b.equipped);
                if equipped {
                    self.settings.inventory.unequipBackpack(&id);
                } else {
                    self.settings.inventory.equipBackpack(&id);
                }
                self.persistSettings();
            }
            if let Some(id) = change.openPack {
                // viewMode 切换已在窗口内处理；此处无需额外动作。
                let _ = id;
            }
            if let Some(idx) = change.pickMain {
                // 已在拖拽中则不重复拾取；投放由 tickFeedDrag 的右键释放检测处理。
                if !self.feedDrag.isCarrying() {
                    if let Some(item) = self.settings.inventory.mainTake(idx) {
                        self.feedSource = Some(FeedSource::Main(idx));
                        self.startFeedDragItem(el, item);
                        self.persistSettings();
                        // 取物后立即触发重绘，否则仓库窗口下帧跳过渲染，旧图标残留。
                        if let Some(iw) = self.inventoryWindow.as_mut() {
                            iw.requestRedraw();
                        }
                    }
                }
            }
            if let Some((packId, slotIdx)) = change.pickPack {
                if !self.feedDrag.isCarrying() {
                    if let Some(item) = self.settings.inventory.bpTake(&packId, slotIdx) {
                        self.feedSource = Some(FeedSource::Pack(packId, slotIdx));
                        self.startFeedDragItem(el, item);
                        self.persistSettings();
                        if let Some(iw) = self.inventoryWindow.as_mut() {
                            iw.requestRedraw();
                        }
                    }
                }
            }
            // 外观样式被改动 → 持久化（调好即存，永久保留）。
            if change.styleChanged {
                self.persistSettings();
            }
        }
        if close {
            self.inventoryWindow = None;
        }
    }

    /// 按物品启动拖拽。食物走 feedDrag 跟随窗（贴图来自 foodsDir）；非食物从 accDir 加载。
    fn startFeedDragItem(&mut self, el: &winit::event_loop::ActiveEventLoop, item: crate::item::Item) {
        self.startFeedDrag(el, &item.id);
        // 记录正在拖拽的物品 kind，结算时区分喂食/使用。
        self.feedDragItemKind = Some(item.kind);
    }

    /// 启动喂食拖拽：进入 Carrying，创建/复用独立透明窗跟随光标。
    fn startFeedDrag(&mut self, el: &winit::event_loop::ActiveEventLoop, foodId: &str) {
        self.feedDrag.start(foodId);
        let root = appRoot(self.opts.configRoot.as_deref());
        let foodsDir = crate::paths::foodsDir(&root);
        let accDir = crate::paths::inventoryDir(&root);
        match self.feedDragWindow.as_mut() {
            Some(w) => w.setFood(foodId),
            None => match crate::feedDrag::FeedDragWindow::create(el, &foodsDir, &accDir, foodId) {
                Ok(w) => { self.feedDragWindow = Some(w); }
                Err(e) => log::warn!("feed drag window create failed: {e:?}"),
            },
        }
        // 定位 + 渲染首帧 + 显示。关键顺序：必须先渲染再显示。
        if let (Some(w), Some((sx, sy))) = (self.feedDragWindow.as_mut(), getCursorScreen()) {
            let dpi = self.dpiScale.max(0.001);
            w.showAtCursor(sx / dpi, sy / dpi);
            w.requestRedraw();
        }
    }

    /// 取消喂食拖拽：回 Idle，隐藏小窗。右键取消/退出时调用。
    fn cancelFeedDrag(&mut self) {
        if let Some(src) = self.feedSource.take() {
            // 取消拖拽：物品按 kind 放回来源格（食物回退）。
            let id = self.feedDrag.currentFood().map(|s| s.to_string());
            if let Some(id) = id {
                let item = itemFromIdKind(&id, self.feedDragItemKind);
                match src {
                    FeedSource::Main(idx) => { self.settings.inventory.mainPlace(idx, item); }
                    FeedSource::Pack(p, s) => { self.settings.inventory.bpPlace(&p, s, item); }
                }
            }
            self.feedDragItemKind = None;
            // 归还物品后立即写盘，防止取消后退出丢失物品。
            self.persistSettings();
        }
        self.endFeedDragUi();
    }

    /// 清理拖拽 UI 状态（不归还物品）：令 feedDrag 回 Idle，隐藏独立窗。
    fn endFeedDragUi(&mut self) {
        self.feedDrag.cancel();
        if let Some(w) = self.feedDragWindow.as_ref() {
            w.window.set_visible(false);
        }
    }

    /// 每帧驱动拖拽窗：跟随光标移动、渲染；右键松手时投放结算。
    fn tickFeedDrag(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        if !self.feedDrag.isCarrying() {
            self.feedDragMouseWasDown = false;
            return;
        }
        // 左键取消拖拽。
        if globalLeftDown() {
            self.cancelFeedDrag();
            self.feedDragMouseWasDown = false;
            return;
        }
        // ~60fps 节流 SetWindowPos + render（昂贵操作）。
        let now = std::time::Instant::now();
        if now.duration_since(self.lastFeedDragRender).as_secs_f32() >= 0.016 {
            self.lastFeedDragRender = now;
            if let (Some(w), Some((sx, sy))) = (self.feedDragWindow.as_mut(), getCursorScreen()) {
                let dpi = self.dpiScale.max(0.001);
                w.followCursor(sx / dpi, sy / dpi);
                w.window.set_visible(true);
                w.render();
                // 首帧渲染成功后设穿透——避免 WS_EX_TRANSPARENT 干扰首次呈现。
                w.enablePassthrough();
            }
        }
        // 右键下降沿检测：不节流，必须每帧执行。
        let down = globalRightDown();
        if self.feedDragMouseWasDown && !down {
            self.resolveFeedDrop(el);
            self.feedDragMouseWasDown = false;
        } else {
            self.feedDragMouseWasDown = down;
        }
    }

    /// 投放结算：优先级 — 命中桌宠身体则喂食/使用；命中仓库空格则放入该格；
    /// 否则掉落为桌面物品（不再退回来源格）。命中判定用屏幕坐标。
    fn resolveFeedDrop(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        let foodId = match self.feedDrag.currentFood() {
            Some(f) => f.to_string(),
            None => return,
        };
        let kind = self.feedDragItemKind;
        if self.cursorHitsBodyScreen() {
            // 命中桌宠：食物→喂食生效；非食物→「使用」占位反馈。物品已从来源格取出，命中即消耗。
            // 食品按 eatLines、饮品按 drinkLines 选台词；无对应台词则不弹气泡。
            let mut feedLine: Option<String> = None;
            if matches!(kind, Some(crate::item::ItemKind::Food)) {
                if let Some(def) = crate::food::foodById(&foodId) {
                    self.settings.needs.feed(def.hunger, def.thirst, def.mood);
                    let picked = match def.kind {
                        crate::food::FoodKind::Eat => self.chatter.pickEat(crate::behavior::rand01()),
                        crate::food::FoodKind::Drink => self.chatter.pickDrink(crate::behavior::rand01()),
                    };
                    feedLine = picked.map(|s| s.to_string());
                }
            }
            self.feedingStartedAt = Some(std::time::Instant::now());
            self.feedingHappyUntil =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(1500));
            self.pickedMotion = Some((
                "paw".into(),
                std::time::Instant::now() + std::time::Duration::from_millis(1200),
            ));
            self.needsForceShowSec = 2.5;
            if let Some(line) = feedLine {
                self.sayText(&line, 1800);
            }
            self.feedSource = None; // 消耗，不归还。
            self.persistSettings();
        } else if let Some(target) = self.cursorHitsInventorySlotAny() {
            // 命中仓库某格：空格则放入，占位则交换，两格都触发 pop 动画。
            let carried = itemFromIdKind(&foodId, kind);
            let old = match &target {
                crate::inventoryWindow::SlotTarget::Main(idx) => {
                    self.settings.inventory.mainPlace(*idx, carried)
                }
                crate::inventoryWindow::SlotTarget::Pack(packId, slotIdx) => {
                    self.settings.inventory.bpPlace(packId, *slotIdx, carried)
                }
            };
            // target 格原有物品（如有）放入来源格，完成交换。
            if let Some(oldItem) = old {
                match self.feedSource.take() {
                    Some(FeedSource::Main(idx)) => {
                        self.settings.inventory.mainPlace(idx, oldItem);
                    }
                    Some(FeedSource::Pack(p, s)) => {
                        self.settings.inventory.bpPlace(&p, s, oldItem);
                    }
                    None => {}
                }
            } else {
                self.feedSource = None; // 移入空格，来源置空。
            }
            if let Some(iw) = self.inventoryWindow.as_mut() {
                iw.triggerSlotPop(target);
            }
            self.persistSettings();
        } else {
            // 未命中仓库/桌宠：物品从仓库取出后掉落到桌面光标处，不再退回来源格。
            self.feedSource = None;
            let item = itemFromIdKind(&foodId, kind);
            let root = appRoot(self.opts.configRoot.as_deref());
            let foodsDir = crate::paths::foodsDir(&root);
            let accDir = crate::paths::inventoryDir(&root);
            let (sw, sh) = if let Some(monitor) = el.primary_monitor() {
                let sf = monitor.scale_factor() as f32;
                let size = monitor.size();
                (size.width as f32 / sf, size.height as f32 / sf)
            } else {
                (1280.0, 720.0)
            };
            let horizonY = sh - 60.0;
            let win = crate::dropItem::WIN as f32;
            let (topLeftX, topLeftY) = match getCursorScreen() {
                Some((sx, sy)) => {
                    let dpi = self.dpiScale.max(0.001);
                    let cx = (sx / dpi - win * 0.5).max(0.0).min(sw - win);
                    let cy = (sy / dpi - win * 0.5).max(0.0).min(sh - win);
                    (cx, cy)
                }
                None => {
                    let restY = horizonY - win;
                    (60.0 + crate::behavior::rand01() * (sw - 120.0 - win).max(1.0),
                     60.0 + crate::behavior::rand01() * (restY - 60.0).max(1.0))
                }
            };
            const MAX_DROPS: usize = 8;
            if self.droppedItems.len() < MAX_DROPS {
                match crate::dropItem::DroppedItem::create(
                    el, &foodsDir, &accDir, item, topLeftX, topLeftY, horizonY,
                ) {
                    Ok(mut d) => {
                        d.render();
                        d.window.set_visible(true);
                        self.droppedItems.push(d);
                    }
                    Err(e) => log::warn!("拖拽掉落创建失败: {e:?}"),
                }
            } else {
                log::warn!("掉落物已达上限 {MAX_DROPS}，拖拽掉落忽略");
            }
            self.persistSettings();
        }
        self.feedDragItemKind = None;
        // 各分支已分别处理消耗/落格/归还；此处只清理拖拽 UI 状态。
        self.endFeedDragUi();
    }

    /// 拖放落点是否命中仓库窗口某空格（窗口可见且光标落在空格 rect 内）。
    fn cursorHitsInventorySlot(&self) -> Option<crate::inventoryWindow::SlotTarget> {
        let (sx, sy) = getCursorScreen()?;
        self.inventoryWindow.as_ref()?.slotAtScreen(sx, sy)
    }

    /// 拖放落点是否命中仓库窗口任意格（不分空/占位），用于交换。
    fn cursorHitsInventorySlotAny(&self) -> Option<crate::inventoryWindow::SlotTarget> {
        let (sx, sy) = getCursorScreen()?;
        self.inventoryWindow.as_ref()?.anySlotAtScreen(sx, sy)
    }

    /// 用屏幕坐标判断光标是否命中桌宠身体（喂食投放命中）。
    fn cursorHitsBodyScreen(&self) -> bool {
        let cursor = match getCursorScreen() {
            Some(c) => c,
            None => return false,
        };
        let renderer = match self.renderer.as_ref() {
            Some(r) => r,
            None => return false,
        };
        if self.lastRenderedLimbs.is_empty() {
            return false;
        }
        let dpi = self.dpiScale.max(0.001);
        let winPhysX = self.physics.x * dpi;
        let winPhysY = self.physics.y * dpi;
        let scrW = renderer.config.width as f32;
        let scrH = renderer.config.height as f32;
        let cxp = cursor.0 - winPhysX;
        let cyp = cursor.1 - winPhysY;
        if cxp < 0.0 || cyp < 0.0 || cxp >= scrW || cyp >= scrH {
            return false;
        }
        let isRight = self.physics.facing > 0;
        let facingSign = if isRight { 1.0 } else { -1.0 };
        let unitToPx = PIXELS_PER_UNIT * self.stageScale * dpi;
        let pxRatio = unitToPx / PIXELS_PER_UNIT;
        anyLimbBboxContains(
            &self.lastRenderedLimbs,
            cxp,
            cyp,
            scrW * 0.5,
            scrH * CENTER_Y_RATIO,
            facingSign,
            unitToPx,
            pxRatio,
        )
    }

    /// 需求气泡选词：心情/饥饿/口渴 偏高或偏低时，从 chatter.json 的 needsLines 池随机取一句。
    /// 阈值：心情 >80 高 / <60 低；饥饿、口渴 >90 高 / <60 低；中间区间静默。各维度独立 30s 冷却。
    /// 台词内容由用户在 chatter.json 编辑；池为空则该档不触发。
    fn pickNeedsBubble(&mut self, dt: f32) -> Option<String> {
        use crate::chatter::NeedsCue;
        for c in self.bubbleCooldown.iter_mut() {
            if *c > 0.0 {
                *c = (*c - dt).max(0.0);
            }
        }
        if !self.settings.needsConfig.bubblesEnabled {
            return None;
        }
        // 拖拽中 / 喂食拖拽中不打扰；已有未过期气泡也不抢。
        if self.input.dragging || self.feedDrag.isCarrying() {
            return None;
        }
        if let Some(b) = self.bubble.as_ref() {
            if std::time::Instant::now() < b.expireAt {
                return None;
            }
        }
        // 各维度选出本帧适用的 cue（高/低/无）。心情阈值与饥饿口渴不同。
        const MOOD_HIGH: f32 = 80.0;
        const MOOD_LOW: f32 = 60.0;
        const FOOD_HIGH: f32 = 90.0;
        const FOOD_LOW: f32 = 60.0;
        let n = self.settings.needs;
        let cueFor = |value: f32, low: f32, high: f32, lowCue: NeedsCue, highCue: NeedsCue| {
            if value < low {
                Some(lowCue)
            } else if value > high {
                Some(highCue)
            } else {
                None
            }
        };
        // (冷却槽位, 本维度 cue)
        let candidates = [
            (0usize, cueFor(n.hunger, FOOD_LOW, FOOD_HIGH, NeedsCue::HungerLow, NeedsCue::HungerHigh)),
            (1, cueFor(n.thirst, FOOD_LOW, FOOD_HIGH, NeedsCue::ThirstLow, NeedsCue::ThirstHigh)),
            (2, cueFor(n.mood, MOOD_LOW, MOOD_HIGH, NeedsCue::MoodLow, NeedsCue::MoodHigh)),
        ];
        for (idx, cue) in candidates {
            if let Some(cue) = cue {
                if self.bubbleCooldown[idx] <= 0.0 {
                    if let Some(text) = self.chatter.pickNeeds(cue, crate::behavior::rand01()) {
                        self.bubbleCooldown[idx] = 30.0;
                        return Some(text.to_string());
                    }
                }
            }
        }
        None
    }

    /// 空闲随机闲聊：递减计时，到点时随机选一句台词并重置间隔。需求抱怨优先，故此项在其之后调用。
    /// 频率由设置 NeedsConfig.chatterMin/MaxSec 决定（用户可在设置面板调）。
    fn pickChatterBubble(&mut self, dt: f32) -> Option<String> {
        let cfg = &self.settings.needsConfig;
        if !cfg.chatterEnabled || self.chatter.isEmpty() {
            return None;
        }
        // 拖拽 / 喂食拖拽中不打扰；已有未过期气泡不抢。
        if self.input.dragging || self.feedDrag.isCarrying() {
            return None;
        }
        if let Some(b) = self.bubble.as_ref() {
            if std::time::Instant::now() < b.expireAt {
                return None;
            }
        }
        self.chatterTimer -= dt;
        if self.chatterTimer > 0.0 {
            return None;
        }
        // 重置下次间隔：[min, max] 区间随机。
        let min = cfg.chatterMinSec.max(1.0);
        let max = cfg.chatterMaxSec.max(min);
        self.chatterTimer = min + crate::behavior::rand01() * (max - min);
        self.chatter
            .pick(crate::behavior::rand01())
            .map(|s| s.to_string())
    }


    fn applyDpiMode(&mut self) {
        if let Some(w) = self.window.as_ref() {
            self.dpiScale = match self.settings.dpiMode {
                DpiMode::System => w.scale_factor() as f32,
                DpiMode::Force1x => 1.0,
                DpiMode::Force2x => 2.0,
            };
        }
    }

    fn persistSettings(&self) {
        let root = appRoot(self.opts.configRoot.as_deref());
        let dir = configsDir(&root);
        if let Err(e) = saveSettings(&dir, &self.opts.petId, &self.settings) {
            log::warn!("save settings failed: {e:?}");
        }
    }

    /// 在屏幕随机逻辑坐标处生成一个掉落物窗（上限 8，超出忽略并告警）。
    fn spawnDroppedItem(&mut self, el: &winit::event_loop::ActiveEventLoop, item: crate::item::Item) {
        const MAX_DROPS: usize = 8;
        if self.droppedItems.len() >= MAX_DROPS {
            log::warn!("掉落物已达上限 {MAX_DROPS}，忽略新掉落 {}", item.id);
            return;
        }
        let root = appRoot(self.opts.configRoot.as_deref());
        let foodsDir = crate::paths::foodsDir(&root);
        let accDir = crate::paths::inventoryDir(&root);
        // 取主屏逻辑尺寸，用于随机落点。
        let (sw, sh) = if let Some(monitor) = el.primary_monitor() {
            let sf = monitor.scale_factor() as f32;
            let size = monitor.size();
            (size.width as f32 / sf, size.height as f32 / sf)
        } else {
            (1280.0, 720.0)
        };
        // 地平线 = 桌宠脚底线（屏幕逻辑 Y = 屏高 - 60，与 initBoundsAndPosition 一致）。
        let horizonY = sh - 60.0;
        // 停驻线（窗口左上角）：窗口底边贴地平线。
        let restY = horizonY - crate::dropItem::WIN as f32;
        // x 随机横跨屏幕（留边，保证窗口整体在屏内）。
        let win = crate::dropItem::WIN as f32;
        let topLeftX = 60.0 + crate::behavior::rand01() * (sw - 120.0 - win).max(1.0);
        // y 在地平线以上的半空随机（[60, restY]）：高于地平线则随后慢慢落下，绝不低于地平线。
        let topLeftY = 60.0 + crate::behavior::rand01() * (restY - 60.0).max(1.0);
        match crate::dropItem::DroppedItem::create(el, &foodsDir, &accDir, item, topLeftX, topLeftY, horizonY) {
            Ok(mut d) => {
                d.render();
                d.window.set_visible(true);
                self.droppedItems.push(d);
            }
            Err(e) => log::warn!("生成掉落物失败: {e:?}"),
        }
    }

    /// 每帧驱动掉落物：排空 pendingDrops，右键按下沿抓取，拖动跟随，右键松开沿结算落点。
    fn tickDroppedItems(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        // 先把 pendingDrops 排空（菜单触发后在此处安全建窗）。
        for item in std::mem::take(&mut self.pendingDrops) {
            self.spawnDroppedItem(el, item);
        }
        if self.droppedItems.is_empty() {
            self.dropRightWasDown = globalRightDown(); // 保持右键边沿状态机同步，避免脏值误触
            return;
        }
        // 节流到 ~60fps：about_to_wait 是 Poll，主循环不限帧率高速空转。掉落物下落/拖动时
        // 每一轮都调 set_outer_position(=Windows SetWindowPos) + render()，高频会打爆 DWM 合成器
        // 把桌宠动画卡住。这里把掉落物的移动/重绘与右键边沿处理限制到 ~16ms 一次；
        // 物理用真实 dt（与帧率无关），节流不改变下落速度，只是不再每帧 SetWindowPos。
        let now = std::time::Instant::now();
        let dt = (now - self.lastDropTickAt).as_secs_f32();
        if dt < 0.016 {
            return;
        }
        self.lastDropTickAt = now;
        let dt = dt.min(0.05); // clamp 防卡顿后大跳
        let down = globalRightDown();
        // 拖动跟随 + 非拖动落体：始终每帧执行（与右键短路无关，否则在途/下落掉落物会卡住）。
        let cursor = getCursorScreen();
        let dpi = self.dpiScale.max(0.001);
        for d in self.droppedItems.iter_mut() {
            d.tickAnim(dt); // 推进抓取反馈动画计时器
            let animActive = d.grabAnimActive(); // 动画进行中：需要持续重绘直到归位
            if d.dragging {
                if let Some((sx, sy)) = cursor {
                    d.followCursor(sx / dpi, sy / dpi);
                    d.render();
                }
            } else if d.tickFall(dt) || animActive {
                // 在地平线以上则按重力下落（移动了才重绘）；动画进行中也重绘。
                d.render();
            }
        }
        // 右键共享冲突防护：feedDrag 正携带时右键归它（取消喂食），掉落物不抢右键，只同步边沿状态避免脏值。
        if self.feedDrag.isCarrying() {
            self.dropRightWasDown = down;
            return;
        }
        // 右键按下沿：找光标命中的第一个掉落物，开始拖动。
        if !self.dropRightWasDown && down {
            if let Some((sx, sy)) = getCursorScreen() {
                if let Some(d) = self.droppedItems.iter_mut().find(|d| dropWindowContains(d, sx, sy)) {
                    d.dragging = true;
                    d.startGrabAnim(); // 右键抓取反馈动画：挤压拉伸脉冲
                }
            }
        }
        // 右键松开沿：结算正在拖动的掉落物落点。
        if self.dropRightWasDown && !down {
            self.resolveDropRelease();
        }
        self.dropRightWasDown = down;
    }

    /// 右键松开时结算正在拖动的掉落物：背包命中身体→穿戴；食物/配饰命中身体→喂食；
    /// 食物/配饰命中仓库空格→入库；否则留原地停止拖动。
    fn resolveDropRelease(&mut self) {
        let idx = match self.droppedItems.iter().position(|d| d.dragging) {
            Some(i) => i,
            None => return,
        };
        // 先 clone item 避免借用冲突（后续需要调用 self 的其他方法）。
        let item = self.droppedItems[idx].item.clone();
        let onBody = self.cursorHitsBodyScreen();
        let slot = self.cursorHitsInventorySlot();
        let mut consumed = false;
        match item.kind {
            crate::item::ItemKind::Backpack => {
                if onBody {
                    self.settings.inventory.addBackpack(&item.id, nowUnixMs());
                    self.settings.inventory.equipBackpack(&item.id);
                    self.persistSettings();
                    if let Some(line) = self.chatter.pickEquip(crate::behavior::rand01()) {
                        let line = line.to_string();
                        self.sayText(&line, 1500);
                    }
                    consumed = true;
                }
                // 背包不放仓库格，靠穿戴消耗。
            }
            crate::item::ItemKind::Food | crate::item::ItemKind::Accessory => {
                // 注：ItemKind::Accessory 当前 randomRewardItem 不产出，此 Accessory 分支暂不可达；
                // 若日后奖励表加入配饰，需明确其落点行为（穿戴 / 入库 / 其它）。
                if onBody {
                    // 食物：触发 needs.feed，并按食品/饮品选对应台词（无台词则不弹气泡）。
                    let mut line: Option<String> = None;
                    if item.kind == crate::item::ItemKind::Food {
                        if let Some(def) = crate::food::foodById(&item.id) {
                            self.settings.needs.feed(def.hunger, def.thirst, def.mood);
                            let picked = match def.kind {
                                crate::food::FoodKind::Eat => self.chatter.pickEat(crate::behavior::rand01()),
                                crate::food::FoodKind::Drink => self.chatter.pickDrink(crate::behavior::rand01()),
                            };
                            line = picked.map(|s| s.to_string());
                        }
                    }
                    self.feedingStartedAt = Some(std::time::Instant::now());
                    self.feedingHappyUntil =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(1500));
                    self.needsForceShowSec = 2.5;
                    if let Some(line) = line {
                        self.sayText(&line, 1800);
                    }
                    self.persistSettings();
                    consumed = true;
                } else if let Some(target) = slot {
                    match target.clone() {
                        crate::inventoryWindow::SlotTarget::Main(i) => {
                            self.settings.inventory.mainPlace(i, item.clone());
                            if let Some(iw) = self.inventoryWindow.as_mut() {
                                iw.triggerSlotPop(crate::inventoryWindow::SlotTarget::Main(i));
                            }
                        }
                        crate::inventoryWindow::SlotTarget::Pack(p, s) => {
                            self.settings.inventory.bpPlace(&p, s, item.clone());
                            if let Some(iw) = self.inventoryWindow.as_mut() {
                                iw.triggerSlotPop(crate::inventoryWindow::SlotTarget::Pack(p, s));
                            }
                        }
                    }
                    self.persistSettings();
                    consumed = true;
                }
            }
        }
        if consumed {
            // 关闭并移除该掉落物窗。
            let d = self.droppedItems.remove(idx);
            d.window.set_visible(false);
            drop(d);
        } else {
            // 未命中：留在松开处，停止拖动。
            self.droppedItems[idx].dragging = false;
        }
    }
}

#[cfg(windows)]
fn getCursorScreen() -> Option<(f32, f32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut pt) };
    if ok == 0 {
        None
    } else {
        Some((pt.x as f32, pt.y as f32))
    }
}

#[cfg(not(windows))]
fn getCursorScreen() -> Option<(f32, f32)> {
    None
}

#[cfg(windows)]
fn globalLeftDown() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    (unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } as u16 & 0x8000) != 0
}

#[cfg(windows)]
fn globalRightDown() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_RBUTTON};
    (unsafe { GetAsyncKeyState(VK_RBUTTON as i32) } as u16 & 0x8000) != 0
}

#[cfg(not(windows))]
fn globalLeftDown() -> bool {
    false
}

#[cfg(not(windows))]
fn globalRightDown() -> bool {
    false
}

/// 判断屏幕物理坐标 (sx, sy) 是否落在掉落物窗的物理矩形内。
/// 用 outer_position（物理像素）+ inner_size（物理像素）构成矩形。
fn dropWindowContains(d: &crate::dropItem::DroppedItem, sx: f32, sy: f32) -> bool {
    let pos = match d.window.outer_position() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let size = d.window.inner_size();
    let x0 = pos.x as f32;
    let y0 = pos.y as f32;
    let x1 = x0 + size.width as f32;
    let y1 = y0 + size.height as f32;
    sx >= x0 && sx < x1 && sy >= y0 && sy < y1
}

fn buildWindowAttributes(title: &str) -> winit::window::WindowAttributes {
    let attrs = Window::default_attributes()
        .with_title(title)
        .with_inner_size(LogicalSize::new(PET_W, PET_H))
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_visible(false); // 首帧渲染成功后再显示，避免白闪
    let attrs = if let Some(icon) = loadAppIcon() {
        attrs.with_window_icon(Some(icon))
    } else {
        attrs
    };
    applyPlatformAttrs(attrs)
}

fn loadAppIcon() -> Option<winit::window::Icon> {
    const PNG: &[u8] = include_bytes!("../icons/icon.png");
    let img = image::load_from_memory(PNG).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

#[cfg(windows)]
fn applyPlatformAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    attrs.with_skip_taskbar(true).with_drag_and_drop(false)
}

#[cfg(not(windows))]
fn applyPlatformAttrs(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs
}

/// 开机自启动：写入/删除注册表 Run 键。
/// 路径指向当前 exe（含 `--pet-id <id>` 参数），以便开机后带相同配置启动。
#[cfg(windows)]
fn applyAutoStart(enabled: bool) {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ, REG_OPTION_NON_VOLATILE,
    };
    let exePath = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log::warn!("applyAutoStart: current_exe failed: {e:?}");
            return;
        }
    };
    let exeStr = exePath.to_string_lossy().to_string();
    // 注册表值名基于 petId 以支持多桌宠（但当前上下文无 petId，使用固定名称）。
    // 实际上开机自启只需一个实例；多实例由 --pet-id 区分。
    let valueName: Vec<u16> = "CasualtiesUnknownPet"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let subKey: Vec<u16> = r"Software\Microsoft\Windows\CurrentVersion\Run"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    if enabled {
        // 写 Run 键
        let mut hKey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
        let ret = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subKey.as_ptr(),
                0,
                std::ptr::null_mut(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut hKey,
                std::ptr::null_mut(),
            )
        };
        if ret != ERROR_SUCCESS || hKey.is_null() {
            log::warn!("applyAutoStart: RegCreateKeyExW failed ({ret})");
            return;
        }
        let valueBytes: Vec<u8> = format!("{}\0", exeStr).into_bytes();
        let ret = unsafe {
            RegSetValueExW(
                hKey,
                valueName.as_ptr(),
                0,
                REG_SZ,
                valueBytes.as_ptr(),
                valueBytes.len() as u32,
            )
        };
        if ret != ERROR_SUCCESS {
            log::warn!("applyAutoStart: RegSetValueExW failed ({ret})");
        } else {
            log::info!("applyAutoStart: 已添加到启动项 -> {}", exeStr);
        }
        unsafe { RegCloseKey(hKey); }
    } else {
        // 删 Run 键
        let mut hKey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
        let ret = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subKey.as_ptr(),
                0,
                std::ptr::null_mut(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut hKey,
                std::ptr::null_mut(),
            )
        };
        if ret != ERROR_SUCCESS || hKey.is_null() {
            log::warn!("applyAutoStart: RegCreateKeyExW (delete) failed ({ret})");
            return;
        }
        let ret = unsafe { RegDeleteValueW(hKey, valueName.as_ptr()) };
        if ret != ERROR_SUCCESS {
            // 值不存在也算成功
            log::info!("applyAutoStart: 启动项已移除（或本来不存在）");
        } else {
            log::info!("applyAutoStart: 已从启动项移除");
        }
        unsafe { RegCloseKey(hKey); }
    }
}

#[cfg(not(windows))]
fn applyAutoStart(_enabled: bool) {
    // 非 Windows 平台暂不支持
}

/// 单实例锁：用命名互斥量保证同一 petId 只有一只桌宠。
/// 返回 true=本进程是唯一实例（可继续）；false=已有实例运行（应退出）。
/// 互斥量句柄不关闭，活到进程退出由 OS 回收，保证锁覆盖整个进程生命周期。
#[cfg(windows)]
fn acquireSingleInstance(petId: &str) -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    // 名字含 petId：不同 petId 各允许一只；相同 petId 第二个被拦截。
    let name: Vec<u16> = format!("MyPet_singleInstance_{petId}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        // 创建失败（极少见）：放行，宁可多开也不静默不启动。
        return true;
    }
    // 句柄故意不 CloseHandle：裸句柄活到进程退出由 OS 回收。
    let lastErr = unsafe { GetLastError() };
    lastErr != ERROR_ALREADY_EXISTS
}

#[cfg(not(windows))]
fn acquireSingleInstance(_petId: &str) -> bool {
    true
}

pub fn runApp(opts: CliOptions) -> Result<()> {
    if !acquireSingleInstance(&opts.petId) {
        log::info!("pet-runtime: 已有 petId={} 实例运行，退出", opts.petId);
        return Ok(());
    }
    log::info!(
        "pet-runtime starting petId={} forceSystemFont={}",
        opts.petId,
        opts.forceSystemFont
    );
    let event_loop = EventLoop::new().context("create event loop")?;
    let mut app = PetApp::new(opts);
    event_loop.run_app(&mut app).context("run app")?;
    Ok(())
}

fn applyPlatformPostCreate(_window: &Window) {}

/// 计算单只翅膀在 unit 局部空间（朝左参考系）中的最终中心位置 (x, y) 与世界旋转 deg；按 SkinSync 父子链累加。
fn computeWingPose(
    piece: WingPiece,
    isLower: bool,
    upperHeightPx: f32,
    parentUnitX: f32,
    parentUnitY: f32,
    parentUnitRotDeg: f32,
    dynLocalDeg: f32,
) -> (f32, f32, f32) {
    let (localOffX, localOffY) = pieceLocalOffset(piece, isLower, upperHeightPx);
    let rad = parentUnitRotDeg.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();
    let rotOffX = localOffX * cos - localOffY * sin;
    let rotOffY = localOffX * sin + localOffY * cos;
    let unitX = parentUnitX + rotOffX;
    let unitY = parentUnitY + rotOffY;
    let unitRot = parentUnitRotDeg + pieceBaseAngle(piece) + dynLocalDeg;
    (unitX, unitY, unitRot)
}


fn makePlayer(controllerPath: &std::path::Path, clipsByName: &HashMap<String, AnimClip>) -> Option<AnimPlayer> {
    if !controllerPath.exists() {
        return None;
    }
    match loadController(controllerPath) {
        Ok(ctrl) => Some(AnimPlayer::new(ctrl, clipsByName.clone())),
        Err(e) => {
            log::warn!("load controller {} failed: {e:?}", controllerPath.display());
            None
        }
    }
}

/// 当前 Unix 毫秒时间戳（背包 24h 时限计时用）。
fn nowUnixMs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 推进爬墙状态机；非爬墙时执行常规物理步进，返回当前是否处于爬墙。
fn tickClimb(
    physics: &mut PhysicsState,
    cfg: &PhysicsConfig,
    bounds: &ScreenBounds,
    behavior: &BehaviorState,
    climb: &mut ClimbState,
    lastClimbEndAt: &mut Option<std::time::Instant>,
    dragging: bool,
    dt: f32,
    allowClimb: bool,
) -> bool {
    let walking = matches!(
        behavior.current,
        crate::behavior::BehaviorName::Walk | crate::behavior::BehaviorName::Run
    );
    match *climb {
        ClimbState::Active { side, climbedPx } => {
            physics.x = if side < 0 { bounds.minX } else { bounds.maxX - cfg.windowW };
            physics.facing = side;
            physics.vx = 0.0;
            physics.vy = -CLIMB_SPEED;
            physics.grounded = false;
            physics.y -= CLIMB_SPEED * dt;
            let climbed = climbedPx + CLIMB_SPEED * dt;
            let topReached = physics.y <= bounds.groundY - CLIMB_MAX_DIST;
            let canDrop = climbed >= CLIMB_MIN_DIST;
            if topReached || (canDrop && crate::behavior::rand01() < CLIMB_DROP_PER_FRAME) {
                *climb = ClimbState::None;
                *lastClimbEndAt = Some(std::time::Instant::now());
                physics.vy = 0.0;
                physics.vx = -(side as f32) * cfg.walkSpeed;
                return false;
            }
            *climb = ClimbState::Active { side, climbedPx: climbed };
            true
        }
        ClimbState::None => {
            physicsStep(physics, cfg, bounds, dragging, dt);
            let cooldownReady = lastClimbEndAt
                .map(|t| t.elapsed().as_secs_f32() > CLIMB_COOLDOWN_SEC)
                .unwrap_or(true);
            if walking && physics.grounded && !dragging && cooldownReady && allowClimb {
                let atLeft = physics.x <= bounds.minX + 0.5;
                let atRight = physics.x >= bounds.maxX - cfg.windowW - 0.5;
                if (atLeft || atRight) && crate::behavior::rand01() < CLIMB_TRIGGER_CHANCE {
                    let side = if atLeft { -1 } else { 1 };
                    *climb = ClimbState::Active { side, climbedPx: 0.0 };
                    physics.facing = side;
                    return true;
                }
            }
            false
        }
    }
}

fn feedBodyParams(player: &mut AnimPlayer, vxUnit: f32, vyUnit: f32, grounded: bool, exercising: bool, climbing: bool) {
    player.setFloat("ForwardSpeed", vxUnit);
    player.setFloat("UpSpeed", vyUnit);
    player.setBool("grounded", grounded);
    player.setFloat("CrouchAmount", 0.0);
    player.setBool("exercising", exercising);
    player.setBool("climbing", climbing);
    player.setInt("wallSide", 0);
    player.setFloat("wallSideFloat", 0.0);
    player.setFloat("workoutSpeed", 1.0);
}

fn feedArmsParams(player: &mut AnimPlayer, vxUnit: f32, vyUnit: f32, grounded: bool, exercising: bool, climbing: bool) {
    player.setFloat("ForwardSpeed", vxUnit);
    player.setFloat("UpSpeed", vyUnit);
    player.setBool("grounded", grounded);
    player.setFloat("CrouchAmount", 0.0);
    player.setBool("gun", false);
    player.setFloat("gunangle", 0.0);
    player.setBool("climbing", climbing);
    player.setBool("exercising", exercising);
    player.setFloat("workoutSpeed", 1.0);
}

fn applyPickedMotion(player: &mut AnimPlayer, picked: &Option<(String, std::time::Instant)>, isArms: bool) {
    let Some((name, _)) = picked else { return };
    let Some((bodyState, armsState)) = motionToState(name) else { return };
    let stateName = if isArms { armsState } else { bodyState };
    if player.currentStateName(0) == Some(stateName) {
        return;
    }
    player.playState(0, stateName);
}

/// 动作 id → (body state, arms state)。两个肢体播放器各取对应 state。
fn motionToState(motion: &str) -> Option<(&'static str, &'static str)> {
    match motion {
        "pushup" => Some(("ExperimentPushups", "ArmsPushups")),
        "squat" => Some(("ExperimentSquats", "ArmsSquats")),
        "plank" => Some(("ExperimentPlank", "ArmsPlank")),
        // 行走 / 奔跑共用奔跑动画。
        "walk" | "run" => Some(("ExperimentRun", "ArmsRun")),
        // 招手：身体保持待机，手臂挥动。
        "paw" => Some(("ExperimentIdle", "ArmsSwing")),
        // 待机：回到待机姿态。
        "idle" => Some(("ExperimentIdle", "ArmsIdle")),
        _ => None,
    }
}


/// 选择眼睛 sprite：非中性表情走 mood 映射；中性时按鼠标相对朝向在前视/回头两档切换（对标 FacialExpression.doBackEye）。
fn pickEyeSprite(mood: Mood, cursorClientX: f32, headX: f32, isRight: bool) -> &'static str {
    if mood != Mood::Neutral {
        return moodToEyeSprite(mood);
    }
    let lookBack = if isRight { cursorClientX < headX } else { cursorClientX > headX };
    if lookBack {
        "experimentEyeLookBack"
    } else {
        "experimentEyeOpen"
    }
}

/// 检测当前前台窗口是否全屏（最大化或覆盖整个显示器）。
/// 用于用户游戏 / 全屏办公时自动隐藏桌宠。
fn isForegroundFullscreen() -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetShellWindow, GetWindowPlacement, GetWindowRect,
        SW_MAXIMIZE, WINDOWPLACEMENT,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }

        // 排除桌面 Shell 窗口：用户在桌面时 GetForegroundWindow() 可能返回
        // Progman / WorkerW 等桌面窗口，它们覆盖整个显示器但并非用户全屏应用。
        let shell = GetShellWindow();
        if !shell.is_null() && hwnd == shell {
            return false;
        }

        // 1) 检查是否最大化
        let mut placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..std::mem::zeroed()
        };
        if GetWindowPlacement(hwnd, &mut placement) != 0 {
            if placement.showCmd == SW_MAXIMIZE as u32 {
                return true;
            }
        }

        // 2) 检查是否覆盖整个显示器（无边框全屏 / 游戏）
        let mut windowRect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(hwnd, &mut windowRect) != 0 {
            let monitor: HMONITOR = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut monitorInfo = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..std::mem::zeroed()
            };
            if GetMonitorInfoW(monitor, &mut monitorInfo) != 0 {
                let mr = monitorInfo.rcMonitor;
                if windowRect.left <= mr.left
                    && windowRect.top <= mr.top
                    && windowRect.right >= mr.right
                    && windowRect.bottom >= mr.bottom
                {
                    return true;
                }
            }
        }
    }
    false
}
