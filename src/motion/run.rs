// run：在 walk 公式基础上提频提幅 + 躯干前倾。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::f32::consts::PI;

use super::{Baseline, LimbOffset, LimbOffsets};

const FREQ: f32 = 2.6;
const LEG_SWING_DEG: f32 = 40.0;
const KNEE_BEND_DEG: f32 = 38.0;
const ARM_SWING_DEG: f32 = 32.0;
const ELBOW_BEND_DEG: f32 = 22.0;
const TORSO_BOB_AMP: f32 = 0.14;
const TORSO_LEAN_DEG: f32 = 8.0;
const HEAD_COUNTER_DEG: f32 = 5.0;

pub fn motion(t: f32, _base: &Baseline) -> LimbOffsets {
    let mut out: HashMap<String, LimbOffset> = HashMap::new();
    let phase = 2.0 * PI * FREQ * t;
    let s = phase.sin();
    let s_b = (phase + PI).sin();
    let bob = ((2.0 * phase).sin()).abs();

    out.insert("ThighF".into(), LimbOffset { dRotZ: s * LEG_SWING_DEG, ..Default::default() });
    out.insert("ThighB".into(), LimbOffset { dRotZ: s_b * LEG_SWING_DEG, ..Default::default() });
    out.insert("CrusF".into(), LimbOffset { dRotZ: -(s.max(0.0)) * KNEE_BEND_DEG, ..Default::default() });
    out.insert("CrusB".into(), LimbOffset { dRotZ: -(s_b.max(0.0)) * KNEE_BEND_DEG, ..Default::default() });
    out.insert("UpArmF".into(), LimbOffset { dRotZ: s_b * ARM_SWING_DEG, ..Default::default() });
    out.insert("UpArmB".into(), LimbOffset { dRotZ: s * ARM_SWING_DEG, ..Default::default() });
    out.insert("DownArmF".into(), LimbOffset { dRotZ: -(s_b.max(0.0)) * ELBOW_BEND_DEG, ..Default::default() });
    out.insert("DownArmB".into(), LimbOffset { dRotZ: -(s.max(0.0)) * ELBOW_BEND_DEG, ..Default::default() });
    out.insert("DownTorso".into(), LimbOffset { dpy: bob * TORSO_BOB_AMP, dRotZ: -TORSO_LEAN_DEG, ..Default::default() });
    out.insert("UpTorso".into(), LimbOffset { dpy: bob * TORSO_BOB_AMP * 0.8, dRotZ: -TORSO_LEAN_DEG * 0.5, ..Default::default() });
    out.insert("Head".into(), LimbOffset { dRotZ: (phase + PI / 2.0).sin() * HEAD_COUNTER_DEG, ..Default::default() });
    out
}
