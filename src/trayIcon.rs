// 系统托盘：图标 + 状态 tooltip + 右键菜单（显示/隐藏 / 设置 / 退出）。
#![allow(non_snake_case)]

use anyhow::{anyhow, Result};
use muda::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub const ID_TRAY_SETTINGS: &str = "tray.settings";
pub const ID_TRAY_TOGGLE: &str = "tray.toggle";
pub const ID_TRAY_QUIT: &str = "tray.quit";

pub struct TrayHandle {
    pub icon: TrayIcon,
}

/// 创建系统托盘，菜单含显示/隐藏、设置、退出；返回句柄保留生命周期。
pub fn createTray(petId: &str) -> Result<TrayHandle> {
    let icon = loadTrayIcon()?;
    let menu = Menu::new();
    menu.append(&MenuItem::with_id(ID_TRAY_TOGGLE, "显示 / 隐藏", true, None))?;
    menu.append(&MenuItem::with_id(ID_TRAY_SETTINGS, "设置...", true, None))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(ID_TRAY_QUIT, "退出桌宠", true, None))?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(format!("Casualties Unknown：desktopPet [{petId}]"))
        .with_icon(icon)
        .with_title("Casualties Unknown：desktopPet")
        .build()
        .map_err(|e| anyhow!("tray build failed: {e}"))?;
    Ok(TrayHandle { icon: tray })
}

pub fn updateTooltip(handle: &TrayHandle, petId: &str, status: &str) {
    let _ = handle.icon.set_tooltip(Some(format!("Casualties Unknown：desktopPet [{petId}] · {status}")));
}

fn loadTrayIcon() -> Result<Icon> {
    const PNG: &[u8] = include_bytes!("../icons/icon.png");
    let img = image::load_from_memory(PNG)?.to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).map_err(|e| anyhow!("tray icon decode: {e}"))
}
