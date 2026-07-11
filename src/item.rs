#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Food,
    Backpack,
    Accessory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub kind: ItemKind,
    pub id: String,
}

impl Item {
    pub fn food(id: &str) -> Self {
        Self { kind: ItemKind::Food, id: id.to_string() }
    }
    pub fn backpack(id: &str) -> Self {
        Self { kind: ItemKind::Backpack, id: id.to_string() }
    }
    pub fn accessory(id: &str) -> Self {
        Self { kind: ItemKind::Accessory, id: id.to_string() }
    }
}

/// 背包 id → 子仓库格数。非背包 id 返回 None。
pub fn backpackCapacity(id: &str) -> Option<usize> {
    match id {
        "bigpack" => Some(60),
        "duffelbag" => Some(64),
        "legpouch" => Some(32),
        "smallpack" => Some(30),
        "slingbag" => Some(24),
        "fannypack" => Some(12),
        _ => None,
    }
}

/// 全部背包 id（固定顺序，用于默认拥有全部背包）。
pub fn allBackpackIds() -> &'static [&'static str] {
    &["bigpack", "duffelbag", "legpouch", "smallpack", "slingbag", "fannypack"]
}
