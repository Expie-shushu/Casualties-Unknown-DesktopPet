// 客户区点 (clientPx) 是否落在任意可见 sprite 的旋转 AABB 上；用于 smart 穿透判定。
#![allow(non_snake_case)]

use crate::pose::LimbPose;

pub fn anyLimbBboxContains(
    limbs: &[LimbPose],
    clientX: f32,
    clientY: f32,
    centerX: f32,
    centerY: f32,
    facingSign: f32,
    unitToPx: f32,
    pxRatio: f32,
) -> bool {
    for limb in limbs.iter().filter(|l| l.visible) {
        let cx = centerX + limb.px * unitToPx * facingSign;
        let cy = centerY - limb.py * unitToPx;
        let halfW = (limb.spriteW * pxRatio * limb.scaleX.abs()) * 0.5;
        let halfH = (limb.spriteH * pxRatio * limb.scaleY.abs()) * 0.5;
        let rad = limb.rotZ.to_radians();
        let cos = rad.cos().abs();
        let sin = rad.sin().abs();
        let extX = halfW * cos + halfH * sin;
        let extY = halfW * sin + halfH * cos;
        if (clientX - cx).abs() <= extX && (clientY - cy).abs() <= extY {
            return true;
        }
    }
    false
}
