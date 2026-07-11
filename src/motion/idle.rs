// idle 动作：呼吸 scale + 头摇 + 整体上下浮动。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::f32::consts::PI;

use super::{Baseline, LimbOffset, LimbOffsets};

const BREATH_FREQ: f32 = 0.4;
const BREATH_AMP: f32 = 0.025;
const HEAD_SWAY_FREQ: f32 = 0.18;
const HEAD_SWAY_DEG: f32 = 1.8;
const ROOT_BOB_FREQ: f32 = 0.4;
const ROOT_BOB_AMP: f32 = 0.04;

pub fn motion(t: f32, _base: &Baseline) -> LimbOffsets {
    let mut out: HashMap<String, LimbOffset> = HashMap::new();
    let breath = (2.0 * PI * BREATH_FREQ * t).sin();
    let sway = (2.0 * PI * HEAD_SWAY_FREQ * t).sin();
    let bob = ((2.0 * PI * ROOT_BOB_FREQ * t).sin()).abs();

    out.insert(
        "UpTorso".into(),
        LimbOffset {
            dScaleX: breath * BREATH_AMP,
            dScaleY: breath * BREATH_AMP * 0.6,
            ..Default::default()
        },
    );
    out.insert(
        "DownTorso".into(),
        LimbOffset {
            dpy: bob * ROOT_BOB_AMP,
            ..Default::default()
        },
    );
    out.insert(
        "Head".into(),
        LimbOffset {
            dRotZ: sway * HEAD_SWAY_DEG,
            dpy: bob * ROOT_BOB_AMP,
            ..Default::default()
        },
    );
    out
}
