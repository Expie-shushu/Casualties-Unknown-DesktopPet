// JS 插件主机：rquickjs 加载 main.js + 注入 KiroPet API + mtime 热重载 + 命令队列。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rquickjs::{Context, Function, Runtime};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum PluginCmd {
    #[serde(rename = "playMotion")]
    PlayMotion {
        name: String,
        #[serde(default = "defaultDuration")]
        durationMs: u64,
    },
    #[serde(rename = "say")]
    Say {
        text: String,
        #[serde(default = "defaultDuration")]
        durationMs: u64,
    },
}

fn defaultDuration() -> u64 {
    1500
}

#[allow(dead_code)]
pub struct LoadedPlugin {
    #[allow(dead_code)]
    pluginPath: PathBuf,
    mtimeMs: u64,
    #[allow(dead_code)]
    context: Context,
}

pub struct PluginHost {
    pub runtime: Runtime,
    plugins: HashMap<String, LoadedPlugin>,
    pub cmdQueue: Arc<Mutex<Vec<PluginCmd>>>,
    pub lastScanAt: Option<Instant>,
}

impl PluginHost {
    pub fn new() -> anyhow::Result<Self> {
        let runtime = Runtime::new()?;
        Ok(Self {
            runtime,
            plugins: HashMap::new(),
            cmdQueue: Arc::new(Mutex::new(Vec::new())),
            lastScanAt: None,
        })
    }

    pub fn tick(&mut self, pluginsDir: &Path) {
        let now = Instant::now();
        if let Some(prev) = self.lastScanAt {
            if (now - prev).as_millis() < 1500 {
                return;
            }
        }
        self.lastScanAt = Some(now);
        if !pluginsDir.exists() {
            return;
        }
        let entries = match std::fs::read_dir(pluginsDir) {
            Ok(it) => it,
            Err(_) => return,
        };
        let mut seen = Vec::new();
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let main = dir.join("main.js");
            if !main.exists() {
                continue;
            }
            let name = match dir.file_name().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            seen.push(name.clone());
            let mtimeMs = std::fs::metadata(&main)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            if let Some(cur) = self.plugins.get(&name) {
                if cur.mtimeMs == mtimeMs {
                    continue;
                }
            }
            self.plugins.remove(&name);
            match self.loadAndSetup(&main, &name, mtimeMs) {
                Ok(loaded) => {
                    self.plugins.insert(name, loaded);
                }
                Err(e) => log::warn!("plugin {name} load failed: {e:?}"),
            }
        }
        self.plugins.retain(|n, _| seen.iter().any(|s| s == n));
    }

    pub fn drainCommands(&self) -> Vec<PluginCmd> {
        let mut q = self.cmdQueue.lock().unwrap();
        std::mem::take(&mut *q)
    }

    fn loadAndSetup(&self, mainPath: &Path, name: &str, mtimeMs: u64) -> anyhow::Result<LoadedPlugin> {
        let code = std::fs::read_to_string(mainPath)?;
        let context = Context::full(&self.runtime)?;
        let queue = self.cmdQueue.clone();
        let pluginName = name.to_string();
        let result: rquickjs::Result<()> = context.with(|ctx| {
            let queue_log = pluginName.clone();
            let logInfo = Function::new(ctx.clone(), move |msg: String| {
                log::info!("[plugin:{queue_log}] {msg}");
            })?;
            let queue_warn = pluginName.clone();
            let logWarn = Function::new(ctx.clone(), move |msg: String| {
                log::warn!("[plugin:{queue_warn}] {msg}");
            })?;
            let queue_clone = queue.clone();
            let pushCmd = Function::new(ctx.clone(), move |json: String| {
                match serde_json::from_str::<PluginCmd>(&json) {
                    Ok(cmd) => queue_clone.lock().unwrap().push(cmd),
                    Err(e) => log::warn!("plugin pushCmd parse failed: {e}"),
                }
            })?;
            let api = rquickjs::Object::new(ctx.clone())?;
            api.set("logInfo", logInfo)?;
            api.set("logWarn", logWarn)?;
            api.set("pushCmd", pushCmd)?;
            ctx.globals().set("KiroPet", api)?;
            ctx.eval::<(), _>(code.as_bytes())?;
            if let Ok(setup) = ctx.globals().get::<_, Function>("setup") {
                let _ = setup.call::<_, ()>(());
            }
            Ok(())
        });
        result?;
        Ok(LoadedPlugin {
            pluginPath: mainPath.to_path_buf(),
            mtimeMs,
            context,
        })
    }
}
