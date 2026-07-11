// 桌宠窗口物理：重力 / 屏幕边界 / 拖拽时禁用 / 撞边转身。
#![allow(non_snake_case)]

#[derive(Clone, Copy, Debug)]
pub struct PhysicsState {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub facing: i32,
    pub grounded: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ScreenBounds {
    pub minX: f32,
    pub maxX: f32,
    pub groundY: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct PhysicsConfig {
    pub gravity: f32,
    pub airDrag: f32,
    pub groundDrag: f32,
    pub walkSpeed: f32,
    pub windowW: f32,
    pub windowH: f32,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            gravity: 1400.0,
            airDrag: 0.08,
            groundDrag: 4.0,
            walkSpeed: 80.0,
            windowW: 360.0,
            windowH: 360.0,
        }
    }
}

impl PhysicsState {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            facing: 1,
            grounded: false,
        }
    }
}

pub fn step(s: &mut PhysicsState, cfg: &PhysicsConfig, bounds: &ScreenBounds, dragging: bool, dt: f32) {
    if dragging {
        s.vx = 0.0;
        s.vy = 0.0;
        s.grounded = false;
        return;
    }
    s.vy += cfg.gravity * dt;
    let drag = if s.grounded { cfg.groundDrag } else { cfg.airDrag };
    s.vx -= s.vx * (drag * dt).min(1.0);

    s.x += s.vx * dt;
    s.y += s.vy * dt;

    if s.x < bounds.minX {
        s.x = bounds.minX;
        s.vx = s.vx.abs();
        s.facing = 1;
    }
    let maxX = bounds.maxX - cfg.windowW;
    if s.x > maxX {
        s.x = maxX;
        s.vx = -s.vx.abs();
        s.facing = -1;
    }
    if s.y >= bounds.groundY {
        s.y = bounds.groundY;
        s.vy = 0.0;
        s.grounded = true;
    } else {
        s.grounded = false;
    }
}

pub fn setWalkVelocity(s: &mut PhysicsState, cfg: &PhysicsConfig, dir: i32) {
    let dir = if dir >= 0 { 1 } else { -1 };
    s.vx = dir as f32 * cfg.walkSpeed;
    s.facing = dir;
}
