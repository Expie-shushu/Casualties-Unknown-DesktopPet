// 基线姿态加载：desktopPet/poses/<name>.json 解析 + 类型定义。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimbPose {
    pub name: String,
    pub limbNum: i32,
    pub px: f32,
    pub py: f32,
    pub rotZ: f32,
    pub scaleX: f32,
    pub scaleY: f32,
    pub sortingOrder: i32,
    pub spriteName: String,
    pub pivotU: f32,
    pub pivotV: f32,
    pub spriteW: f32,
    pub spriteH: f32,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetPose {
    pub version: u32,
    #[serde(default)]
    pub isRight: bool,
    pub limbs: Vec<LimbPose>,
}

pub fn parsePose(text: &str) -> Result<PetPose> {
    serde_json::from_str::<PetPose>(text).context("parse pose json")
}

pub fn loadPose(root: &Path, name: &str) -> Result<PetPose> {
    let p = crate::paths::posesDir(root).join(format!("{name}.json"));
    let text = std::fs::read_to_string(&p).with_context(|| format!("read pose {}", p.display()))?;
    parsePose(&text)
}

pub fn poseByName(pose: &PetPose) -> HashMap<String, LimbPose> {
    let mut map = HashMap::with_capacity(pose.limbs.len());
    for limb in &pose.limbs {
        map.insert(limb.name.clone(), limb.clone());
    }
    map
}
