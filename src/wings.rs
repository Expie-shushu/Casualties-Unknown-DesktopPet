// 翅膀按 SkinSync WingScript + WingsConfigLoader.Defaults 重写：grounded/vy/crouching 切换 spread，lerp 逼近，按父子链累加位置/角度。
#![allow(non_snake_case)]

const PIXELS_PER_UNIT: f32 = 8.0;
const UPTORSO_OFFSET_X: f32 = 6.0;
const UPTORSO_OFFSET_Y: f32 = -10.0;

#[derive(Clone, Copy, Debug)]
pub struct WingsConfig {
    pub restAngleDeg: f32,
    pub maxAngleDeg: f32,
    pub crouchSpread: f32,
    pub jumpSpread: f32,
    pub fallSpread: f32,
    pub lerpSpeed: f32,
}

impl Default for WingsConfig {
    fn default() -> Self {
        Self {
            restAngleDeg: 18.0,
            maxAngleDeg: 110.0,
            crouchSpread: 0.30,
            jumpSpread: 0.55,
            fallSpread: 1.0,
            lerpSpeed: 8.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WingPiece {
    pub xPx: i32,
    pub yPx: i32,
    pub rotationDeg: f32,
    pub zOrder: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct WingsLayout {
    pub wingUL: WingPiece,
    pub wingDL: WingPiece,
    pub wingUR: WingPiece,
    pub wingDR: WingPiece,
}

impl Default for WingsLayout {
    fn default() -> Self {
        Self {
            wingUL: WingPiece { xPx: -2, yPx: -8, rotationDeg: 314.0, zOrder: 5 },
            wingDL: WingPiece { xPx: -1, yPx: -10, rotationDeg: 0.0, zOrder: 4 },
            wingUR: WingPiece { xPx: -2, yPx: -9, rotationDeg: 318.0, zOrder: 5 },
            wingDR: WingPiece { xPx: 0, yPx: -12, rotationDeg: 0.0, zOrder: 4 },
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WingsState {
    spreadUL: f32,
    spreadUR: f32,
    spreadDL: f32,
    spreadDR: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct WingDynAngles {
    pub wingUL: f32,
    pub wingUR: f32,
    pub wingDL: f32,
    pub wingDR: f32,
}

/// 按 SkinSync WingScript.Update 算 4 张翅膀相对父级的本地动态偏角；不含 baseAngleDeg。
pub fn step(
    state: &mut WingsState,
    cfg: WingsConfig,
    grounded: bool,
    vyDownPositive: f32,
    crouching: bool,
    isRight: bool,
    dt: f32,
) -> WingDynAngles {
    let target = if !grounded && vyDownPositive > 0.1 {
        cfg.fallSpread
    } else if !grounded && vyDownPositive < -0.1 {
        cfg.jumpSpread
    } else if crouching {
        cfg.crouchSpread
    } else {
        0.0
    };
    let alpha = (dt * cfg.lerpSpeed).clamp(0.0, 1.0);
    state.spreadUL += (target - state.spreadUL) * alpha;
    state.spreadUR += (target - state.spreadUR) * alpha;
    state.spreadDL += (target - state.spreadDL) * alpha;
    state.spreadDR += (target - state.spreadDR) * alpha;

    let dynAngle = |spread: f32, isLeft: bool| -> f32 {
        let spreadDelta = (cfg.maxAngleDeg - cfg.restAngleDeg) * spread;
        let mut d = if isLeft {
            -(cfg.restAngleDeg + spreadDelta)
        } else {
            cfg.restAngleDeg + spreadDelta
        };
        if !isRight {
            d = -d;
        }
        d
    };
    WingDynAngles {
        wingUL: dynAngle(state.spreadUL, true),
        wingUR: dynAngle(state.spreadUR, false),
        wingDL: dynAngle(state.spreadDL, true),
        wingDR: dynAngle(state.spreadDR, false),
    }
}

/// 父级局部坐标偏移（unit）：upper 用 (X-6, -(Y+10))/8，lower 用 (X, -Y)/8 再 y 减 upperHalfPx/8。
pub fn pieceLocalOffset(piece: WingPiece, isLower: bool, upperHeightPx: f32) -> (f32, f32) {
    let localPxX = if isLower { piece.xPx as f32 } else { piece.xPx as f32 - UPTORSO_OFFSET_X };
    let localPxY = if isLower { piece.yPx as f32 } else { piece.yPx as f32 - UPTORSO_OFFSET_Y };
    let lx = localPxX / PIXELS_PER_UNIT;
    let mut ly = -localPxY / PIXELS_PER_UNIT;
    if isLower {
        ly += -(upperHeightPx * 0.5) / PIXELS_PER_UNIT;
    }
    (lx, ly)
}

/// SkinApplier.AttachWing 中 baseAngleDeg = -piece.Rotation。
pub fn pieceBaseAngle(piece: WingPiece) -> f32 {
    -piece.rotationDeg
}
