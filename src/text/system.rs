// TextSystem：FontSystem + SwashCache + 字体来源策略。
#![allow(non_snake_case)]

use std::path::Path;

use cosmic_text::fontdb;
use cosmic_text::{FontSystem, SwashCache};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSourceMode {
    Embedded,
    System,
    Both,
}

pub struct TextSystem {
    pub fontSystem: FontSystem,
    pub swashCache: SwashCache,
}

impl TextSystem {
    pub fn new(mode: FontSourceMode, projectFontDir: Option<&Path>) -> Self {
        let mut db = fontdb::Database::new();

        let loadEmbedded = matches!(mode, FontSourceMode::Embedded | FontSourceMode::Both);
        if loadEmbedded {
            if let Some(dir) = projectFontDir {
                db.load_fonts_dir(dir);
            }
        }

        let loadSystem = matches!(mode, FontSourceMode::System | FontSourceMode::Both);
        if loadSystem {
            db.load_system_fonts();
        }

        let fontSystem = FontSystem::new_with_locale_and_db("zh-CN".into(), db);
        Self {
            fontSystem,
            swashCache: SwashCache::new(),
        }
    }

}

/// 扫描指定目录中的字体文件，返回所有字体族名（去重排序），供设置 UI 下拉框展示。
/// 仅扫描用户放入 desktopPet/fonts/ 的字体，不含系统字体（系统字体通过手动输入族名使用）。
pub fn scanDirFamilies(dir: &Path) -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_fonts_dir(dir);
    let mut families: Vec<String> = db
        .faces()
        .flat_map(|f| f.families.iter().map(|t| t.0.clone()))
        .collect();
    families.sort();
    families.dedup();
    families
}

/// 在 fontDir 中查找匹配 family 名称的字体文件路径，找不到返回 None。
pub fn findFontFile(family: &str, dir: &Path) -> Option<std::path::PathBuf> {
    let mut db = fontdb::Database::new();
    db.load_fonts_dir(dir);
    for face in db.faces() {
        if face.families.iter().any(|(name, _)| name == family) {
            if let fontdb::Source::File(ref path) = face.source {
                return Some(path.to_path_buf());
            }
        }
    }
    None
}
