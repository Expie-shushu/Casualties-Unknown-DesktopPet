// paw：单臂挥爪 0.6s 循环。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::f32::consts::PI;

use super::{Baseline, LimbOffset, LimbOffsets};

const CYCLE_SEC: f32 = 0.6;

pub fn motion(t: f32, _base: &Baseline) -> LimbOffsets {
    let mut out: HashMap<String, LimbOffset> = HashMap::new();
    let phase = (t.rem_euclid(CYCLE_SEC)) / CYCLE_SEC;
    let (upArm, downArm) = if phase < 0.4 {
        let k = phase / 0.4;
        (-k * 75.0, -k * 30.0)
    } else if phase < 0.7 {
        let k = (phase - 0.4) / 0.3;
        (-75.0 + k * 110.0, -30.0 + k * 50.0)
    } else {
        let k = (phase - 0.7) / 0.3;
        (35.0 - k * 35.0, 20.0 - k * 20.0)
    };

    out.insert("UpArmF".into(), LimbOffset { dRotZ: upArm, ..Default::default() });
    out.insert("DownArmF".into(), LimbOffset { dRotZ: downArm, ..Default::default() });
    out.insert("Head".into(), LimbOffset { dRotZ: (phase * PI).sin() * 6.0, ..Default::default() });
    out
}
