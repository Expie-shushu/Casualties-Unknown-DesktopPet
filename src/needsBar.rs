// 头顶状态面板：每行 = [标题(上)] + [图标 | 分段格进度条(圆角描边) | 数值%]。
// 三行（心情/饥饿/口渴）纵向堆叠在头顶上方。心情用爱心格，饥饿/口渴用方格。
// 图标/爱心为用户自备 PNG（desktopPet/needs/），缺图回退纯色方块；标题/数字/百分号用缓存字形。
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

use crate::asset::SpriteAsset;
use crate::needs::Needs;
use crate::renderer::spritePipeline::buildSpriteMatrix;
use crate::renderer::SpriteDraw;

/// 行内元素几何（屏幕像素，紧凑以适配 360px 窗口头顶有限空间）。
const ICON: f32 = 14.0;
/// 分段格数量与单格尺寸。
const CELLS: usize = 10;
const CELL_W: f32 = 6.5;
const CELL_H: f32 = 8.0;
const CELL_GAP: f32 = 1.5;
/// 图标 / 进度条 / 数值之间的水平间距。
const GAP: f32 = 4.0;
/// 数值预留宽度（约 3 位数 + 百分号）。
const VALUE_SLOT: f32 = 28.0;
/// 标题底边到进度条顶边的间距。
const TITLE_GAP: f32 = 0.5;
/// 行与行之间的额外纵向间距。
const ROW_GAP: f32 = 2.0;
/// 进度条描边相对格区域外扩的留白（含描边线宽）。
const BORDER_PAD: f32 = 1.5;
const BORDER_PX: f32 = 1.0;
/// 面板半透明底板的内边距与配色（叠在桌宠身上也清晰可读）。底板不透明度由调用方传入。
const PANEL_PAD_X: f32 = 6.0;
const PANEL_PAD_Y: f32 = 4.0;
const PANEL_BG_RGB: [f32; 3] = [0.08, 0.08, 0.11];

const FULL_UV: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// 把绘制中心对齐到整数像素左上角再回算中心：nearest 采样下让文字/数字 1:1 锐利不发糊。
fn snap(cx: f32, cy: f32, w: f32, h: f32) -> (f32, f32) {
    ((cx - w * 0.5).round() + w * 0.5, (cy - h * 0.5).round() + h * 0.5)
}

/// 空格（未填充）颜色。
const TRACK_COLOR: [f32; 3] = [0.22, 0.22, 0.26];
/// 描边色（浅灰白，近圆角边框）。
const BORDER_COLOR: [f32; 3] = [0.85, 0.85, 0.90];
/// 心情 / 饥饿 / 口渴填充色。
const MOOD_COLOR: [f32; 3] = [0.95, 0.45, 0.62];
const HUNGER_COLOR: [f32; 3] = [0.96, 0.62, 0.26];
const THIRST_COLOR: [f32; 3] = [0.36, 0.62, 0.96];
/// 数值文字色（近白）。
const TEXT_COLOR: [f32; 3] = [0.96, 0.96, 0.96];

