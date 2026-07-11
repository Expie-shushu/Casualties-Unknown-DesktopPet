// 桌宠骨骼父子关系 + 与 SkinSync 同款 sided sprite 选择规则。
#![allow(non_snake_case)]

use std::collections::HashMap;

use crate::asset::SpriteAsset;

pub fn parentOf(name: &str) -> Option<&'static str> {
    match name {
        "DownTorso" => None,
        "UpTorso" => Some("DownTorso"),
        "Head" => Some("UpTorso"),
        "UpArmF" | "UpArmB" => Some("UpTorso"),
        "DownArmF" => Some("UpArmF"),
        "HandF" => Some("DownArmF"),
        "DownArmB" => Some("UpArmB"),
        "HandB" => Some("DownArmB"),
        "ThighF" | "ThighB" => Some("DownTorso"),
        "CrusF" => Some("ThighF"),
        "FootF" => Some("CrusF"),
        "CrusB" => Some("ThighB"),
        "FootB" => Some("CrusB"),
        "Tail" => Some("DownTorso"),
        "wingUL" | "wingUR" => Some("UpTorso"),
        "wingDL" => Some("wingUL"),
        "wingDR" => Some("wingUR"),
        _ => None,
    }
}

pub fn limbSide(limbName: &str) -> Option<char> {
    let last = limbName.chars().last()?;
    if last == 'F' || last == 'B' {
        Some(last)
    } else {
        None
    }
}

pub fn resolveSidedSprite<'a>(
    sprites: &'a HashMap<String, SpriteAsset>,
    baseName: &str,
    side: Option<char>,
    isRight: bool,
) -> Option<&'a SpriteAsset> {
    if isRight {
        if let Some(s) = side {
            if let Some(a) = sprites.get(&format!("R_{}{}", baseName, s)) {
                return Some(a);
            }
        }
        if let Some(a) = sprites.get(&format!("R_{}", baseName)) {
            return Some(a);
        }
    }
    if let Some(s) = side {
        if let Some(a) = sprites.get(&format!("{}{}", baseName, s)) {
            return Some(a);
        }
    }
    if let Some(a) = sprites.get(baseName) {
        return Some(a);
    }
    if let Some(s) = side {
        let opp = if s == 'F' { 'B' } else { 'F' };
        if isRight {
            if let Some(a) = sprites.get(&format!("R_{}{}", baseName, opp)) {
                return Some(a);
            }
        }
        if let Some(a) = sprites.get(&format!("{}{}", baseName, opp)) {
            return Some(a);
        }
    }
    None
}
