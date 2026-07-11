// critically damped spring：5 字段 limb transform 的弹性追踪。
#![allow(non_snake_case)]

#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    pub current: f32,
    pub velocity: f32,
}

impl Spring {
    pub fn new(initial: f32) -> Self {
        Self {
            current: initial,
            velocity: 0.0,
        }
    }

    pub fn snap(&mut self, value: f32) {
        self.current = value;
        self.velocity = 0.0;
    }

    pub fn step(&mut self, target: f32, freqHz: f32, damp: f32, dt: f32) {
        let omega = 2.0 * std::f32::consts::PI * freqHz.max(0.001);
        let accel = -2.0 * omega * damp * self.velocity + omega * omega * (target - self.current);
        self.velocity += accel * dt;
        self.current += self.velocity * dt;
    }
}
