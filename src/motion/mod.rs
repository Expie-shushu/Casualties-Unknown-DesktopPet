// 动作名常量与中文标签。实际动画由 Unity Mecanim 控制器驱动（AnimPlayer），
// 此处仅作为动作名注册表供菜单/设置面板使用。
#![allow(non_snake_case)]

/// 非训练动作（双击 paw、行为决策器 walk/run、默认 idle）。
pub const ALL_MOTION_NAMES: &[&str] = &["idle", "walk", "run", "paw"];

/// 训练动作（右键菜单"训练"子菜单）。执行时行为状态机暂停，避免自动走动打断。
pub const TRAINING_MOTION_NAMES: &[&str] = &["pushup", "squat", "plank"];

/// 动作 id → 中文显示名（用于右键菜单 / 设置面板）。未知 id 原样返回。
pub fn motionLabelZh(name: &str) -> &str {
    match name {
        "idle" => "待机",
        "walk" => "行走",
        "run" => "奔跑",
        "pushup" => "俯卧撑",
        "squat" => "深蹲",
        "plank" => "平板支撑",
        "paw" => "招手",
        other => other,
    }
}
