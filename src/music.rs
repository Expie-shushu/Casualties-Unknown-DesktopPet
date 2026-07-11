// 音乐模块：文件扫描 + 音频播放（rodio）。
#![allow(non_snake_case)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

/// 支持的音频扩展名（小写）。
const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg", "aac", "m4a"];

/// 单个音乐文件信息。
#[derive(Clone, Debug)]
pub struct MusicFile {
    /// 文件绝对路径。
    pub path: PathBuf,
    /// 含扩展名的文件名（如 "song.mp3"）。
    pub filename: String,
    /// 文件名 stem（如 "song"），用作显示名。
    pub stem: String,
}

// ── 文件扫描 ──────────────────────────────────────────

/// 递归扫描 musicDir，返回所有支持的音频文件列表。
/// 目录不存在或为空时返回空 Vec。
pub fn scanMusicFiles(musicDir: &Path) -> Vec<MusicFile> {
    let mut files = Vec::new();
    if musicDir.exists() {
        walkMusicDir(musicDir, &mut files);
    }
    files
}

fn walkMusicDir(dir: &Path, out: &mut Vec<MusicFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walkMusicDir(&p, out);
            continue;
        }
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let filename = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&filename)
            .to_string();
        out.push(MusicFile {
            path: p,
            filename,
            stem,
        });
    }
}

// ── 播放模式 ──────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayMode {
    /// 按用户排列顺序播放全部后停止。
    Sequential,
    /// 列表循环：播放全部后回到开头继续。
    ListLoop,
    /// 单曲循环：只重复当前一首。
    SingleLoop,
}

// ── 音频播放器 ────────────────────────────────────────

/// 音乐播放器：封装 rodio 的 OutputStream + Sink 队列。
pub struct MusicPlayer {
    /// 必须持有 _stream，释放后音频设备断开。
    _stream: OutputStream,
    _handle: OutputStreamHandle,
    sink: Sink,
    /// 当前音量（0.0~1.0），跨 sink 重建保持。
    volume: f32,
    /// 播放模式。
    play_mode: PlayMode,
    /// 本次 start() 入队的曲目数。
    tracks_enqueued: usize,
    /// 本次入队在 current_order 中的起始索引（start()=0，play_from()=display_idx）。
    queue_start_index: usize,
    /// 本次 start() 入队的曲目 stem，与 play_order 对齐。
    play_order: Vec<String>,
    /// 每首歌的总时长（与 play_order 对齐），None 表示不支持时长查询。
    track_durations: Vec<Option<Duration>>,
    /// 用户可调整的播放顺序（files 索引数组）。
    current_order: Vec<usize>,
    /// 播放模式被切换后置 true，下一帧 sync_mode_if_needed 重新入队。
    needs_mode_sync: bool,
}

impl MusicPlayer {
    /// 创建播放器，打开默认音频输出设备。
    pub fn try_new() -> Result<Self, String> {
        let (stream, handle) =
            OutputStream::try_default().map_err(|e| format!("无法打开音频设备: {}", e))?;
        let sink = Sink::try_new(&handle)
            .map_err(|e| format!("无法创建音频队列: {}", e))?;
        let vol = 0.5;
        sink.set_volume(vol);
        Ok(Self {
            _stream: stream,
            _handle: handle,
            sink,
            volume: vol,
            play_mode: PlayMode::ListLoop,
            tracks_enqueued: 0,
            queue_start_index: 0,
            play_order: Vec::new(),
            track_durations: Vec::new(),
            current_order: Vec::new(),
            needs_mode_sync: false,
        })
    }

