// 尾巴姿态：移植反编译 TailScript.Update — 单张 sprite 整体旋转，无切片无 mesh，按速度方向 LerpAngle 平滑。
#![allow(non_snake_case)]

#[derive(Debug, Clone, Copy, Default)]
pub struct TailState {
    pub eulerZ: f32,
}

/// 按 TailScript.Update 算尾巴 localEulerAngles.z；vx/vy 是物理 px/sec（vy 向下为正）。
pub fn tickTail(state: &mut TailState, vx: f32, vy: f32, isRight: bool, dt: f32) -> f32 {
    let vxUnit = vx / 8.0;
    let vyUp = -vy / 8.0;
    let speed = (vxUnit * vxUnit + vyUp * vyUp).sqrt();

    if speed > dt && speed > 1e-3 {
        let nx = vxUnit / speed;
        let ny = vyUp / speed;
        let mut offX = nx;
        let mut offY = ny - 0.1;
        let m = (offX * offX + offY * offY).sqrt().max(1e-5);
        offX /= m;
        offY /= m;
        if !isRight {
            offX = -offX;
        }
        let target = offY.atan2(offX).to_degrees();
        let factor = (dt * speed * 0.14).clamp(0.0, 1.0);
        state.eulerZ = lerpAngle(state.eulerZ, target, factor);
    }

    if speed < 1.4 {
        let factor = (dt * 0.78).clamp(0.0, 1.0);
        state.eulerZ = lerpAngle(state.eulerZ, 24.0, factor);
    }

    state.eulerZ = state.eulerZ.clamp(-80.0, 80.0);
    state.eulerZ
}

fn lerpAngle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a).rem_euclid(360.0);
    if diff > 180.0 {
        diff -= 360.0;
    }
    a + diff * t
}
