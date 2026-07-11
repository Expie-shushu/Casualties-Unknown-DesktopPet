// pushup：四肢撑地，躯干上下起伏。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::f32::consts::PI;

use super::{Baseline, LimbOffset, LimbOffsets};

const FREQ: f32 = 0.7;
const PUSH_AMP: f32 = 0.35;

pub fn motion(t: f32, _base: &Baseline) -> LimbOffsets {
    let mut out: HashMap<String, LimbOffset> = HashMap::new();
    let s = (2.0 * PI * FREQ * t).sin();
    let lift = (s + 1.0) * 0.5;

    out.insert("UpArmF".into(), LimbOffset { dRotZ: 90.0, ..Default::default() });
    out.insert("UpArmB".into(), LimbOffset { dRotZ: 90.0, ..Default::default() });
    out.insert("DownArmF".into(), LimbOffset { dRotZ: -lift * 60.0, ..Default::default() });
    out.insert("DownArmB".into(), LimbOffset { dRotZ: -lift * 60.0, ..Default::default() });
    out.insert("ThighF".into(), LimbOffset { dRotZ: 80.0, ..Default::default() });
    out.insert("ThighB".into(), LimbOffset { dRotZ: 80.0, ..Default::default() });
    out.insert("DownTorso".into(), LimbOffset { dpy: -0.5 - lift * PUSH_AMP, dRotZ: -85.0, ..Default::default() });
    out.insert("UpTorso".into(), LimbOffset { dpy: -0.5 - lift * PUSH_AMP, dRotZ: -85.0, ..Default::default() });
    out.insert("Head".into(), LimbOffset { dpy: -0.5 - lift * PUSH_AMP, dRotZ: -85.0, ..Default::default() });
    out
}
