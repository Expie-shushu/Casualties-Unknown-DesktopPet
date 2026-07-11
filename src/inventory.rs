// 方格式仓库：6 格圆环主仓库（不可叠放，一格一物）+ 背包子仓库扩容。纯数据逻辑，无 UI 依赖。
#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

use crate::item::{backpackCapacity, Item};

/// 主仓库槽位数：0=主手 1=副手 2=上背部 3=下背部 4=中背部 5=口腔。
pub const MAIN_SLOTS: usize = 6;
/// 口腔槽索引（含物触发口齿不清）。
pub const MOUTH_SLOT: usize = 5;

/// 主槽中文标签。
pub fn mainSlotLabel(idx: usize) -> &'static str {
    match idx {
        0 => "主手",
        1 => "副手",
        2 => "上背部",
        3 => "下背部",
        4 => "中背部",
        5 => "口腔",
        _ => "?",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Backpack {
    pub id: String,
    pub equipped: bool,
    pub slots: Vec<Option<Item>>,
    /// 获得时刻的 Unix 毫秒时间戳；用于 24h 时限过期。None=无时限（旧存档/特殊背包）。
    pub acquiredAtMs: Option<u64>,
}

impl Default for Backpack {
    fn default() -> Self {
        Self { id: String::new(), equipped: false, slots: Vec::new(), acquiredAtMs: None }
    }
}

/// 背包时限：获得后 24 小时（真实时间）过期消失（连同内容物）。
pub const BACKPACK_TTL_MS: u64 = 24 * 60 * 60 * 1000;

impl Backpack {
    fn new(id: &str) -> Self {
        let cap = backpackCapacity(id).unwrap_or(0);
        Self { id: id.to_string(), equipped: false, slots: vec![None; cap], acquiredAtMs: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Inventory {
    pub mainSlots: Vec<Option<Item>>,
    pub backpacks: Vec<Backpack>,
}

impl Default for Inventory {
    fn default() -> Self {
        // 初始：主仓库全空；不默认拥有任何背包（背包只能玩游戏赢取，且有 24h 时限）。
        Self { mainSlots: vec![None; MAIN_SLOTS], backpacks: Vec::new() }
    }
}

impl Inventory {
    /// serde 反序列化后修复（旧/损坏 JSON 防御）：mainSlots 补/截到 6；
    /// 清除无获得时间戳的背包（旧版「默认全套」遗留——新机制下所有背包都靠赢取且带时间戳）；
    /// 修复背包子仓库长度（容量表可能变更）。
    pub fn normalize(&mut self) {
        if self.mainSlots.len() != MAIN_SLOTS {
            self.mainSlots.resize(MAIN_SLOTS, None);
        }
        // 清除无时间戳的历史遗留背包（旧版默认全套）。新赢取的背包均带 acquiredAtMs。
        self.backpacks.retain(|bp| bp.acquiredAtMs.is_some());
        // 修复背包子仓库长度（容量表可能变更）。
        for bp in &mut self.backpacks {
            if let Some(cap) = backpackCapacity(&bp.id) {
                if bp.slots.len() != cap {
                    bp.slots.resize(cap, None);
                }
            }
        }
    }

    /// 口腔槽是否含物（驱动口齿不清气泡）。
    pub fn mouthOccupied(&self) -> bool {
        self.mainSlots.get(MOUTH_SLOT).map_or(false, |s| s.is_some())
    }

    /// 取出主槽物品并置空。
    pub fn mainTake(&mut self, idx: usize) -> Option<Item> {
        self.mainSlots.get_mut(idx).and_then(|s| s.take())
    }

    /// 放入主槽，返回被替换出的旧物品（一格一物）。越界返回传入物品本身。
    pub fn mainPlace(&mut self, idx: usize, item: Item) -> Option<Item> {
        match self.mainSlots.get_mut(idx) {
            Some(s) => s.replace(item),
            None => Some(item),
        }
    }

    /// 首个空主槽索引。
    pub fn firstEmptyMain(&self) -> Option<usize> {
        self.mainSlots.iter().position(|s| s.is_none())
    }

    /// 获得一个背包：未拥有且 id 合法则按容量新建条目并 push，记录获得时刻（24h 时限）。
    /// `nowMs`=当前 Unix 毫秒时间戳。返回是否新增。
    pub fn addBackpack(&mut self, id: &str, nowMs: u64) -> bool {
        if self.backpackById(id).is_some() {
            return false; // 已拥有（同款唯一）。
        }
        match backpackCapacity(id) {
            Some(_) => {
                let mut bp = Backpack::new(id); // 复用私有构造器，按容量建空格。
                bp.acquiredAtMs = Some(nowMs);  // 起算 24h 时限。
                self.backpacks.push(bp);
                true
            }
            None => false, // 未知背包 id。
        }
    }

    /// 移除所有已过 24h 时限的背包（连同内容物）。`nowMs`=当前 Unix 毫秒。
    /// 返回被移除的背包 id 列表（供气泡提示用）。acquiredAtMs=None 的背包永不过期。
    pub fn expireBackpacks(&mut self, nowMs: u64) -> Vec<String> {
        let mut removed = Vec::new();
        self.backpacks.retain(|bp| {
            let expired = bp
                .acquiredAtMs
                .map(|t| nowMs.saturating_sub(t) >= BACKPACK_TTL_MS)
                .unwrap_or(false);
            if expired {
                removed.push(bp.id.clone());
            }
            !expired
        });
        removed
    }

    /// 装备背包（同款唯一：本就一种一个，置 equipped=true）。成功返回 true。
    pub fn equipBackpack(&mut self, id: &str) -> bool {
        if let Some(bp) = self.backpackByIdMut(id) {
            bp.equipped = true;
            true
        } else {
            false
        }
    }

    /// 卸下背包。
    pub fn unequipBackpack(&mut self, id: &str) -> bool {
        if let Some(bp) = self.backpackByIdMut(id) {
            bp.equipped = false;
            true
        } else {
            false
        }
    }

    pub fn backpackById(&self, id: &str) -> Option<&Backpack> {
        self.backpacks.iter().find(|b| b.id == id)
    }

    pub fn backpackByIdMut(&mut self, id: &str) -> Option<&mut Backpack> {
        self.backpacks.iter_mut().find(|b| b.id == id)
    }

    /// 子仓库取物并置空。
    pub fn bpTake(&mut self, packId: &str, slotIdx: usize) -> Option<Item> {
        self.backpackByIdMut(packId)
            .and_then(|bp| bp.slots.get_mut(slotIdx))
            .and_then(|s| s.take())
    }

    /// 子仓库放物，返回被替换出的旧物。越界/无此背包返回传入物品本身。
    pub fn bpPlace(&mut self, packId: &str, slotIdx: usize, item: Item) -> Option<Item> {
        match self.backpackByIdMut(packId).and_then(|bp| bp.slots.get_mut(slotIdx)) {
            Some(s) => s.replace(item),
            None => Some(item),
        }
    }

    /// 子仓库首个空格。
    pub fn firstEmptyInPack(&self, packId: &str) -> Option<usize> {
        self.backpackById(packId)
            .and_then(|bp| bp.slots.iter().position(|s| s.is_none()))
    }
}
