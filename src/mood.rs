// 桌宠心情系统：根据物理与行为状态自动切换 mood，决定 FacialExpression 眼睛 sprite。
#![allow(non_snake_case)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Neutral,
    Happy,
    Sad,
    Sleepy,
    Scared,
    Panic,
    Closed,
}

#[derive(Debug, Clone, Copy)]
pub struct MoodInputs {
    pub dragging: bool,
    pub grounded: bool,
    pub idleTimeSec: f32,
    pub pickedMotion: Option<&'static str>,
    pub vyDownPositive: f32,
    pub falling: bool,
    /// 心情数值（0~100），来自 needs 系统。
    pub moodValue: f32,
    /// 刚喂食的短时窗口内：强制开心表情。
    pub feedingHappy: bool,
}

/// 按物理 / 行为状态确定 mood，与 FacialExpression.Update 的优先级近似：先 panic（巨大下落）/ 再 scared（拖拽 / 高速下落）/ 再 happy（指定动作 / 喂食）/ 再按心情数值（低→Sad，极低→Closed）/ 再 sleepy（idle 太久）。
pub fn updateMood(inputs: MoodInputs) -> Mood {
    if inputs.falling && inputs.vyDownPositive > 600.0 {
        return Mood::Panic;
    }
    if inputs.dragging {
        return Mood::Scared;
    }
    if inputs.falling && inputs.vyDownPositive > 200.0 {
        return Mood::Scared;
    }
    if inputs.feedingHappy {
        // 喂食时眼睛按心情值分级：高(≥60)→开心 / 低(≤30)→悲伤 / 其他→中性
        if inputs.moodValue >= 60.0 {
            return Mood::Happy;
        }
        if inputs.moodValue <= crate::needs::LOW {
            return Mood::Sad;
        }
        return Mood::Neutral;
    }
    if let Some(motion) = inputs.pickedMotion {
        if matches!(motion, "paw" | "happy") {
            return Mood::Happy;
        }
    }
    // 心情数值偏低时主导表情（低于 sleepy/neutral 优先级，但在巨大物理事件之后）。
    if inputs.moodValue <= crate::needs::CRITICAL {
        return Mood::Closed;
    }
    if inputs.moodValue <= crate::needs::LOW {
        return Mood::Sad;
    }
    if inputs.idleTimeSec > 16.0 {
        return Mood::Closed;
    }
    if inputs.idleTimeSec > 8.0 {
        return Mood::Sleepy;
    }
    Mood::Neutral
}

pub fn moodToEyeSprite(mood: Mood) -> &'static str {
    match mood {
        Mood::Happy => "experimentEyeHappy",
        Mood::Sad => "experimentEyeSad",
        Mood::Sleepy => "experimentEyeHalfClosed",
        Mood::Scared => "experimentEyeScared",
        Mood::Panic => "experimentEyePanic",
        Mood::Closed => "experimentEyeClosed",
        Mood::Neutral => "experimentEyeOpen",
    }
}
