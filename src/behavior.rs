// 行为状态机：根据 cooldown / 概率 / 物理状态切换 idle / walk / falling / drag。
#![allow(non_snake_case)]

use std::time::Instant;

use crate::physics::{setWalkVelocity, PhysicsConfig, PhysicsState};

pub fn setRunVelocity(s: &mut PhysicsState, cfg: &PhysicsConfig, dir: i32) {
    let dir = if dir >= 0 { 1 } else { -1 };
    let runSpeed = cfg.walkSpeed * 2.5;
    s.vx = dir as f32 * runSpeed;
    s.facing = dir;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BehaviorName {
    Idle,
    Walk,
    Run,
    Falling,
    Drag,
}

impl BehaviorName {
    pub fn asStr(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Walk => "walk",
            Self::Run => "run",
            Self::Falling => "falling",
            Self::Drag => "drag",
        }
    }
}

pub struct BehaviorState {
    pub current: BehaviorName,
    pub enteredAt: Instant,
    pub nextDecisionAt: Instant,
}

impl Default for BehaviorState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            current: BehaviorName::Idle,
            enteredAt: now,
            nextDecisionAt: now + std::time::Duration::from_millis(2000),
        }
    }
}

pub fn tick(
    s: &mut BehaviorState,
    physics: &mut PhysicsState,
    cfg: &PhysicsConfig,
    dragging: bool,
    walkSec: f32,
    runSec: f32,
    allowRun: bool,
) -> BehaviorName {
    let now = Instant::now();
    if dragging {
        enter(s, BehaviorName::Drag, now);
        return s.current;
    }
    if !physics.grounded && physics.vy.abs() > 30.0 {
        enter(s, BehaviorName::Falling, now);
        return s.current;
    }
    if matches!(s.current, BehaviorName::Falling) && physics.grounded {
        enter(s, BehaviorName::Idle, now);
    }
    if matches!(s.current, BehaviorName::Drag) && !dragging {
        enter(s, BehaviorName::Idle, now);
    }
    match s.current {
        BehaviorName::Walk => physics.vx = physics.facing as f32 * cfg.walkSpeed,
        BehaviorName::Run => physics.vx = physics.facing as f32 * cfg.walkSpeed * 2.5,
        _ => {}
    }
    if now < s.nextDecisionAt {
        return s.current;
    }

    match s.current {
        BehaviorName::Idle => {
            if pickProb(0.5) {
                let dir = if pickProb(0.5) { -1 } else { 1 };
                // allowRun=false（心情低）时不奔跑，只随机走动。
                if allowRun && pickProb(0.35) {
                    setRunVelocity(physics, cfg, dir);
                    enter(s, BehaviorName::Run, now);
                    s.nextDecisionAt = now + std::time::Duration::from_millis((runSec * 1000.0) as u64);
                } else {
                    setWalkVelocity(physics, cfg, dir);
                    enter(s, BehaviorName::Walk, now);
                    s.nextDecisionAt = now + std::time::Duration::from_millis((walkSec * 1000.0) as u64);
                }
            } else {
                s.nextDecisionAt = now + std::time::Duration::from_millis(2000 + (rand01() * 3000.0) as u64);
            }
        }
        BehaviorName::Walk | BehaviorName::Run => {
            physics.vx = 0.0;
            enter(s, BehaviorName::Idle, now);
            s.nextDecisionAt = now + std::time::Duration::from_millis(1500 + (rand01() * 2500.0) as u64);
        }
        _ => {
            s.nextDecisionAt = now + std::time::Duration::from_millis(1000);
        }
    }
    s.current
}

fn enter(s: &mut BehaviorState, name: BehaviorName, now: Instant) {
    if s.current == name {
        return;
    }
    s.current = name;
    s.enteredAt = now;
    s.nextDecisionAt = now + std::time::Duration::from_millis(800);
}

pub fn behaviorToMotion(name: BehaviorName) -> &'static str {
    match name {
        BehaviorName::Walk => "walk",
        BehaviorName::Run => "run",
        BehaviorName::Falling => "idle",
        BehaviorName::Drag => "idle",
        BehaviorName::Idle => "idle",
    }
}

pub(crate) fn rand01() -> f32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    ((nanos.wrapping_mul(2654435761)) as f32 / u32::MAX as f32).fract().abs()
}

fn pickProb(p: f32) -> bool {
    rand01() < p
}
