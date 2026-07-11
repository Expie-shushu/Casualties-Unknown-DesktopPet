// 桌宠动作注册：返回相对基线的 5 字段偏移；运行时 spring 追踪。
#![allow(non_snake_case)]

use std::collections::HashMap;

use crate::pose::LimbPose;

pub mod idle;
pub mod jump;
pub mod paw;
pub mod plank;
pub mod pushup;
pub mod run;
pub mod squat;
pub mod walk;

#[derive(Default, Clone, Copy, Debug)]
pub struct LimbOffset {
    pub dpx: f32,
    pub dpy: f32,
    pub dRotZ: f32,
    pub dScaleX: f32,
    pub dScaleY: f32,
}

pub type LimbOffsets = HashMap<String, LimbOffset>;
pub type Baseline = HashMap<String, LimbPose>;
pub type MotionFn = fn(t: f32, base: &Baseline) -> LimbOffsets;

pub fn getMotion(name: &str) -> Option<MotionFn> {
    match name {
        "idle" => Some(idle::motion),
        "walk" => Some(walk::motion),
        "run" => Some(run::motion),
        "jump" => Some(jump::motion),
        "pushup" => Some(pushup::motion),
        "squat" => Some(squat::motion),
        "plank" => Some(plank::motion),
        "paw" => Some(paw::motion),
        _ => None,
    }
}

pub const ALL_MOTION_NAMES: &[&str] = &[
    "idle", "walk", "run", "pushup", "squat", "plank", "paw",
];

/// 动作 id → 中文显示名（用于右键菜单 / 设置面板）。未知 id 原样返回。
pub fn motionLabelZh(name: &str) -> &str {
    match name {
        "idle" => "待机",
        "walk" => "行走",
        "run" => "奔跑",
        "jump" => "跳跃",
        "pushup" => "俯卧撑",
        "squat" => "深蹲",
        "plank" => "平板支撑",
        "paw" => "招手",
        other => other,
    }
}
