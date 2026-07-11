// 对话气泡：白底 + 黑色文字。配合 TextSystem 栅格化 glyph 后逐字 SpriteDraw。
#![allow(non_snake_case)]

use std::time::Instant;

use crate::asset::{SpriteAsset, SpriteFactory};
use crate::renderer::spritePipeline::buildSpriteMatrix;
use crate::renderer::SpriteDraw;
use crate::text::{rasterizeWrapped, RasterizedGlyph, TextSystem};

const PADDING: f32 = 10.0;
/// 气泡文字最大行宽（物理像素）：超出自动换行，避免长句超出 360 窗口被裁。
const MAX_TEXT_WIDTH: f32 = 300.0;

pub struct Bubble {
    pub text: String,
    pub expireAt: Instant,
    pub glyphAssets: Vec<(RasterizedGlyph, SpriteAsset)>,
    pub width: f32,
    pub height: f32,
}

pub fn say(
    factory: &SpriteFactory<'_>,
    ts: &mut TextSystem,
    text: &str,
    durationMs: u64,
    fontFamily: &str,
    fontSizePx: f32,
) -> Option<Bubble> {
    let glyphs = rasterizeWrapped(ts, text, fontSizePx, MAX_TEXT_WIDTH, fontFamily);
    if glyphs.is_empty() {
        return None;
    }
    let mut maxX: f32 = 0.0;
    let mut maxY: f32 = 0.0;
    for g in &glyphs {
        let rx = g.xPx + g.widthPx as f32;
        let ry = g.yPx + g.heightPx as f32;
        if rx > maxX {
            maxX = rx;
        }
        if ry > maxY {
            maxY = ry;
        }
    }
    let mut glyphAssets = Vec::with_capacity(glyphs.len());
    for g in glyphs {
        match factory.fromRgba(&g.rgba, g.widthPx, g.heightPx, "glyph") {
            Ok(a) => glyphAssets.push((g, a)),
            Err(e) => log::warn!("glyph upload failed: {e:?}"),
        }
    }
    Some(Bubble {
        text: text.to_string(),
        expireAt: Instant::now() + std::time::Duration::from_millis(durationMs),
        glyphAssets,
        width: maxX + PADDING * 2.0,
        height: maxY + PADDING * 2.0,
    })
}

/// 白底气泡：白色半透明底板 + 深色正文。`bgAlpha` 为底板不透明度（设置可调）。
/// `textColor` 和 `bgColor` 来自 BubbleStyle 设置。
/// 位置参数沿用旧约定（anchorScreenX 水平中心，anchorTopY 文字块顶部）。
pub fn appendBubbleDraws<'a>(
    bubble: &'a Bubble,
    bubbleBg: &'a SpriteAsset,
    anchorScreenX: f32,
    anchorTopY: f32,
    bgAlpha: f32,
    textColor: [f32; 4],
    bgColor: [f32; 3],
    screenW: f32,
    screenH: f32,
    out: &mut Vec<SpriteDraw<'a>>,
) {
    // ── 半透明底板（覆盖整个文字块，含内边距）──
    let panelCx = anchorScreenX;
    let panelCy = anchorTopY + bubble.height * 0.5;
    let m = buildSpriteMatrix(
        screenW, screenH, panelCx, panelCy, bubble.width, bubble.height, 0.0, 1.0,
    );
    out.push(SpriteDraw {
        asset: bubbleBg,
        matrix: m,
        color: [bgColor[0], bgColor[1], bgColor[2], bgAlpha],
        uvRect: [0.0, 0.0, 1.0, 1.0],
    });

    // ── 正文（逐字，整数像素对齐避免发糊）──
    for (g, a) in &bubble.glyphAssets {
        let gw = g.widthPx as f32;
        let gh = g.heightPx as f32;
        let leftX = (anchorScreenX - bubble.width * 0.5 + PADDING + g.xPx).round();
        let topY = (anchorTopY + PADDING + g.yPx).round();
        let cx = leftX + gw * 0.5;
        let cy = topY + gh * 0.5;
        let m = buildSpriteMatrix(screenW, screenH, cx, cy, gw, gh, 0.0, 1.0);
        out.push(SpriteDraw {
            asset: a,
            matrix: m,
            color: textColor,
            uvRect: [0.0, 0.0, 1.0, 1.0],
        });
    }
}
