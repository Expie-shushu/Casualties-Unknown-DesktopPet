// 资源路径解析。优先 --config 参数 / SKIN_EDITOR_APP_ROOT 环境变量 / 当前 exe 同目录。
#![allow(non_snake_case)]

use std::path::{Path, PathBuf};

pub const APP_ROOT_ENV: &str = "SKIN_EDITOR_APP_ROOT";

pub fn appRoot(override_: Option<&Path>) -> PathBuf {
    if let Some(p) = override_ {
        return p.to_path_buf();
    }
    if let Some(v) = std::env::var_os(APP_ROOT_ENV) {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn dataDir(root: &Path) -> PathBuf {
    root.join("data")
}

pub fn skinDir(root: &Path, name: &str) -> PathBuf {
    dataDir(root).join("skin").join(name)
}

/// 扫描 data/skin/ 下的所有子目录，返回皮肤名列表。
pub fn listSkins(root: &Path) -> Vec<String> {
    let dir = dataDir(root).join("skin");
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = e.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

pub fn desktopPetRoot(root: &Path) -> PathBuf {
    root.join("desktopPet")
}

pub fn posesDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("poses")
}

pub fn motionsDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("motions")
}

pub fn interactionsDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("interactions")
}

pub fn pluginsDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("plugins")
}

pub fn configsDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("configs")
}

pub fn foodsDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("foods")
}

pub fn inventoryDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("inventory")
}

/// 仓库圆环槽位背景图目录（用户把 hands/mouth/uptorso 等 png 丢这里）。
pub fn slotBgDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("slotbg")
}

/// 石头剪刀布按钮图目录（用户把 rock/scissors/paper.png 丢这里）。
pub fn rpsDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("rps")
}

/// 表情包目录（用户把 PNG/JPG/GIF 表情丢进 idle/ win/ draw/ lose/ 子目录）。
pub fn stickersDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("stickers")
}

/// 音乐文件目录（用户把 MP3/WAV/FLAC/OGG 等丢这里）。
pub fn musicDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("music")
}

/// 抽奖转盘图片目录（用户把 coin/exit/btn_locked 等 png 丢这里）。
pub fn wheelDir(root: &Path) -> PathBuf {
    desktopPetRoot(root).join("wheel")
}
