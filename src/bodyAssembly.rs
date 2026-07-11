// 把 bodyPlayer/armsPlayer 的 clip 采样合成 15 limb 的 body 局部位姿（游戏朝右原始坐标）。
// body limb 的 clip 值直接是 body 局部；arm limb 的 clip 是 Arms 节点局部，
// 需经 UpTorso 动画位姿 ∘ Arms 固定偏移 ∘ armClip 三级 FK（对标 Body.LateUpdate）。
#![allow(non_snake_case)]

use std::collections::HashMap;

use crate::animClip::LimbSample;
use crate::pose::LimbPose;

const ARMS_OFFSET_X: f32 = -0.117;
const ARMS_OFFSET_Y: f32 = 0.128;
const ARM_LIMBS: [&str; 6] = ["UpArmF", "DownArmF", "HandF", "UpArmB", "DownArmB", "HandB"];

fn norm360(z: f32) -> f32 {
    z.rem_euclid(360.0)
}

fn armBodyLocal(ut: &LimbSample, arm: &LimbSample) -> (f32, f32, f32) {
    let utz = ut.eulerZ.to_radians();
    let (uc, us) = (utz.cos(), utz.sin());
    let armsX = ut.px + uc * ARMS_OFFSET_X - us * ARMS_OFFSET_Y;
    let armsY = ut.py + us * ARMS_OFFSET_X + uc * ARMS_OFFSET_Y;
    let armsZ = ut.eulerZ;
    let arad = armsZ.to_radians();
    let (ac, as_) = (arad.cos(), arad.sin());
    let ax = armsX + ac * arm.px - as_ * arm.py;
    let ay = armsY + as_ * arm.px + ac * arm.py;
    (ax, ay, armsZ + arm.eulerZ)
}

/// 合成 15 limb 的 body 局部位姿（朝右原始坐标）；facing 镜像由 buildDrawsFrom 处理。
/// baseline 提供 spriteName/sortingOrder 等元数据；缺采样的 limb 保留 baseline。
pub fn assembleLimbs(
    baseline: &[LimbPose],
    body: &HashMap<String, LimbSample>,
    arms: &HashMap<String, LimbSample>,
) -> Vec<LimbPose> {
    let upTorso = body.get("UpTorso").copied();
    baseline
        .iter()
        .map(|b| {
            let mut l = b.clone();
            if ARM_LIMBS.contains(&b.name.as_str()) {
                if let (Some(ut), Some(a)) = (upTorso, arms.get(&b.name)) {
                    if a.hasPos {
                        let (ax, ay, az) = armBodyLocal(&ut, a);
                        l.px = ax;
                        l.py = ay;
                        l.rotZ = norm360(az);
                    }
                }
            } else if let Some(s) = body.get(&b.name) {
                if s.hasPos {
                    l.px = s.px;
                    l.py = s.py;
                }
                if s.hasEuler {
                    l.rotZ = norm360(s.eulerZ);
                }
            }
            l
        })
        .collect()
}
