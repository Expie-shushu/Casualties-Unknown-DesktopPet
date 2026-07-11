// 单行文本栅格化：layout + glyph 像素图，用于上传 wgpu 纹理 + 单字 quad 绘制。
#![allow(non_snake_case)]

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, SwashContent, Wrap};

use super::system::TextSystem;

pub struct RasterizedGlyph {
    pub xPx: f32,
    pub yPx: f32,
    pub widthPx: u32,
    pub heightPx: u32,
    pub rgba: Vec<u8>,
}

/// 字体族名 → cosmic-text Family。空串回退 SansSerif（系统默认无衬线）。
fn familyOf(name: &str) -> Family<'_> {
    if name.is_empty() {
        Family::SansSerif
    } else {
        Family::Name(name)
    }
}

/// 单行栅格化：极宽不换行（数字/标题等短文本用）。`family` 空串=默认字体。
pub fn rasterizeLine(
    ts: &mut TextSystem,
    text: &str,
    sizePx: f32,
    family: &str,
) -> Vec<RasterizedGlyph> {
    rasterizeWidth(ts, text, sizePx, 4096.0, sizePx * 4.0, family)
}

/// 限宽栅格化：超过 maxWidthPx 自动换行（气泡长文本用）。多行布局，glyph.yPx 含行偏移。
pub fn rasterizeWrapped(
    ts: &mut TextSystem,
    text: &str,
    sizePx: f32,
    maxWidthPx: f32,
    family: &str,
) -> Vec<RasterizedGlyph> {
    // 高度给足（约 12 行余量），避免长文本被 shape_until_scroll 截断。
    rasterizeWidth(ts, text, sizePx, maxWidthPx, sizePx * 16.0, family)
}

fn rasterizeWidth(
    ts: &mut TextSystem,
    text: &str,
    sizePx: f32,
    maxWidthPx: f32,
    maxHeightPx: f32,
    family: &str,
) -> Vec<RasterizedGlyph> {
    let metrics = Metrics::new(sizePx, sizePx * 1.25);
    let mut buffer = Buffer::new(&mut ts.fontSystem, metrics);
    buffer.set_size(&mut ts.fontSystem, Some(maxWidthPx), Some(maxHeightPx));
    let attrs = Attrs::new().family(familyOf(family));
    buffer.set_text(&mut ts.fontSystem, text, attrs, Shaping::Advanced);
    // CJK 无空格：默认 Wrap::Word 不会在汉字间断行 → 超宽被裁。Wrap::Glyph 强制按字形断行。
    // 必须在 set_text 之后、shape 之前设置才生效。
    buffer.set_wrap(&mut ts.fontSystem, Wrap::Glyph);
    buffer.shape_until_scroll(&mut ts.fontSystem, false);

    let mut glyphs = Vec::new();
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            let img = match ts.swashCache.get_image(&mut ts.fontSystem, physical.cache_key) {
                Some(i) => i,
                None => continue,
            };
            let w = img.placement.width;
            let h = img.placement.height;
            if w == 0 || h == 0 {
                continue;
            }
            let baseX = physical.x as f32 + img.placement.left as f32;
            let baseY = run.line_y + physical.y as f32 - img.placement.top as f32;
            let rgba = match img.content {
                SwashContent::Mask => maskToRgba(&img.data),
                SwashContent::Color => img.data.clone(),
                SwashContent::SubpixelMask => maskToRgba(&img.data),
            };
            glyphs.push(RasterizedGlyph {
                xPx: baseX,
                yPx: baseY,
                widthPx: w,
                heightPx: h,
                rgba,
            });
        }
    }
    glyphs
}

fn maskToRgba(mask: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(mask.len() * 4);
    for &a in mask {
        out.push(255);
        out.push(255);
        out.push(255);
        out.push(a);
    }
    out
}