/// 往 `out` 追加头顶状态面板的 draw。
///
/// - `needs`：当前数值（0~100）。
/// - `solid`：1×1 纯色（白）sprite，按 color 着色复用（格 / 描边 / 占位）。
/// - `icons`：心情/饥饿/口渴图标（None 则画彩色方块占位）。
/// - `titles`：心情值/饥饿值/口渴值标题字形（None 则不画标题）。
/// - `percent`：百分号字形（None 则数值后不画 %）。
/// - `heart`：爱心格 PNG（心情条用其形状染色；None 则心情条也用方格）。
/// - `digits`：0~9 数字字形缓存（None 则该位不绘制）。
/// - `alpha`：整体不透明度（淡入淡出）。
/// - `centerX`：面板水平中心屏幕 X（桌宠头部 X）。
/// - `groupBottomY`：面板底边屏幕 Y（应在头顶上方）；三行自此向上堆叠，越界则下移保护。
/// - `(screenW, screenH)`：客户区像素尺寸，用于 NDC 换算。
pub fn appendNeedsBarDraws<'a>(
    needs: &Needs,
    solid: &'a SpriteAsset,
    icons: [Option<&'a SpriteAsset>; 3],
    titles: [Option<&'a SpriteAsset>; 3],
    percent: Option<&'a SpriteAsset>,
    heart: Option<&'a SpriteAsset>,
    digits: &'a [Option<SpriteAsset>; 10],
    alpha: f32,
    panelBgAlpha: f32,
    centerX: f32,
    groupBottomY: f32,
    screenW: f32,
    screenH: f32,
    out: &mut Vec<SpriteDraw<'a>>,
) {
    if alpha <= 0.001 {
        return;
    }
    let rows = [
        (needs.mood, MOOD_COLOR, icons[0], titles[0], true),
        (needs.hunger, HUNGER_COLOR, icons[1], titles[1], false),
        (needs.thirst, THIRST_COLOR, icons[2], titles[2], false),
    ];

    let barW = CELLS as f32 * CELL_W + (CELLS as f32 - 1.0) * CELL_GAP;
    let rowW = ICON + GAP + barW + GAP + VALUE_SLOT;
    let leftX = centerX - rowW * 0.5;

    // 单行整体高度：标题块（约字号）+ 间距 + 进度条块（取图标/格较高者 + 描边留白）。
    let titleH = NEEDS_TITLE_ROW_H;
    let barRowH = ICON.max(CELL_H + BORDER_PAD * 2.0);
    let rowH = titleH + TITLE_GAP + barRowH;
    let groupH = rowH * rows.len() as f32 + ROW_GAP * (rows.len() as f32 - 1.0);
    // 顶部越界保护：头顶空间不足时整体下移（会叠到桌宠身上），留出 2px。
    let topY = (groupBottomY - groupH).max(2.0);

    // ── 半透明圆角底板：使面板叠在桌宠任意部位（如脸部）仍清晰可读 ──
    {
        let panelW = rowW + PANEL_PAD_X * 2.0;
        let panelH = groupH + PANEL_PAD_Y * 2.0;
        let panelCx = centerX;
        let panelCy = topY + groupH * 0.5;
        let m = buildSpriteMatrix(screenW, screenH, panelCx, panelCy, panelW, panelH, 0.0, 1.0);
        out.push(SpriteDraw {
            asset: solid,
            matrix: m,
            color: [PANEL_BG_RGB[0], PANEL_BG_RGB[1], PANEL_BG_RGB[2], panelBgAlpha * alpha],
            uvRect: FULL_UV,
        });
    }

    for (i, (value, fill, icon, title, isHeart)) in rows.iter().enumerate() {
        let rowTop = topY + (rowH + ROW_GAP) * i as f32;
        let barRowCy = rowTop + titleH + TITLE_GAP + barRowH * 0.5;

        // ── 标题（行顶，水平居中于整行）──
        if let Some(t) = title {
            let (tw, th) = (t.width as f32, t.height as f32);
            let (scx, scy) = snap(centerX, rowTop + titleH * 0.5, tw, th);
            let m = buildSpriteMatrix(screenW, screenH, scx, scy, tw, th, 0.0, 1.0);
            out.push(SpriteDraw {
                asset: t,
                matrix: m,
                color: [TEXT_COLOR[0], TEXT_COLOR[1], TEXT_COLOR[2], alpha],
                uvRect: FULL_UV,
            });
        }

        // ── 图标（左）：有 PNG 则等比贴合，否则彩色方块占位 ──
        let iconCx = leftX + ICON * 0.5;
        match icon {
            Some(a) => {
                let (iw, ih) = (a.width as f32, a.height as f32);
                let fit = (ICON / iw).min(ICON / ih);
                let m = buildSpriteMatrix(
                    screenW, screenH, iconCx, barRowCy, iw * fit, ih * fit, 0.0, 1.0,
                );
                out.push(SpriteDraw {
                    asset: a,
                    matrix: m,
                    color: [1.0, 1.0, 1.0, alpha],
                    uvRect: FULL_UV,
                });
            }
            None => {
                let m = buildSpriteMatrix(
                    screenW, screenH, iconCx, barRowCy, ICON * 0.8, ICON * 0.8, 0.0, 1.0,
                );
                out.push(SpriteDraw {
                    asset: solid,
                    matrix: m,
                    color: [fill[0], fill[1], fill[2], alpha],
                    uvRect: FULL_UV,
                });
            }
        }

        // ── 进度条（中）：圆角描边框 + 分段格填充 ──
        let barLeft = leftX + ICON + GAP;
        let ratio = (value / 100.0).clamp(0.0, 1.0);
        let filled = (ratio * CELLS as f32).round() as usize;
        // （已移除进度条外框：用户要求无边框）
        for c in 0..CELLS {
            let cellCx = barLeft + CELL_W * 0.5 + (CELL_W + CELL_GAP) * c as f32;
            let on = c < filled;
            let color = if on {
                [fill[0], fill[1], fill[2], alpha]
            } else {
                [TRACK_COLOR[0], TRACK_COLOR[1], TRACK_COLOR[2], alpha * 0.85]
            };
            // 心情行且有爱心 PNG：用爱心形状染色；否则纯色方格。
            let useHeart = *isHeart && heart.is_some();
            let asset: &SpriteAsset = if useHeart { heart.unwrap() } else { solid };
            let (dw, dh) = if useHeart {
                let h = heart.unwrap();
                let fit = (CELL_W / h.width as f32).min(CELL_H / h.height as f32);
                (h.width as f32 * fit, h.height as f32 * fit)
            } else {
                (CELL_W, CELL_H)
            };
            let m = buildSpriteMatrix(screenW, screenH, cellCx, barRowCy, dw, dh, 0.0, 1.0);
            out.push(SpriteDraw {
                asset,
                matrix: m,
                color,
                uvRect: FULL_UV,
            });
        }

        // ── 数值（右）：缓存数字字形逐位 + 百分号 ──
        let valueLeft = barLeft + barW + GAP;
        appendValue(
            value.round() as i32,
            digits,
            percent,
            valueLeft,
            barRowCy,
            alpha,
            screenW,
            screenH,
            out,
        );
    }
}

/// 标题行预留高度（与 NEEDS_TITLE_PX 同量级，留点行距）。
const NEEDS_TITLE_ROW_H: f32 = 13.0;

/// 画一圈“圆角”描边框（4 条边，水平边两端各内缩 1px、垂直边上下各内缩 1px 以伪造圆角）。
/// 框包住格区域（cx 居中于条，barW×barH 为格区域尺寸），外扩 BORDER_PAD。
fn appendRoundedBorder<'a>(
    solid: &'a SpriteAsset,
    barLeft: f32,
    cy: f32,
    barW: f32,
    barH: f32,
    alpha: f32,
    screenW: f32,
    screenH: f32,
    out: &mut Vec<SpriteDraw<'a>>,
) {
    let cx = barLeft + barW * 0.5;
    let fw = barW + BORDER_PAD * 2.0;
    let fh = barH + BORDER_PAD * 2.0;
    let col = [BORDER_COLOR[0], BORDER_COLOR[1], BORDER_COLOR[2], alpha];
    let r = 1.0; // 圆角内缩量
    // 上 / 下 边（两端内缩 r）。
    for sy in [cy - fh * 0.5 + BORDER_PX * 0.5, cy + fh * 0.5 - BORDER_PX * 0.5] {
        let m = buildSpriteMatrix(screenW, screenH, cx, sy, fw - r * 2.0, BORDER_PX, 0.0, 1.0);
        out.push(SpriteDraw { asset: solid, matrix: m, color: col, uvRect: FULL_UV });
    }
    // 左 / 右 边（上下内缩 r）。
    for sx in [cx - fw * 0.5 + BORDER_PX * 0.5, cx + fw * 0.5 - BORDER_PX * 0.5] {
        let m = buildSpriteMatrix(screenW, screenH, sx, cy, BORDER_PX, fh - r * 2.0, 0.0, 1.0);
        out.push(SpriteDraw { asset: solid, matrix: m, color: col, uvRect: FULL_UV });
    }
}

