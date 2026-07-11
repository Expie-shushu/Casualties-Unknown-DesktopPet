// squat：屈膝下蹲循环。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::f32::consts::PI;

use super::{Baseline, LimbOffset, LimbOffsets};

const FREQ: f32 = 0.55;
const HIP_BEND_DEG: f32 = 30.0;
const KNEE_BEND_DEG: f32 = 60.0;
const TORSO_DROP: f32 = 0.45;
const ARM_FORWARD_DEG: f32 = 60.0;

pub fn motion(t: f32, _base: &Baseline) -> LimbOffsets {
    let mut out: HashMap<String, LimbOffset> = HashMap::new();
    let s = ((2.0 * PI * FREQ * t).sin() + 1.0) * 0.5;

    out.insert("ThighF".into(), LimbOffset { dRotZ: s * HIP_BEND_DEG, ..Default::default() });
    out.insert("ThighB".into(), LimbOffset { dRotZ: -s * HIP_BEND_DEG, ..Default::default() });
    out.insert("CrusF".into(), LimbOffset { dRotZ: -s * KNEE_BEND_DEG, ..Default::default() });
    out.insert("CrusB".into(), LimbOffset { dRotZ: -s * KNEE_BEND_DEG, ..Default::default() });
    out.insert("UpArmF".into(), LimbOffset { dRotZ: s * ARM_FORWARD_DEG, ..Default::default() });
    out.insert("UpArmB".into(), LimbOffset { dRotZ: s * ARM_FORWARD_DEG, ..Default::default() });
    out.insert("DownTorso".into(), LimbOffset { dpy: -s * TORSO_DROP, ..Default::default() });
    out.insert("UpTorso".into(), LimbOffset { dpy: -s * TORSO_DROP * 0.8, ..Default::default() });
    out.insert("Head".into(), LimbOffset { dpy: -s * TORSO_DROP * 0.6, ..Default::default() });
    out
}
