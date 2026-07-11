// 编译期把 data/skin 与 desktopPet/ 资源复制到 exe 同目录（target/<profile>/）。
// 桌宠运行时仅从 current_exe 同目录读取，不依赖外部路径。
#![allow(non_snake_case)]

use std::path::{Path, PathBuf};

const RESOURCE_DIRS: &[&str] = &["data", "desktopPet"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    embedWindowsResource();
    let projectRoot = projectRoot();
    let outDir = match exeDir() {
        Some(d) => d,
        None => {
            println!("cargo:warning=cannot resolve target dir");
            return;
        }
    };
    for name in RESOURCE_DIRS {
        let src = projectRoot.join(name);
        if !src.exists() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", src.display());
        let dst = outDir.join(name);
        copyDirRecursive(&src, &dst);
    }
    createDesktopShortcut(&outDir);
}

/// release 构建时在桌面生成启动快捷方式，指向 target/release 下的 exe。
/// 快捷方式目标即使此刻 exe 尚未链接完成也可创建（指向固定路径，链接后即生效）。
#[cfg(windows)]
fn createDesktopShortcut(outDir: &Path) {
    // 仅 release 生成。
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }
    let exe = outDir.join("Casualties_Unknown_DesktopPet.exe");
    // 工作目录 = exe 所在目录（桌宠从 current_exe 同目录读资源，必须正确）。
    let workDir = outDir;
    // 用 PowerShell WScript.Shell 创建 .lnk；桌面路径用 GetFolderPath('Desktop') 取，
    // 自动处理桌面重定向（如桌面被移到 D 盘）。路径里的单引号转义为两个单引号。
    let ps = format!(
        "$desktop=[Environment]::GetFolderPath('Desktop');\
         $lnk=Join-Path $desktop 'CU DesktopPet.lnk';\
         $s=(New-Object -ComObject WScript.Shell).CreateShortcut($lnk);\
         $s.TargetPath='{exe}';\
         $s.WorkingDirectory='{work}';\
         $s.Save()",
        exe = exe.display().to_string().replace('\'', "''"),
        work = workDir.display().to_string().replace('\'', "''"),
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=已在桌面生成快捷方式 Casualties Unknown：desktopPet.lnk");
        }
        Ok(s) => println!("cargo:warning=创建快捷方式失败（退出码 {:?}）", s.code()),
        Err(e) => println!("cargo:warning=创建快捷方式失败: {e}"),
    }
}

#[cfg(not(windows))]
fn createDesktopShortcut(_outDir: &Path) {}

#[cfg(windows)]
fn embedWindowsResource() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let icon = Path::new(&manifest).join("icons").join("icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if !icon.exists() {
        println!("cargo:warning=icon.ico not found, skip resource");
        return;
    }
    let mut res = winres::WindowsResource::new();
    res.set_icon(icon.to_string_lossy().as_ref());
    res.set("ProductName", "Casualties Unknown：desktopPet");
    res.set("FileDescription", "Casualties Unknown：desktopPet");
    res.set("CompanyName", "huanxin996");
    res.set("LegalCopyright", "Copyright (c) 2026 huanxin996");
    res.set("OriginalFilename", "Casualties_Unknown_desktopPet.exe");
    if let Err(e) = res.compile() {
        println!("cargo:warning=winres compile failed: {e}");
    }
}

#[cfg(not(windows))]
fn embedWindowsResource() {}

fn projectRoot() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    if manifest.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(manifest)
    }
}

fn exeDir() -> Option<PathBuf> {
    let out = std::env::var("OUT_DIR").ok()?;
    Path::new(&out).ancestors().nth(3).map(|p| p.to_path_buf())
}

fn copyDirRecursive(src: &Path, dst: &Path) {
    if !src.is_dir() {
        return;
    }
    let _ = std::fs::create_dir_all(dst);
    let entries = match std::fs::read_dir(src) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copyDirRecursive(&path, &target);
        } else if shouldCopyFile(&path, &target) {
            let _ = std::fs::copy(&path, &target);
        }
    }
}

fn shouldCopyFile(src: &Path, dst: &Path) -> bool {
    if !dst.exists() {
        return true;
    }
    let srcMeta = match std::fs::metadata(src) {
        Ok(m) => m,
        Err(_) => return true,
    };
    let dstMeta = match std::fs::metadata(dst) {
        Ok(m) => m,
        Err(_) => return true,
    };
    if srcMeta.len() != dstMeta.len() {
        return true;
    }
    match (srcMeta.modified(), dstMeta.modified()) {
        (Ok(s), Ok(d)) => s > d,
        _ => true,
    }
}
