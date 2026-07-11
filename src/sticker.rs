// 表情包系统：用户往 desktopPet/stickers/{moodHigh,moodLow,hungerHigh,hungerLow,
// thirstHigh,thirstLow,rpsGameWin,rpsGameDraw,rpsGameLose}/ 放 PNG/JPG/GIF，游戏在对应时机弹出表情。
// 显示 3.6s，大小约 180×180 逻辑像素，不遮挡气泡。
#![allow(non_snake_case)]

use std::path::Path;

use image::AnimationDecoder;

use crate::asset::{SpriteAsset, SpriteFactory};
use crate::renderer::spritePipeline::buildSpriteMatrix;
use crate::renderer::SpriteDraw;

/// 表情包分类：6 种需求状态 + 3 种游戏结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickerCategory {
    MoodHigh,
    MoodLow,
    HungerHigh,
    HungerLow,
    ThirstHigh,
    ThirstLow,
    Win,
    Draw,
    Lose,
}

impl StickerCategory {
    fn dirName(self) -> &'static str {
        match self {
            Self::MoodHigh => "moodHigh",
            Self::MoodLow => "moodLow",
            Self::HungerHigh => "hungerHigh",
            Self::HungerLow => "hungerLow",
            Self::ThirstHigh => "thirstHigh",
            Self::ThirstLow => "thirstLow",
            Self::Win => "rpsGameWin",
            Self::Draw => "rpsGameDraw",
            Self::Lose => "rpsGameLose",
        }
    }
}

/// 表情包单帧：已上传的 sprite + 该帧显示时长（ms）。
pub struct StickerFrame {
    pub sprite: SpriteAsset,
    pub delayMs: u64,
}

/// 一个表情包定义（可能含多帧，用于 GIF 动画循环）。
pub struct StickerDef {
    pub frames: Vec<StickerFrame>,
}

/// 当前正在显示的表情包实例。
pub struct ActiveSticker {
    pub defIdx: usize,
    pub category: StickerCategory,
    /// 出现在宠物左侧(-1)还是右侧(1)。
    pub side: i32,
    /// 已显示秒数。
    pub elapsed: f32,
    /// 当前播放到第几帧。
    pub frameIdx: usize,
    /// 当前帧已显示秒数。
    pub frameElapsed: f32,
    /// 显示总时长（秒）。
    pub duration: f32,
}

/// 需求表情包阈值：与 chatter.rs 中 NeedsCue 判定一致。
const MOOD_HIGH: f32 = 80.0;
const MOOD_LOW: f32 = 60.0;
const FOOD_HIGH: f32 = 90.0;
const FOOD_LOW: f32 = 60.0;

/// 单个表情包显示时长（秒）。
const STICKER_DURATION: f32 = 3.6;

/// 根据当前需求值，返回所有满足条件的分类列表。
fn eligibleNeedsCategories(mood: f32, hunger: f32, thirst: f32) -> Vec<StickerCategory> {
    let mut cats = Vec::with_capacity(6);
    if mood > MOOD_HIGH {
        cats.push(StickerCategory::MoodHigh);
    }
    if mood < MOOD_LOW {
        cats.push(StickerCategory::MoodLow);
    }
    if hunger > FOOD_HIGH {
        cats.push(StickerCategory::HungerHigh);
    }
    if hunger < FOOD_LOW {
        cats.push(StickerCategory::HungerLow);
    }
    if thirst > FOOD_HIGH {
        cats.push(StickerCategory::ThirstHigh);
    }
    if thirst < FOOD_LOW {
        cats.push(StickerCategory::ThirstLow);
    }
    cats
}

/// 表情包管理器：加载、触发、计时、渲染。
pub struct StickerManager {
    pools: Vec<(StickerCategory, Vec<StickerDef>)>,
    pub active: Option<ActiveSticker>,
    /// 距下一次需求表情包随机弹出的剩余秒数。
    pub idleTimer: f32,
}

