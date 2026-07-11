// 多桌宠互动决策：邻居状态 + 距离 + cooldown，命中规则即触发 actions。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::bus::PetState;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InteractionAction {
    pub r#type: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub speed: Option<f32>,
    #[serde(default)]
    pub durationMs: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InteractionTrigger {
    #[serde(default)]
    pub distancePx: Option<f32>,
    #[serde(default)]
    pub selfState: Option<Vec<String>>,
    #[serde(default)]
    pub otherState: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InteractionRule {
    pub id: String,
    #[serde(default)]
    pub cooldownMs: u64,
    pub trigger: InteractionTrigger,
    pub actions: Vec<InteractionAction>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InteractionConfig {
    #[serde(default = "defaultVersion")]
    pub version: u32,
    #[serde(default)]
    pub rules: Vec<InteractionRule>,
}

fn defaultVersion() -> u32 {
    1
}

pub struct InteractionState {
    pub config: InteractionConfig,
    lastFire: HashMap<String, Instant>,
}

impl InteractionState {
    pub fn new(config: InteractionConfig) -> Self {
        Self {
            config,
            lastFire: HashMap::new(),
        }
    }
}

pub fn loadInteractions(petRoot: &Path, name: &str) -> InteractionConfig {
    let p = petRoot.join("interactions").join(format!("{name}.json"));
    if !p.exists() {
        return InteractionConfig::default();
    }
    match std::fs::read_to_string(&p) {
        Ok(text) => serde_json::from_str::<InteractionConfig>(&text).unwrap_or_default(),
        Err(_) => InteractionConfig::default(),
    }
}

pub struct TriggeredAction {
    pub kind: String,
    pub targetX: f32,
    pub targetY: f32,
    pub motion: Option<String>,
    pub text: Option<String>,
    pub speed: f32,
    pub durationMs: u64,
}

pub fn tick(
    s: &mut InteractionState,
    selfState: &PetState,
    neighbors: &[PetState],
) -> Vec<TriggeredAction> {
    let now = Instant::now();
    let mut out = Vec::new();
    for rule in s.config.rules.clone() {
        if let Some(prev) = s.lastFire.get(&rule.id) {
            if (now - *prev).as_millis() < rule.cooldownMs as u128 {
                continue;
            }
        }
        let target = pickNeighbor(&rule, selfState, neighbors);
        let target = match target {
            Some(t) => t,
            None => continue,
        };
        s.lastFire.insert(rule.id.clone(), now);
        for a in &rule.actions {
            out.push(TriggeredAction {
                kind: a.r#type.clone(),
                targetX: target.screenX,
                targetY: target.screenY,
                motion: a.name.clone(),
                text: a.text.clone(),
                speed: a.speed.unwrap_or(80.0),
                durationMs: a.durationMs.unwrap_or(1000),
            });
        }
    }
    out
}

fn pickNeighbor<'a>(
    rule: &InteractionRule,
    self_: &PetState,
    neighbors: &'a [PetState],
) -> Option<&'a PetState> {
    let trigger = &rule.trigger;
    if let Some(states) = &trigger.selfState {
        if !states.contains(&self_.behavior) {
            return None;
        }
    }
    let mut best: Option<(f32, &PetState)> = None;
    for n in neighbors {
        if n.petId == self_.petId {
            continue;
        }
        if let Some(states) = &trigger.otherState {
            if !states.contains(&n.behavior) {
                continue;
            }
        }
        let dx = n.screenX - self_.screenX;
        let dy = n.screenY - self_.screenY;
        let dist = (dx * dx + dy * dy).sqrt();
        if let Some(d) = trigger.distancePx {
            if dist > d {
                continue;
            }
        }
        match best {
            None => best = Some((dist, n)),
            Some((bd, _)) if dist < bd => best = Some((dist, n)),
            _ => {}
        }
    }
    best.map(|(_, n)| n)
}
