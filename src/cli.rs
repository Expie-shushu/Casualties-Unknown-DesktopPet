// 桌宠命令行参数。--pet-id <id> 选目标配置；--config <path> 覆盖资源根；--system-font 临时切系统字体。
#![allow(non_snake_case)]

#[derive(Debug, Clone)]
pub struct CliOptions {
    pub petId: String,
    pub configRoot: Option<std::path::PathBuf>,
    pub forceSystemFont: bool,
}

const DEFAULT_PET_ID: &str = "default";

pub fn parseCli(args: Vec<String>) -> CliOptions {
    let mut opts = CliOptions {
        petId: DEFAULT_PET_ID.into(),
        configRoot: None,
        forceSystemFont: false,
    };
    let mut iter = args.iter().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--pet-id" | "--pet" => {
                if let Some(v) = iter.next() {
                    opts.petId = sanitize(v);
                }
            }
            "--config" => {
                if let Some(v) = iter.next() {
                    opts.configRoot = Some(std::path::PathBuf::from(v));
                }
            }
            "--system-font" => opts.forceSystemFont = true,
            _ if a.starts_with("--pet=") => opts.petId = sanitize(&a["--pet=".len()..]),
            _ if a.starts_with("--pet-id=") => opts.petId = sanitize(&a["--pet-id=".len()..]),
            _ if a.starts_with("--config=") => {
                opts.configRoot = Some(std::path::PathBuf::from(&a["--config=".len()..]));
            }
            _ => {}
        }
    }
    opts
}

fn sanitize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        }
    }
    if out.is_empty() {
        out.push_str(DEFAULT_PET_ID);
    }
    out
}
