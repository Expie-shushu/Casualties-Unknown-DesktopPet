// 右键菜单：动作 / 缩放 / 游戏... / 设置 / 关闭。DPI/字体/穿透/关于 移到 GUI 设置面板。
#![allow(non_snake_case)]

use anyhow::Result;
use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

use crate::input::PenetrationMode;
use crate::motion::ALL_MOTION_NAMES;
use crate::settings::{DpiMode, FontModeKey, PenetrationKey};

pub const ID_SCALE_PREFIX: &str = "scale.";
pub const ID_SETTINGS: &str = "settings.open";
pub const ID_INVENTORY: &str = "inventory.open";
pub const ID_CLOSE: &str = "close";
/// 石头剪刀布游戏窗口入口。
pub const ID_GAME: &str = "game.open";
/// 抽奖转盘窗口入口。
pub const ID_WHEEL: &str = "wheel.open";
/// 切换音乐播放状态。
pub const ID_TOGGLE_MUSIC: &str = "music.toggle";
/// 音乐播放器窗口入口。
pub const ID_MUSIC_PLAYER: &str = "musicPlayer.open";

const SCALES: &[f32] = &[1.0, 2.0, 3.0, 4.0, 6.0, 8.0];

pub fn buildMenu() -> Result<Menu> {
    let menu = Menu::new();

    let scaleMenu = Submenu::new("缩放", true);
    for s in SCALES {
        let id = format!("{}{}", ID_SCALE_PREFIX, *s);
        let label = format!("{}×", *s);
        scaleMenu.append(&MenuItem::with_id(id, label, true, None))?;
    }
    menu.append(&scaleMenu)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(ID_INVENTORY, "仓库/背包", true, None))?;
    menu.append(&MenuItem::with_id(ID_GAME, "与exp猜拳", true, None))?;
    menu.append(&MenuItem::with_id(ID_WHEEL, "抽奖轮盘", true, None))?;
    menu.append(&MenuItem::with_id(ID_TOGGLE_MUSIC, "听/停音乐", true, None))?;
    menu.append(&MenuItem::with_id(ID_MUSIC_PLAYER, "音乐播放器", true, None))?;
    menu.append(&MenuItem::with_id(ID_SETTINGS, "设置", true, None))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(ID_CLOSE, "关闭桌宠", true, None))?;
    Ok(menu)
}

pub enum MenuAction {
    PickMotion(String),
    SetScale(f32),
    SetDpi(DpiMode),
    SetFont(FontModeKey),
    SetPenetration(PenetrationMode, PenetrationKey),
    OpenSettings,
    OpenInventory,
    Close,
    /// 打开石头剪刀布游戏窗口。
    OpenGame,
    /// 打开抽奖转盘窗口。
    OpenWheel,
    /// 切换音乐播放状态。
    ToggleMusic,
    /// 打开音乐播放器窗口。
    OpenMusicPlayer,
}

pub fn parseMenuId(id: &str) -> Option<MenuAction> {
    if ALL_MOTION_NAMES.contains(&id) {
        return Some(MenuAction::PickMotion(id.to_string()));
    }
    if let Some(rest) = id.strip_prefix(ID_SCALE_PREFIX) {
        if let Ok(v) = rest.parse::<f32>() {
            return Some(MenuAction::SetScale(v));
        }
    }
    match id {
        ID_SETTINGS => Some(MenuAction::OpenSettings),
        ID_INVENTORY => Some(MenuAction::OpenInventory),
        ID_CLOSE => Some(MenuAction::Close),
        ID_GAME => Some(MenuAction::OpenGame),
        ID_WHEEL => Some(MenuAction::OpenWheel),
        ID_TOGGLE_MUSIC => Some(MenuAction::ToggleMusic),
        ID_MUSIC_PLAYER => Some(MenuAction::OpenMusicPlayer),
        _ => None,
    }
}

/// 模态菜单循环期间的重绘定时器 id（任意非零值即可）。
#[cfg(windows)]
const MENU_REDRAW_TIMER_ID: usize = 0xBEEF;

/// 定时器回调：在 TrackPopupMenu 的模态消息循环里被派发，令桌宠窗口失效产生 WM_PAINT，
/// winit 据此在模态循环内派发 RedrawRequested → 桌宠动画不冻结。hwnd 由系统作为首参传入。
#[cfg(windows)]
unsafe extern "system" fn menuRedrawTimerProc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    _msg: u32,
    _id: usize,
    _time: u32,
) {
    use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
    InvalidateRect(hwnd, std::ptr::null(), 0);
}

#[cfg(windows)]
pub fn showContextMenu(menu: &Menu, window: &winit::window::Window) -> Result<()> {
    use muda::ContextMenu;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

    let handle = window.window_handle()?;
    if let RawWindowHandle::Win32(h) = handle.as_raw() {
        let hwndIsize = h.hwnd.get();
        let hwndPtr = hwndIsize as windows_sys::Win32::Foundation::HWND;
        // 菜单弹出是阻塞式模态循环；用 ~60fps 定时器持续触发桌宠重绘，期间动画照常。
        unsafe {
            SetTimer(hwndPtr, MENU_REDRAW_TIMER_ID, 16, Some(menuRedrawTimerProc));
        }
        menu.show_context_menu_for_hwnd(hwndIsize, None);
        unsafe {
            KillTimer(hwndPtr, MENU_REDRAW_TIMER_ID);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn showContextMenu(_menu: &Menu, _window: &winit::window::Window) -> Result<()> {
    Ok(())
}
