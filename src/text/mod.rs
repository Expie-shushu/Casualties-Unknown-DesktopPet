// 文本子系统：cosmic-text + fontdb 字体加载 + glyph 栅格化 → wgpu 纹理。
#![allow(non_snake_case)]

pub mod system;
pub mod render;

pub use render::{rasterizeLine, rasterizeWrapped, RasterizedGlyph};
pub use system::{findFontFile, scanDirFamilies, FontSourceMode, TextSystem};
