// 多桌宠状态共享：desktopPet/state/<petId>.json 周期写入 + 扫描其它在场桌宠。
#![allow(non_snake_case)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const STATE_DIR_NAME: &str = "state";
const ENTRY_TTL_MS: u64 = 5_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PetState {
    pub petId: String,
    pub pid: u32,
    pub screenX: f32,
    pub screenY: f32,
    pub facing: i32,
    pub behavior: String,
    pub timestampMs: u64,
}

pub fn stateDir(petRoot: &Path) -> PathBuf {
    petRoot.join(STATE_DIR_NAME)
}

pub fn writeOwnState(petRoot: &Path, state: &PetState) -> std::io::Result<()> {
    let dir = stateDir(petRoot);
    std::fs::create_dir_all(&dir)?;
    let p = dir.join(format!("{}.json", sanitize(&state.petId)));
    let json = serde_json::to_string(state).unwrap_or_default();
    std::fs::write(p, json)
}

pub fn readNeighbors(petRoot: &Path, selfPetId: &str) -> Vec<PetState> {
    let dir = stateDir(petRoot);
    let entries = match std::fs::read_dir(&dir) {
        Ok(it) => it,
        Err(_) => return Vec::new(),
    };
    let now = nowMillis();
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(s) = serde_json::from_str::<PetState>(&text) {
            if s.petId == selfPetId {
                continue;
            }
            if now.saturating_sub(s.timestampMs) > ENTRY_TTL_MS {
                continue;
            }
            out.push(s);
        }
    }
    out
}

pub fn removeOwnState(petRoot: &Path, petId: &str) {
    let p = stateDir(petRoot).join(format!("{}.json", sanitize(petId)));
    let _ = std::fs::remove_file(p);
}

pub fn nowMillis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sanitize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        }
    }
    if out.is_empty() {
        out.push_str("default");
    }
    out
}
