// jump：单次跳跃循环 1.0s 内分蹲 / 起 / 腾 / 落 4 段。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::f32::consts::PI;

use super::{Baseline, LimbOffset, LimbOffsets};

const CYCLE_SEC: f32 = 1.0;

pub fn motion(t: f32, _base: &Baseline) -> LimbOffsets {
    let mut out: HashMap<String, LimbOffset> = HashMap::new();
    let phase = (t.rem_euclid(CYCLE_SEC)) / CYCLE_SEC;
    let (crouch, armUp, bodyY) = if phase < 0.15 {
        let k = phase / 0.15;
        (k, -k * 8.0, -k * 0.15)
    } else if phase < 0.35 {
        let k = (phase - 0.15) / 0.2;
        (1.0 - k, -8.0 + k * 28.0, -0.15 + k * 0.45)
    } else if phase < 0.65 {
        let k = (phase - 0.35) / 0.3;
        (-0.2 + (k * PI).sin() * 0.4, 20.0 + (k * PI).sin() * 6.0, 0.3 + (k * PI).sin() * 0.18)
    } else {
        let k = (phase - 0.65) / 0.35;
        (0.3 - k * 0.3, 20.0 - k * 28.0, 0.3 - k * 0.3)
    };

    out.insert("ThighF".into(), LimbOffset { dRotZ: crouch * 35.0, ..Default::default() });
    out.insert("ThighB".into(), LimbOffset { dRotZ: -crouch * 35.0, ..Default::default() });
    out.insert("CrusF".into(), LimbOffset { dRotZ: -crouch.max(0.0) * 50.0, ..Default::default() });
    out.insert("CrusB".into(), LimbOffset { dRotZ: -crouch.max(0.0) * 50.0, ..Default::default() });
    out.insert("UpArmF".into(), LimbOffset { dRotZ: armUp, ..Default::default() });
    out.insert("UpArmB".into(), LimbOffset { dRotZ: armUp, ..Default::default() });
    out.insert("DownTorso".into(), LimbOffset { dpy: bodyY, ..Default::default() });
    out.insert("UpTorso".into(), LimbOffset { dpy: bodyY * 0.8, ..Default::default() });
    out.insert("Head".into(), LimbOffset { dpy: bodyY * 0.6, ..Default::default() });
    out
}
