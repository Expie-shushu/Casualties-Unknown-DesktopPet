// walk 动作：上下肢反相位摆动 + 双频躯干颠簸。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::f32::consts::PI;

use super::{Baseline, LimbOffset, LimbOffsets};

const FREQ: f32 = 1.6;
const LEG_SWING_DEG: f32 = 28.0;
const KNEE_BEND_DEG: f32 = 22.0;
const ARM_SWING_DEG: f32 = 22.0;
const ELBOW_BEND_DEG: f32 = 12.0;
const TORSO_BOB_AMP: f32 = 0.08;
const HEAD_COUNTER_DEG: f32 = 4.0;

pub fn motion(t: f32, _base: &Baseline) -> LimbOffsets {
    let mut out: HashMap<String, LimbOffset> = HashMap::new();
    let phase = 2.0 * PI * FREQ * t;
    let s = phase.sin();
    let s_b = (phase + PI).sin();
    let bob = ((2.0 * phase).sin()).abs();

    out.insert(
        "ThighF".into(),
        LimbOffset {
            dRotZ: s * LEG_SWING_DEG,
            ..Default::default()
        },
    );
    out.insert(
        "ThighB".into(),
        LimbOffset {
            dRotZ: s_b * LEG_SWING_DEG,
            ..Default::default()
        },
    );
    out.insert(
        "CrusF".into(),
        LimbOffset {
            dRotZ: -(s.max(0.0)) * KNEE_BEND_DEG,
            ..Default::default()
        },
    );
    out.insert(
        "CrusB".into(),
        LimbOffset {
            dRotZ: -(s_b.max(0.0)) * KNEE_BEND_DEG,
            ..Default::default()
        },
    );
    out.insert(
        "UpArmF".into(),
        LimbOffset {
            dRotZ: s_b * ARM_SWING_DEG,
            ..Default::default()
        },
    );
    out.insert(
        "UpArmB".into(),
        LimbOffset {
            dRotZ: s * ARM_SWING_DEG,
            ..Default::default()
        },
    );
    out.insert(
        "DownArmF".into(),
        LimbOffset {
            dRotZ: -(s_b.max(0.0)) * ELBOW_BEND_DEG,
            ..Default::default()
        },
    );
    out.insert(
        "DownArmB".into(),
        LimbOffset {
            dRotZ: -(s.max(0.0)) * ELBOW_BEND_DEG,
            ..Default::default()
        },
    );
    out.insert(
        "DownTorso".into(),
        LimbOffset {
            dpy: bob * TORSO_BOB_AMP,
            ..Default::default()
        },
    );
    out.insert(
        "UpTorso".into(),
        LimbOffset {
            dpy: bob * TORSO_BOB_AMP * 0.8,
            ..Default::default()
        },
    );
    out.insert(
        "Head".into(),
        LimbOffset {
            dRotZ: (phase + PI / 2.0).sin() * HEAD_COUNTER_DEG,
            ..Default::default()
        },
    );
    out
}