    /// 开始播放：新建 Sink，按播放模式排列文件后全部加入队列。
    pub fn start(&mut self, files: &[MusicFile]) {
        let sink = match Sink::try_new(&self._handle) {
            Ok(s) => s,
            Err(e) => {
                log::error!("music: failed to create sink: {}", e);
                return;
            }
        };
        sink.set_volume(self.volume);
        self.sink = sink;
        self.tracks_enqueued = 0;
        self.queue_start_index = 0;
        self.play_order.clear();
        self.track_durations.clear();

        if files.is_empty() {
            return;
        }

        // 确定 / 刷新播放顺序
        if self.current_order.len() != files.len() {
            // 文件列表变了（增减文件），重建默认顺序。
            self.current_order = (0..files.len()).collect();
        }
        let indices: Vec<usize> = match self.play_mode {
            PlayMode::Sequential | PlayMode::ListLoop => self.current_order.clone(),
            PlayMode::SingleLoop => {
                // 只取第一首重复播放
                self.current_order.first().copied().into_iter().collect()
            }
        };

        let mut queued = 0u32;
        for &i in &indices {
            // 预解码获取总时长
            let dur = match std::fs::File::open(&files[i].path) {
                Ok(file) => {
                    let reader = std::io::BufReader::new(file);
                    match Decoder::new(reader) {
                        Ok(source) => {
                            let d = source.total_duration();
                            self.sink.append(source);
                            queued += 1;
                            d
                        }
                        Err(e) => {
                            log::warn!(
                                "跳过无法解码的音频 {}: {}",
                                files[i].filename,
                                e
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    log::warn!("无法打开音频文件 {}: {}", files[i].filename, e);
                    None
                }
            };
            self.play_order.push(files[i].stem.clone());
            self.track_durations.push(dur);
        }

        self.tracks_enqueued = queued as usize;
        log::info!(
            "music: start playing {} tracks ({} queued), mode={:?}",
            files.len(),
            queued,
            self.play_mode
        );
    }

    /// 停止播放并清空队列（保留 current_order）。
    pub fn stop(&mut self) {
        self.sink = Sink::try_new(&self._handle).unwrap_or_else(|e| {
            log::error!("music: failed to recreate sink on stop: {}", e);
            panic!("music: sink recreate failed: {}", e);
        });
        self.sink.set_volume(self.volume);
        self.tracks_enqueued = 0;
        self.queue_start_index = 0;
        self.play_order.clear();
        self.track_durations.clear();
        log::info!("music: stopped");
    }

    /// 播放队列是否已空（所有歌曲播完）。
    pub fn is_finished(&self) -> bool {
        self.sink.empty()
    }

    // ── 进度 / 曲目信息 ──────────────────────────────

    /// 当前曲目已播放时长。
    pub fn elapsed(&self) -> Duration {
        self.sink.get_pos()
    }

    /// 当前曲目总时长（预缓存的 `Decoder::total_duration()`）。
    pub fn current_duration(&self) -> Option<Duration> {
        let idx = self.current_track_sub_index()?;
        self.track_durations.get(idx).copied().flatten()
    }

    /// 返回 `(0-based 绝对索引, 总曲目数, 曲目名)`，无曲目时返回 None。
    pub fn current_track_info(&self) -> Option<(usize, usize, &str)> {
        let abs_idx = self.current_track_index()?;
        let total = self.current_order.len();
        if total == 0 || self.tracks_enqueued == 0 {
            return None;
        }
        let sub_idx = self.current_track_sub_index()?;
        let name = self.play_order.get(sub_idx).map(|s| s.as_str()).unwrap_or("?");
        Some((abs_idx, total, name))
    }

    /// 当前曲目在入队子集中的 0-based 索引（用于 play_order / track_durations 查表）。
    fn current_track_sub_index(&self) -> Option<usize> {
        let remaining = self.sink.len();
        if remaining == 0 || self.tracks_enqueued == 0 {
            return None;
        }
        Some(self.tracks_enqueued.saturating_sub(remaining))
    }

    /// 当前曲目在 current_order 中的绝对 0-based 索引（用于 UI 歌单高亮）。
    fn current_track_index(&self) -> Option<usize> {
        let sub = self.current_track_sub_index()?;
        Some(self.queue_start_index + sub)
    }

    /// 队列中剩余曲目数（含当前播放的）。
    pub fn tracks_remaining(&self) -> usize {
        self.sink.len()
    }

    /// 跳转到当前曲目的指定位置。
    pub fn seek(&self, pos: Duration) {
        if let Err(e) = self.sink.try_seek(pos) {
            log::warn!("music: seek failed: {:?}", e);
        }
    }

    /// 跳到下一首（用户主动操作，优先级高于播放模式）。
    pub fn next_track(&mut self, files: &[MusicFile]) {
        let cur = self.current_track_index().unwrap_or(0);
        let next = if cur + 1 < self.current_order.len() {
            cur + 1
        } else {
            0 // 最后一首 → 回到第一首
        };
        self.play_from(files, next);
    }

    /// 切换到上一首（若已播放超过 2 秒则重播当前首，否则跳到上一首）。
    /// 用户主动操作，优先级高于播放模式。
    pub fn prev_track(&mut self, files: &[MusicFile]) {
        let cur = self.current_track_index();
        // 已播放 > 2 秒 → 重播当前首；否则跳到上一首
        let target = if self.elapsed().as_secs() > 2 {
            cur.unwrap_or(0)
        } else {
            cur.map_or(0, |c| if c > 0 { c - 1 } else { 0 })
        };
        self.play_from(files, target);
    }

    // ── 音量 ──────────────────────────────────────────

    pub fn volume(&self) -> f32 {
        self.sink.volume()
    }

    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        self.sink.set_volume(self.volume);
    }

    // ── 播放模式 ──────────────────────────────────────

    pub fn play_mode(&self) -> PlayMode {
        self.play_mode
    }

    pub fn set_play_mode(&mut self, mode: PlayMode) {
        if self.play_mode != mode {
            self.play_mode = mode;
            self.needs_mode_sync = true;
        }
    }

    /// 播放模式切换后，按新模式重新入队当前曲目。
    /// 应在主循环每帧调用（紧邻 restart_if_loop_finished）。
    pub fn sync_mode_if_needed(&mut self, files: &[MusicFile]) {
        if !self.needs_mode_sync {
            return;
        }
        self.needs_mode_sync = false;
        if self.tracks_enqueued == 0 || files.is_empty() {
            return;
        }
        if let Some(idx) = self.current_track_index() {
            log::info!(
                "music: mode changed to {:?}, re-queue from track {}",
                self.play_mode,
                idx + 1
            );
            self.play_from(files, idx);
        }
    }

    // ── 暂停 / 恢复 ───────────────────────────────────

    pub fn is_paused(&self) -> bool {
        self.sink.is_paused()
    }

    pub fn toggle_pause(&self) {
        if self.sink.is_paused() {
            self.sink.play();
        } else {
            self.sink.pause();
        }
    }

    // ── 播放速度 ──────────────────────────────────────

    pub fn speed(&self) -> f32 {
        self.sink.speed()
    }

    pub fn set_speed(&self, s: f32) {
        self.sink.set_speed(s.clamp(0.25, 4.0));
    }

    // ── 歌单排序 ──────────────────────────────────────

    /// 返回当前播放顺序（files 索引快照）。
    pub fn order_snapshot(&self) -> &[usize] {
        &self.current_order
    }

    /// 将 idx 位置的曲目上移一位。
    pub fn move_up(&mut self, idx: usize) {
        if idx > 0 && idx < self.current_order.len() {
            self.current_order.swap(idx, idx - 1);
        }
    }

    /// 将 idx 位置的曲目下移一位。
    pub fn move_down(&mut self, idx: usize) {
        if idx + 1 < self.current_order.len() {
            self.current_order.swap(idx, idx + 1);
        }
    }

    // ── 从指定曲目开始播放 ────────────────────────────

    /// 从 current_order 的第 display_idx 首开始播放。
    /// 用户主动操作（点击曲目 / 上一首 / 下一首），不受播放模式限制，
    /// 始终从指定位置入队全部后续曲目。
    pub fn play_from(&mut self, files: &[MusicFile], display_idx: usize) {
        if display_idx >= self.current_order.len() {
            return;
        }
        let sink = match Sink::try_new(&self._handle) {
            Ok(s) => s,
            Err(e) => {
                log::error!("music: failed to create sink: {}", e);
                return;
            }
        };
        sink.set_volume(self.volume);
        self.sink = sink;
        self.tracks_enqueued = 0;
        self.queue_start_index = display_idx;
        self.play_order.clear();
        self.track_durations.clear();

        // 用户主动操作：根据播放模式决定入队范围
        let indices: Vec<usize> = match self.play_mode {
            PlayMode::SingleLoop => {
                // 单曲循环：只入队当前选中的一首，播完自动循环
                vec![self.current_order[display_idx]]
            }
            _ => {
                // 顺序 / 列表循环：入队从目标位置到末尾的全部曲目
                self.current_order[display_idx..].to_vec()
            }
        };
        let mut queued = 0u32;
        for &i in &indices {
            let dur = match std::fs::File::open(&files[i].path) {
                Ok(file) => {
                    let reader = std::io::BufReader::new(file);
                    match Decoder::new(reader) {
                        Ok(source) => {
                            let d = source.total_duration();
                            self.sink.append(source);
                            queued += 1;
                            d
                        }
                        Err(e) => {
                            log::warn!("跳过无法解码的音频 {}: {}", files[i].filename, e);
                            None
                        }
                    }
                }
                Err(e) => {
                    log::warn!("无法打开音频文件 {}: {}", files[i].filename, e);
                    None
                }
            };
            self.play_order.push(files[i].stem.clone());
            self.track_durations.push(dur);
        }
        self.tracks_enqueued = queued as usize;
        log::info!(
            "music: play_from track {}/{} ({} queued)",
            display_idx + 1,
            self.current_order.len(),
            queued
        );
    }

    // ── 循环 ──────────────────────────────────────────

    /// 根据播放模式，在队列播完后自动续播。
    pub fn restart_if_loop_finished(&mut self, files: &[MusicFile]) {
        if self.sink.empty() && self.tracks_enqueued > 0 {
            match self.play_mode {
                PlayMode::ListLoop => {
                    log::info!("music: list loop restart");
                    self.start(files);
                }
                PlayMode::SingleLoop => {
                    // 重播刚才那首，而非总是第一首
                    let last_idx = (self.queue_start_index + self.tracks_enqueued).saturating_sub(1);
                    if last_idx < self.current_order.len() {
                        log::info!("music: single loop restart track {}", last_idx + 1);
                        self.play_from(files, last_idx);
                    }
                }
                PlayMode::Sequential => {
                    // 顺序播放，播完就停止，不续播。
                }
            }
        }
    }
}
