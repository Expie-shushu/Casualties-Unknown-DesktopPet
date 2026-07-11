// 桌宠独立 exe 入口。命令行解析 → 启动 winit ApplicationHandler。
#![windows_subsystem = "windows"]
#![allow(non_snake_case)]

use pet_runtime::app::PetApp;
use pet_runtime::cli::parseCli;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let opts = parseCli(std::env::args().collect());
    if let Err(e) = pet_runtime::app::runApp(opts) {
        eprintln!("[pet-runtime] startup failed: {e}");
        std::process::exit(1);
    }
    let _ = std::any::type_name::<PetApp>();
}