/// 整数 value 拆位用缓存字形从 startX 向右逐位绘制，末尾补百分号，垂直居中于 cy。
fn appendValue<'a>(
    value: i32,
    digits: &'a [Option<SpriteAsset>; 10],
    percent: Option<&'a SpriteAsset>,
    startX: f32,
    cy: f32,
    alpha: f32,
    screenW: f32,
    screenH: f32,
    out: &mut Vec<SpriteDraw<'a>>,
) {
    let v = value.clamp(0, 999);
    let mut x = startX;
    for ch in v.to_string().chars() {
        let d = (ch as u8 - b'0') as usize;
        if let Some(a) = digits.get(d).and_then(|o| o.as_ref()) {
            let (dw, dh) = (a.width as f32, a.height as f32);
            let (scx, scy) = snap(x + dw * 0.5, cy, dw, dh);
            let m = buildSpriteMatrix(screenW, screenH, scx, scy, dw, dh, 0.0, 1.0);
            out.push(SpriteDraw {
                asset: a,
                matrix: m,
                color: [TEXT_COLOR[0], TEXT_COLOR[1], TEXT_COLOR[2], alpha],
                uvRect: FULL_UV,
            });
            x += dw + 1.0;
        }
    }
    if let Some(p) = percent {
        let (pw, ph) = (p.width as f32, p.height as f32);
        let (scx, scy) = snap(x + pw * 0.5, cy, pw, ph);
        let m = buildSpriteMatrix(screenW, screenH, scx, scy, pw, ph, 0.0, 1.0);
        out.push(SpriteDraw {
            asset: p,
            matrix: m,
            color: [TEXT_COLOR[0], TEXT_COLOR[1], TEXT_COLOR[2], alpha],
            uvRect: FULL_UV,
        });
    }
}