/// 九种分类的创建顺序（与 load 中遍历一致）。
const ALL_CATS: [StickerCategory; 9] = [
    StickerCategory::MoodHigh,
    StickerCategory::MoodLow,
    StickerCategory::HungerHigh,
    StickerCategory::HungerLow,
    StickerCategory::ThirstHigh,
    StickerCategory::ThirstLow,
    StickerCategory::Win,
    StickerCategory::Draw,
    StickerCategory::Lose,
];

impl StickerManager {
    pub fn new() -> Self {
        Self {
            pools: ALL_CATS.iter().map(|&c| (c, Vec::new())).collect(),
            active: None,
            idleTimer: 30.0,
        }
    }

    /// 重载全部表情包（首次加载）。
    pub fn loadAndReplace(&mut self, stickersDir: &Path, factory: &SpriteFactory) {
        *self = Self::load(stickersDir, factory);
    }

    fn load(stickersDir: &Path, factory: &SpriteFactory) -> Self {
        let mut m = Self::new();
        for (cat, list) in &mut m.pools {
            let dir = stickersDir.join(cat.dirName());
            if !dir.exists() {
                continue;
            }
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif") {
                    continue;
                }
                let bytes = match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!("sticker read {}: {e:?}", path.display());
                        continue;
                    }
                };
                if ext == "gif" {
                    if let Some(def) = Self::loadGif(&bytes, factory, &path) {
                        list.push(def);
                    }
                } else if let Some(def) = Self::loadStatic(&bytes, factory, &path) {
                    list.push(def);
                }
            }
            log::info!(
                "stickers: loaded {} in {} ({})",
                list.len(),
                cat.dirName(),
                dir.display()
            );
        }
        m
    }

    fn loadStatic(bytes: &[u8], factory: &SpriteFactory, path: &Path) -> Option<StickerDef> {
        let img = match image::load_from_memory(bytes) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                log::warn!("sticker decode {}: {e:?}", path.display());
                return None;
            }
        };
        let (w, h) = img.dimensions();
        let label = path.file_stem().and_then(|s| s.to_str()).unwrap_or("sticker");
        let sprite = match factory.fromRgba(&img.into_raw(), w, h, label) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("sticker upload {}: {e:?}", path.display());
                return None;
            }
        };
        Some(StickerDef {
            frames: vec![StickerFrame { sprite, delayMs: 0 }],
        })
    }

    fn loadGif(bytes: &[u8], factory: &SpriteFactory, path: &Path) -> Option<StickerDef> {
        use image::codecs::gif::GifDecoder;
        let decoder = match GifDecoder::new(std::io::Cursor::new(bytes)) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("sticker gif decode {}: {e:?}", path.display());
                return None;
            }
        };
        let frames = match decoder.into_frames().collect_frames() {
            Ok(f) => f,
            Err(e) => {
                log::warn!("sticker gif frames {}: {e:?}", path.display());
                return None;
            }
        };
        if frames.is_empty() {
            return None;
        }
        let label = path.file_stem().and_then(|s| s.to_str()).unwrap_or("sticker");
        let mut out: Vec<StickerFrame> = Vec::with_capacity(frames.len());
        for (i, frame) in frames.into_iter().enumerate() {
            let (num, denom) = frame.delay().numer_denom_ms();
            let delayMs: u64 = if denom > 0 { (num / denom) as u64 } else { 100 };
            let rgba = frame.into_buffer();
            let (w, h) = rgba.dimensions();
            let sprite = match factory.fromRgba(&rgba.into_raw(), w, h, &format!("{label}_f{i}")) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("sticker gif frame {i} upload {}: {e:?}", path.display());
                    continue;
                }
            };
            out.push(StickerFrame { sprite, delayMs });
        }
        if out.is_empty() {
            None
        } else {
            Some(StickerDef { frames: out })
        }
    }

    /// 触发指定分类的表情包（从池中随机选一个）。
    pub fn trigger(&mut self, category: StickerCategory, rand01: f32, rand01b: f32) {
        let pool = self.poolFor(category);
        if pool.is_empty() {
            return;
        }
        let idx = ((rand01 * pool.len() as f32) as usize).min(pool.len() - 1);
        let side = if rand01b < 0.5 { -1 } else { 1 };
        self.active = Some(ActiveSticker {
            defIdx: idx,
            category,
            side,
            elapsed: 0.0,
            frameIdx: 0,
            frameElapsed: 0.0,
            duration: STICKER_DURATION,
        });
    }

    /// 每帧推进：活跃表情计时 + 需求表情包随机弹出。
    /// `needs` 提供当前心情/饥饿/口渴值，`cfg` 提供用户设置的时间区间。
    pub fn tick(&mut self, dt: f32, needs: &crate::needs::Needs, cfg: &crate::settings::StickerConfig) {
        // ── 推进活跃表情包计时 ──
        // 先查池信息（不可变借用），再做帧推进/过期（可变借用）。
        let frameInfo: Option<(u64, usize)> = self.active.as_ref().and_then(|a| {
            let pool = self.poolFor(a.category);
            if a.defIdx < pool.len() && pool[a.defIdx].frames.len() > 1 {
                let delay = pool[a.defIdx].frames[a.frameIdx].delayMs.max(1);
                Some((delay, pool[a.defIdx].frames.len()))
            } else {
                None
            }
        });
        if let Some(a) = self.active.as_mut() {
            a.elapsed += dt;
            a.frameElapsed += dt;
            if let Some((delayMs, frameCount)) = frameInfo {
                let curDelay = delayMs as f32 / 1000.0;
                if a.frameElapsed >= curDelay {
                    a.frameElapsed -= curDelay;
                    a.frameIdx = (a.frameIdx + 1) % frameCount;
                }
            }
            if a.elapsed >= a.duration {
                self.active = None;
            }
        }

        // ── 需求表情包随机计时器 ──
        if !cfg.enabled {
            return;
        }
        if self.active.is_some() {
            return; // 有活跃表情时不抢
        }
        self.idleTimer -= dt;
        if self.idleTimer <= 0.0 {
            let min = cfg.minIntervalSec.max(5.0);
            let max = cfg.maxIntervalSec.max(min);
            self.idleTimer = min + crate::behavior::rand01() * (max - min);
            // 收集当前满足条件的分类，随机选一个非空池触发。
            let eligible = eligibleNeedsCategories(needs.mood, needs.hunger, needs.thirst);
            if !eligible.is_empty() {
                let start = (crate::behavior::rand01() * eligible.len() as f32) as usize;
                for offset in 0..eligible.len() {
                    let cat = eligible[(start + offset) % eligible.len()];
                    if !self.poolFor(cat).is_empty() {
                        let r1 = crate::behavior::rand01();
                        let r2 = crate::behavior::rand01();
                        self.trigger(cat, r1, r2);
                        break;
                    }
                }
            }
        }
    }

    /// 生成活跃表情包的 SpriteDraw 列表。
    /// `stickerW`/`stickerH` — 用户设定的表情包最大宽高（像素），等比缩放适配。
    pub fn buildDraws(
        &self,
        screenW: f32,
        screenH: f32,
        petCenterX: f32,
        petCenterY: f32,
        stickerW: f32,
        stickerH: f32,
    ) -> Vec<SpriteDraw<'_>> {
        let a = match self.active.as_ref() {
            Some(a) => a,
            None => return Vec::new(),
        };
        let pool = self.poolFor(a.category);
        let def = match pool.get(a.defIdx) {
            Some(d) => d,
            None => return Vec::new(),
        };
        let frame = &def.frames[a.frameIdx];
        let (iw, ih) = (frame.sprite.width as f32, frame.sprite.height as f32);
        let fit = (stickerW / iw).min(stickerH / ih);
        let dw = iw * fit;
        let dh = ih * fit;
        let cx = petCenterX + a.side as f32 * 110.0;
        let cy = petCenterY - 20.0;
        let m = buildSpriteMatrix(screenW, screenH, cx, cy, dw, dh, 0.0, 1.0);
        vec![SpriteDraw::full(&frame.sprite, m)]
    }

    fn poolFor(&self, cat: StickerCategory) -> &Vec<StickerDef> {
        for (c, list) in &self.pools {
            if *c == cat {
                return list;
            }
        }
        // 不会发生：所有分类已预创建。
        static EMPTY: Vec<StickerDef> = Vec::new();
        &EMPTY
    }
}
