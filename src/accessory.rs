// 配饰定义：解析 data/accessories.json，描述某配饰挂到哪个父肢体、局部偏移/旋转/排序。
// 渲染时按父肢体的 posed px/py/rotZ 做 FK 合成，叠加到桌宠身上（见 app.rs::appendAccessoryDraws）。
#![allow(non_snake_case)]

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// 单个配饰定义（对应 accessories.json 一条）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessoryDef {
    /// 配饰 id（= 背包/物品 id，如 "bigpack"）。
    pub id: String,
    /// 父肢体名（如 "UpTorso" / "Head" / "HandF"），渲染时在 posed limbs 里 find。
    pub limb: String,
    /// 贴图名（= scene.sprites 的键，通常与 id 同名）。
    pub sprite: String,
    /// 父肢体局部坐标系下的偏移（单位与 limb px/py 同）。
    #[serde(default)]
    pub offX: f32,
    #[serde(default)]
    pub offY: f32,
    /// 本地旋转（度），叠加在父肢体 rotZ 上。
    #[serde(default)]
    pub rot: f32,
    /// 相对父肢体的排序增量：accessory.sortingOrder = parentLimb.sortingOrder + z。
    #[serde(default)]
    pub z: i32,
    #[serde(default)]
    pub slot: String,
}

/// 加载 data/accessories.json。失败（缺文件/解析错）返回空表，不影响桌宠运行。
pub fn loadAccessoryDefs(root: &Path) -> Vec<AccessoryDef> {
    match tryLoad(root) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("load accessories.json failed: {e:?}");
            Vec::new()
        }
    }
}

fn tryLoad(root: &Path) -> Result<Vec<AccessoryDef>> {
    let p = crate::paths::dataDir(root).join("accessories.json");
    let text = std::fs::read_to_string(&p)
        .with_context(|| format!("read {}", p.display()))?;
    serde_json::from_str::<Vec<AccessoryDef>>(&text).context("parse accessories.json")
}

/// 按 id 查配饰定义。
pub fn accessoryById<'a>(defs: &'a [AccessoryDef], id: &str) -> Option<&'a AccessoryDef> {
    defs.iter().find(|a| a.id == id)
}
