// AnimationClip JSON 加载 + Hermite 插值。每条 track = 单 limb 的 position 或 eulerAngles 的单 axis 曲线。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackAttr {
    Position,
    EulerAngles,
    Scale,
    RotationQuat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackAxis {
    X,
    Y,
    Z,
    W,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeyFrame {
    pub t: f32,
    pub v: f32,
    #[serde(default)]
    pub co: [f32; 4],
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackData {
    pub limb: String,
    pub attr: TrackAttr,
    pub axis: TrackAxis,
    #[serde(default)]
    pub curveIdx: i32,
    pub keys: Vec<KeyFrame>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnimClip {
    pub name: String,
    pub duration: f32,
    #[serde(default)]
    pub sampleRate: f32,
    pub tracks: Vec<TrackData>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LimbSample {
    pub px: f32,
    pub py: f32,
    pub pz: f32,
    pub eulerX: f32,
    pub eulerY: f32,
    pub eulerZ: f32,
    pub scaleX: f32,
    pub scaleY: f32,
    pub scaleZ: f32,
    pub hasPos: bool,
    pub hasEuler: bool,
}

impl LimbSample {
    pub fn defaultRest() -> Self {
        Self {
            scaleX: 1.0,
            scaleY: 1.0,
            scaleZ: 1.0,
            ..Default::default()
        }
    }
}

/// 从单 track 在时刻 t 求值：找到 t 所在的 key 区间，按 Hermite 多项式 c0·Δt³ + c1·Δt² + c2·Δt + c3 求值。
pub fn evaluateTrack(track: &TrackData, t: f32) -> f32 {
    let keys = &track.keys;
    if keys.is_empty() {
        return 0.0;
    }
    if keys.len() == 1 || t <= keys[0].t {
        return keys[0].v;
    }
    let last = &keys[keys.len() - 1];
    if t >= last.t {
        return last.v;
    }
    let mut lo = 0usize;
    let mut hi = keys.len() - 1;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if keys[mid].t <= t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let k = &keys[lo];
    let dt = t - k.t;
    let c = k.co;
    c[0] * dt * dt * dt + c[1] * dt * dt + c[2] * dt + c[3]
}

/// 在时刻 t 求 clip 中所有 limb 的位姿；t 应已经做过 loop / clamp 处理。
pub fn evaluateClip(clip: &AnimClip, t: f32) -> HashMap<String, LimbSample> {
    let mut out: HashMap<String, LimbSample> = HashMap::new();
    for track in &clip.tracks {
        let v = evaluateTrack(track, t);
        let entry = out
            .entry(track.limb.clone())
            .or_insert_with(LimbSample::defaultRest);
        match (track.attr, track.axis) {
            (TrackAttr::Position, TrackAxis::X) => {
                entry.px = v;
                entry.hasPos = true;
            }
            (TrackAttr::Position, TrackAxis::Y) => {
                entry.py = v;
                entry.hasPos = true;
            }
            (TrackAttr::Position, TrackAxis::Z) => {
                entry.pz = v;
                entry.hasPos = true;
            }
            (TrackAttr::EulerAngles, TrackAxis::X) => {
                entry.eulerX = v;
                entry.hasEuler = true;
            }
            (TrackAttr::EulerAngles, TrackAxis::Y) => {
                entry.eulerY = v;
                entry.hasEuler = true;
            }
            (TrackAttr::EulerAngles, TrackAxis::Z) => {
                entry.eulerZ = v;
                entry.hasEuler = true;
            }
            (TrackAttr::Scale, TrackAxis::X) => entry.scaleX = v,
            (TrackAttr::Scale, TrackAxis::Y) => entry.scaleY = v,
            (TrackAttr::Scale, TrackAxis::Z) => entry.scaleZ = v,
            _ => {}
        }
    }
    out
}

pub fn loopOrClampTime(clip: &AnimClip, time: f32, looped: bool) -> f32 {
    let dur = clip.duration.max(1e-4);
    if looped {
        let m = time.rem_euclid(dur);
        m
    } else {
        time.clamp(0.0, dur)
    }
}

pub fn loadClip(path: &Path) -> Result<AnimClip> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read clip {}", path.display()))?;
    let clip: AnimClip = serde_json::from_str(&text)
        .with_context(|| format!("parse clip {}", path.display()))?;
    Ok(clip)
}

pub fn loadClipsFromDir(dir: &Path) -> Result<HashMap<String, AnimClip>> {
    let mut out = HashMap::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("read clips dir {}", dir.display()))?
    {
        let entry = entry?;
        let p = entry.path();
        if !p.extension().map(|e| e == "json").unwrap_or(false) {
            continue;
        }
        match loadClip(&p) {
            Ok(c) => {
                out.insert(c.name.clone(), c);
            }
            Err(e) => log::warn!("load clip {} failed: {e:?}", p.display()),
        }
    }
    Ok(out)
}
