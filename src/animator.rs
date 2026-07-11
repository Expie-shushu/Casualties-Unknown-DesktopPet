// 帧调度 + spring 追踪：每帧按当前 motion 算偏移，spring 平滑到实际 LimbPose。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::time::Instant;

use crate::motion::{getMotion, Baseline, LimbOffsets, MotionFn};
use crate::pose::{LimbPose, PetPose};
use crate::spring::Spring;

const SPRING_FREQ_HZ: f32 = 4.0;
const SPRING_DAMP: f32 = 0.7;
const MAX_DT_SEC: f32 = 0.05;

#[derive(Default, Clone, Copy)]
struct LimbSprings {
    px: Spring,
    py: Spring,
    rotZ: Spring,
    scaleX: Spring,
    scaleY: Spring,
}

pub struct Animator {
    pub baseline: Baseline,
    pub baseLimbs: Vec<LimbPose>,
    springs: HashMap<String, LimbSprings>,
    motionName: String,
    motionFn: Option<MotionFn>,
    startedAt: Instant,
    lastNow: Instant,
}

impl Animator {
    pub fn fromPose(pose: &PetPose, initial: &str) -> Self {
        let mut springs = HashMap::with_capacity(pose.limbs.len());
        let mut baseline = HashMap::with_capacity(pose.limbs.len());
        for limb in &pose.limbs {
            springs.insert(
                limb.name.clone(),
                LimbSprings {
                    px: Spring::new(limb.px),
                    py: Spring::new(limb.py),
                    rotZ: Spring::new(limb.rotZ),
                    scaleX: Spring::new(limb.scaleX),
                    scaleY: Spring::new(limb.scaleY),
                },
            );
            baseline.insert(limb.name.clone(), limb.clone());
        }
        let now = Instant::now();
        Self {
            baseline,
            baseLimbs: pose.limbs.clone(),
            springs,
            motionName: initial.to_string(),
            motionFn: getMotion(initial),
            startedAt: now,
            lastNow: now,
        }
    }

    pub fn currentMotion(&self) -> &str {
        &self.motionName
    }

    pub fn setMotion(&mut self, name: &str) {
        if self.motionName == name {
            return;
        }
        self.motionName = name.to_string();
        self.motionFn = getMotion(name);
    }

    pub fn tickAndOutput(&mut self) -> Vec<LimbPose> {
        let now = Instant::now();
        let dt = (now - self.lastNow).as_secs_f32().min(MAX_DT_SEC);
        self.lastNow = now;
        let t = (now - self.startedAt).as_secs_f32();

        let offsets: LimbOffsets = self
            .motionFn
            .map(|f| f(t, &self.baseline))
            .unwrap_or_default();

        let mut out = Vec::with_capacity(self.baseLimbs.len());
        for limb in &self.baseLimbs {
            let s = match self.springs.get_mut(&limb.name) {
                Some(s) => s,
                None => continue,
            };
            let off = offsets.get(&limb.name).copied().unwrap_or_default();
            let target_px = limb.px + off.dpx;
            let target_py = limb.py + off.dpy;
            let target_rotZ = limb.rotZ + off.dRotZ;
            let target_sx = limb.scaleX + off.dScaleX;
            let target_sy = limb.scaleY + off.dScaleY;
            s.px.step(target_px, SPRING_FREQ_HZ, SPRING_DAMP, dt);
            s.py.step(target_py, SPRING_FREQ_HZ, SPRING_DAMP, dt);
            s.rotZ.step(target_rotZ, SPRING_FREQ_HZ, SPRING_DAMP, dt);
            s.scaleX.step(target_sx, SPRING_FREQ_HZ, SPRING_DAMP, dt);
            s.scaleY.step(target_sy, SPRING_FREQ_HZ, SPRING_DAMP, dt);
            let mut effective = limb.clone();
            effective.px = s.px.current;
            effective.py = s.py.current;
            effective.rotZ = s.rotZ.current;
            effective.scaleX = s.scaleX.current;
            effective.scaleY = s.scaleY.current;
            out.push(effective);
        }
        out
    }
}
