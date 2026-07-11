// 随机闲聊气泡台词加载。用户可编辑 desktopPet/configs/chatter.json 增删台词。
// 出现频率不在此处，而在设置里（NeedsConfig.chatterMin/MaxSec），可在设置面板调节。
#![allow(non_snake_case)]

use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ChatterConfig {
    pub version: u32,
    /// 闲聊台词池；随机抽一条显示。
    pub lines: Vec<String>,
    /// 吃「食品」时的台词池；随机抽一条显示。
    pub eatLines: Vec<String>,
    /// 喝「饮品」时的台词池；随机抽一条显示。
    pub drinkLines: Vec<String>,
    /// 打开石头剪刀布游戏时的开场台词池；随机抽一条显示。
    pub gameStartLines: Vec<String>,
    /// 打开抽奖转盘时的开场台词池；随机抽一条显示。
    pub wheelStartLines: Vec<String>,
    /// 石头剪刀布获胜时的台词池；随机抽一条显示。
    pub gameWinLines: Vec<String>,
    /// 石头剪刀布平局时的台词池；随机抽一条显示。
    pub gameDrawLines: Vec<String>,
    /// 石头剪刀布失败时的台词池；随机抽一条显示。
    pub gameLoseLines: Vec<String>,
    /// 双击桌宠打招呼的台词池。
    pub greetingLines: Vec<String>,
    /// 仓库已满提示的台词池。
    pub inventoryFullLines: Vec<String>,
    /// 穿戴背包成功反馈的台词池。
    pub equipLines: Vec<String>,
    /// 开始听音乐时的台词池。
    pub musicOnLines: Vec<String>,
    /// 停止听音乐时的台词池。
    pub musicOffLines: Vec<String>,
    /// 需求值偏高 / 偏低时的台词池（心情 / 饥饿 / 口渴各两档）。
    pub needsLines: NeedsLines,
}

/// 六个需求台词池：每个需求维度的「偏高」「偏低」两档。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct NeedsLines {
    pub moodHigh: Vec<String>,
    pub moodLow: Vec<String>,
    pub hungerHigh: Vec<String>,
    pub hungerLow: Vec<String>,
    pub thirstHigh: Vec<String>,
    pub thirstLow: Vec<String>,
}

/// 需求台词的类别（维度 + 高/低），用于选词。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedsCue {
    MoodHigh,
    MoodLow,
    HungerHigh,
    HungerLow,
    ThirstHigh,
    ThirstLow,
}

impl Default for ChatterConfig {
    fn default() -> Self {
        // 不再内置任何台词：所有台词均来自 desktopPet/configs/chatter.json。
        // 缺文件 / 缺某分类时，对应气泡直接不显示（pick 返回 None），而非弹内置默认句。
        Self {
            version: 1,
            lines: Vec::new(),
            eatLines: Vec::new(),
            drinkLines: Vec::new(),
            gameStartLines: Vec::new(),
            wheelStartLines: Vec::new(),
            gameWinLines: Vec::new(),
            gameDrawLines: Vec::new(),
            gameLoseLines: Vec::new(),
            greetingLines: Vec::new(),
            inventoryFullLines: Vec::new(),
            equipLines: Vec::new(),
            musicOnLines: Vec::new(),
            musicOffLines: Vec::new(),
            needsLines: NeedsLines::default(),
        }
    }
}

impl ChatterConfig {
    pub fn isEmpty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 用 [0,1) 随机数选一条闲聊台词。空池返回 None。
    pub fn pick(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.lines, rand01)
    }

    /// 用 [0,1) 随机数选一条「吃食品」台词。空池返回 None。
    pub fn pickEat(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.eatLines, rand01)
    }

    /// 用 [0,1) 随机数选一条「喝饮品」台词。空池返回 None。
    pub fn pickDrink(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.drinkLines, rand01)
    }

    /// 用 [0,1) 随机数选一条游戏开场台词。空池返回 None。
    pub fn pickGameStart(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.gameStartLines, rand01)
    }

    /// 用 [0,1) 随机数选一条转盘开场台词。空池返回 None。
    pub fn pickWheelStart(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.wheelStartLines, rand01)
    }

    /// 用 [0,1) 随机数选一条游戏获胜台词。空池返回 None。
    pub fn pickGameWin(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.gameWinLines, rand01)
    }

    /// 用 [0,1) 随机数选一条游戏平局台词。空池返回 None。
    pub fn pickGameDraw(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.gameDrawLines, rand01)
    }

    /// 用 [0,1) 随机数选一条游戏失败台词。空池返回 None。
    pub fn pickGameLose(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.gameLoseLines, rand01)
    }

    /// 用 [0,1) 随机数选一条打招呼台词。空池返回 None。
    pub fn pickGreeting(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.greetingLines, rand01)
    }

    /// 用 [0,1) 随机数选一条「仓库已满」台词。空池返回 None。
    pub fn pickInventoryFull(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.inventoryFullLines, rand01)
    }

    /// 用 [0,1) 随机数选一条「穿戴背包」台词。空池返回 None。
    pub fn pickEquip(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.equipLines, rand01)
    }

    /// 用 [0,1) 随机数选一条「开始听音乐」台词。空池返回 None。
    pub fn pickMusicOn(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.musicOnLines, rand01)
    }

    /// 用 [0,1) 随机数选一条「停止听音乐」台词。空池返回 None。
    pub fn pickMusicOff(&self, rand01: f32) -> Option<&str> {
        pickFrom(&self.musicOffLines, rand01)
    }

    /// 用 [0,1) 随机数从对应需求台词池选一条。空池返回 None。
    pub fn pickNeeds(&self, cue: NeedsCue, rand01: f32) -> Option<&str> {
        let pool = match cue {
            NeedsCue::MoodHigh => &self.needsLines.moodHigh,
            NeedsCue::MoodLow => &self.needsLines.moodLow,
            NeedsCue::HungerHigh => &self.needsLines.hungerHigh,
            NeedsCue::HungerLow => &self.needsLines.hungerLow,
            NeedsCue::ThirstHigh => &self.needsLines.thirstHigh,
            NeedsCue::ThirstLow => &self.needsLines.thirstLow,
        };
        pickFrom(pool, rand01)
    }
}

fn pickFrom(pool: &[String], rand01: f32) -> Option<&str> {
    if pool.is_empty() {
        return None;
    }
    let idx = ((rand01 * pool.len() as f32) as usize).min(pool.len() - 1);
    Some(pool[idx].as_str())
}

/// 读 <configsDir>/chatter.json；缺失 / 解析失败回退内置默认台词。
pub fn loadChatter(configsDir: &Path) -> ChatterConfig {
    let p = configsDir.join("chatter.json");
    if !p.exists() {
        return ChatterConfig::default();
    }
    match std::fs::read_to_string(&p) {
        Ok(text) => serde_json::from_str::<ChatterConfig>(&text).unwrap_or_default(),
        Err(_) => ChatterConfig::default(),
    }
}
