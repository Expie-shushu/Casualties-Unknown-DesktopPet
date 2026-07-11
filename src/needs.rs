// 三系统数据模型：心情 / 饥饿 / 口渴。0~100，越高越好，初始 80。纯逻辑 + 单元测试。
#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

/// 低值阈值（供表情 / 气泡 / 状态条共用）。
pub const LOW: f32 = 30.0;
/// 极低（危急）阈值。
pub const CRITICAL: f32 = 15.0;

/// 心情向饥渴充足时回升的基线（不超过该值）。
const MOOD_BASELINE: f32 = 60.0;
/// 饥渴过低时心情下降速率（每秒）。
const MOOD_DROP: f32 = 0.03;
/// 饥渴充足时心情向基线回升速率（每秒）。
const MOOD_RECOVER: f32 = 0.01;
/// 互动时心情提升速率（每秒）。
const MOOD_PLAY: f32 = 0.05;
/// 听音乐时心情提升速率（每秒）。
const MOOD_MUSIC: f32 = 0.002;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Needs {
    pub mood: f32,
    pub hunger: f32,
    pub thirst: f32,
}

impl Default for Needs {
    fn default() -> Self {
        Self {
            mood: 80.0,
            hunger: 80.0,
            thirst: 80.0,
        }
    }
}

impl Needs {
    /// 每帧推进：饥渴线性衰减 → 心情按饥渴状态 / 互动驱动 → 三值 clamp 0..=100。
    pub fn tick(&mut self, dtSec: f32, decayPerSec: f32, interaction: bool, musicPlaying: bool) {
        self.hunger -= decayPerSec * dtSec;
        self.thirst -= decayPerSec * dtSec;

        // 心情衰减：饥饿 / 口渴各有一个 <40 即触发，下降速率按低值个数加倍。
        // 一个 <40 → 2×MOOD_DROP；两个都 <40 → 4×MOOD_DROP。
        let lowCount = (self.hunger < 40.0) as u8 + (self.thirst < 40.0) as u8;
        if lowCount >= 1 {
            let mul = if lowCount >= 2 { 4.0 } else { 2.0 };
            self.mood -= MOOD_DROP * mul * dtSec;
        } else if self.hunger > 60.0 && self.thirst > 60.0 {
            // 饥渴充足：心情向基线 60 靠拢，但不超过基线。
            if self.mood < MOOD_BASELINE {
                self.mood = (self.mood + MOOD_RECOVER * dtSec).min(MOOD_BASELINE);
            }
        }
        // 用户互动，心情提升速率为 0.05/s
        if interaction {
            self.mood += MOOD_PLAY * dtSec;
        }
        // 听音乐互动，心情提升速率为 0.002/s
        if musicPlaying {
            self.mood += MOOD_MUSIC * dtSec;
        }

        self.clampAll();
    }

    /// 进食：三值各加对应量并 clamp。
    pub fn feed(&mut self, hunger: f32, thirst: f32, mood: f32) {
        self.hunger += hunger;
        self.thirst += thirst;
        self.mood += mood;
        self.clampAll();
    }

    /// 三值 clamp 到 0..=100。
    pub fn clampAll(&mut self) {
        self.mood = self.mood.clamp(0.0, 100.0);
        self.hunger = self.hunger.clamp(0.0, 100.0);
        self.thirst = self.thirst.clamp(0.0, 100.0);
    }
}
