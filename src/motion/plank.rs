// plank：手臂直撑、躯干水平、轻微呼吸晃动。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::f32::consts::PI;

use super::{Baseline, LimbOffset, LimbOffsets};

const BREATH_FREQ: f32 = 0.3;
const BREATH_AMP: f32 = 0.04;

pub fn motion(t: f32, _base: &Baseline) -> LimbOffsets {
    let mut out: HashMap<String, LimbOffset> = HashMap::new();
    let breath = (2.0 * PI * BREATH_FREQ * t).sin();

    out.insert("UpArmF".into(), LimbOffset { dRotZ: 90.0, ..Default::default() });
    out.insert("UpArmB".into(), LimbOffset { dRotZ: 90.0, ..Default::default() });
    out.insert("ThighF".into(), LimbOffset { dRotZ: 80.0, ..Default::default() });
    out.insert("ThighB".into(), LimbOffset { dRotZ: 80.0, ..Default::default() });
    out.insert("DownTorso".into(), LimbOffset { dpy: -0.6 + breath * BREATH_AMP, dRotZ: -85.0, ..Default::default() });
    out.insert("UpTorso".into(), LimbOffset { dpy: -0.6 + breath * BREATH_AMP, dRotZ: -85.0, ..Default::default() });
    out.insert("Head".into(), LimbOffset { dpy: -0.6 + breath * BREATH_AMP, dRotZ: -85.0, ..Default::default() });
    out
}
