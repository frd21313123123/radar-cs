//! GSI confirms the local player's kills. Audio runs in the radar process,
//! independently of browser visibility and autoplay restrictions.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, sync::Mutex, time::{Duration, Instant}};

pub const MAX_MP3_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    enabled: bool,
    volume: u16,
    name: String,
    slot: u8,
}
impl Default for Settings {
    fn default() -> Self { Self { enabled: false, volume: 70, name: String::new(), slot: 0 } }
}

#[derive(Default)]
struct KillTracker {
    baseline: Option<(String, String, u64)>,
    timestamp: Option<f64>,
    received: Option<Instant>,
}
impl KillTracker {
    fn update(&mut self, value: &Value, now: Instant) -> bool {
        let timestamp = value["provider"]["timestamp"].as_f64().filter(|n| n.is_finite());
        // Provider time can repeat within a second; the kill counter deduplicates
        // those updates. Ignore older requests delivered by another HTTP worker.
        if matches!((self.timestamp, timestamp), (Some(a), Some(b)) if b < a) { return false; }
        let previous_timestamp = self.timestamp;
        let continuous = self.received.is_some_and(|at| now.duration_since(at) < Duration::from_secs(5));
        self.received = Some(now);
        self.timestamp = timestamp;
        let sample = (|| {
            timestamp?;
            let own = value["provider"]["steamid"].as_str().filter(|s| !s.is_empty())?;
            if value["player"]["steamid"].as_str()? != own { return None; }
            if value["player"]["activity"].as_str() != Some("playing") { return None; }
            let map = crate::model::normalize_map_name(value["map"]["name"].as_str()?)?;
            let kills = value["player"]["match_stats"]["kills"].as_u64()?;
            Some((own.to_owned(), map, kills))
        })();
        let killed = match (&self.baseline, &sample) {
            (Some((id, map, kills)), Some((next_id, next_map, next_kills))) =>
                continuous && id == next_id && map == next_map && next_kills > kills,
            _ => false,
        };
        if continuous && previous_timestamp == timestamp && matches!((&self.baseline, &sample),
            (Some((id, map, kills)), Some((next_id, next_map, next_kills))) if id == next_id && map == next_map && next_kills < kills) {
            return false;
        }
        self.baseline = sample;
        killed
    }
}

struct Inner { settings: Settings, tracker: KillTracker, error: Option<String> }
pub struct KillSound { cache: PathBuf, inner: Mutex<Inner>, audio: Audio }
impl KillSound {
    pub fn new(cache: PathBuf) -> Self {
        let mut settings: Settings = fs::read(cache.join("killsound.json")).ok()
            .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        settings.volume = settings.volume.min(100);
        settings.slot = settings.slot.min(1);
        if !sound_path(&cache, settings.slot).is_file() { settings.name.clear(); settings.enabled = false; }
        Self { cache, inner: Mutex::new(Inner { settings, tracker: KillTracker::default(), error: None }), audio: Audio::new() }
    }
    fn view(&self, inner: &Inner) -> Value {
        json!({"supported": cfg!(windows), "enabled": inner.settings.enabled,
            "volume": inner.settings.volume, "name": inner.settings.name,
            "has_file": !inner.settings.name.is_empty(), "error": inner.error,
            "gsi_ready": inner.tracker.baseline.is_some() && inner.tracker.received.is_some_and(|at| at.elapsed() < Duration::from_secs(5))})
    }
    pub fn status(&self) -> Value { self.view(&self.inner.lock().unwrap()) }
    fn save(&self, settings: &Settings) -> Result<(), String> {
        fs::write(self.cache.join("killsound.json"), serde_json::to_vec(settings).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Не удалось сохранить киллсаунд: {e}"))
    }
    pub fn update(&self, value: &Value) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        let mut settings = inner.settings.clone();
        if let Some(enabled) = value.get("enabled") { settings.enabled = enabled.as_bool().ok_or("Некорректный переключатель")?; }
        if let Some(volume) = value.get("volume") { settings.volume = volume.as_u64().filter(|n| *n <= 100).ok_or("Громкость: 0–100")? as u16; }
        if settings.enabled && settings.name.is_empty() { return Err("Сначала выберите MP3".into()); }
        self.save(&settings)?;
        inner.settings = settings;
        if !inner.settings.enabled || inner.settings.volume == 0 { let _ = self.audio.send(None, 0, false); }
        Ok(self.view(&inner))
    }
    pub fn upload(&self, name: &str, bytes: &[u8]) -> Result<Value, String> {
        if name.is_empty() || name.chars().count() > 160 || name.contains(['/', '\\', '\0'])
            || !name.to_ascii_lowercase().ends_with(".mp3") { return Err("Выберите файл .mp3".into()); }
        // Recognize an ID3 tag or an MPEG audio frame; Windows additionally
        // opens the file with its MP3 decoder before replacing the old sound.
        if bytes.is_empty() || bytes.len() > MAX_MP3_BYTES
            || !(bytes.starts_with(b"ID3") || bytes.windows(2).take(4096).any(|b| b[0] == 0xff && b[1] & 0xe0 == 0xe0 && b[1] & 0x18 != 0x08 && b[1] & 0x06 != 0)) {
            return Err("Нужен MP3-файл размером до 10 МБ".into());
        }
        let mut inner = self.inner.lock().unwrap();
        let mut settings = inner.settings.clone();
        settings.slot = 1 - settings.slot;
        let path = sound_path(&self.cache, settings.slot);
        fs::write(&path, bytes).map_err(|e| format!("Не удалось сохранить MP3: {e}"))?;
        if cfg!(windows) { self.audio.send(Some(path), settings.volume, false)?; }
        settings.name = name.to_owned();
        self.save(&settings)?;
        inner.settings = settings;
        inner.error = None;
        Ok(self.view(&inner))
    }
    pub fn preview(&self) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        self.play(&mut inner)?;
        Ok(self.view(&inner))
    }
    fn play(&self, inner: &mut Inner) -> Result<(), String> {
        if inner.settings.name.is_empty() { return Err("Сначала выберите MP3".into()); }
        let result = self.audio.send(Some(sound_path(&self.cache, inner.settings.slot)), inner.settings.volume, true);
        inner.error = result.as_ref().err().cloned();
        result
    }
    pub fn observe(&self, value: &Value) {
        let mut inner = self.inner.lock().unwrap();
        // Track even while disabled so enabling doesn't replay earlier kills.
        if inner.tracker.update(value, Instant::now()) && inner.settings.enabled && inner.settings.volume > 0 {
            let _ = self.play(&mut inner);
        }
    }
}
fn sound_path(cache: &Path, slot: u8) -> PathBuf { cache.join(format!("killsound-{slot}.mp3")) }

// MCI devices belong to a persistent audio thread, never a short-lived HTTP
// worker. A new kill restarts the sound, so long MP3s cannot accumulate.
#[cfg(windows)]
struct Audio { tx: std::sync::mpsc::Sender<AudioRequest> }
#[cfg(windows)]
struct AudioRequest { path: Option<PathBuf>, volume: u16, play: bool, reply: std::sync::mpsc::Sender<Result<(), String>> }
#[cfg(windows)]
impl Audio {
    fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<AudioRequest>();
        std::thread::spawn(move || {
            for request in rx {
                let _ = mci("close cas_killsound");
                let result = (|| {
                    let Some(path) = request.path else { return Ok(()); };
                    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
                    let path = path.to_str().ok_or("Не удалось открыть путь к MP3")?.trim_start_matches(r"\\?\");
                    mci(&format!("open \"{path}\" type mpegvideo alias cas_killsound"))?;
                    if request.play {
                        mci(&format!("setaudio cas_killsound volume to {}", request.volume * 10))?;
                        mci("play cas_killsound from 0")
                    } else { mci("close cas_killsound") }
                })();
                if result.is_err() { let _ = mci("close cas_killsound"); }
                let _ = request.reply.send(result);
            }
            let _ = mci("close cas_killsound");
        });
        Self { tx }
    }
    fn send(&self, path: Option<PathBuf>, volume: u16, play: bool) -> Result<(), String> {
        let (reply, rx) = std::sync::mpsc::channel();
        self.tx.send(AudioRequest { path, volume, play, reply }).map_err(|_| "Аудиопоток недоступен")?;
        rx.recv().map_err(|_| "Аудиопоток недоступен")?
    }
}
#[cfg(windows)]
fn mci(command: &str) -> Result<(), String> {
    #[link(name = "winmm")]
    extern "system" { fn mciSendStringW(command: *const u16, result: *mut u16, size: u32, callback: isize) -> u32; }
    let command: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    let error = unsafe { mciSendStringW(command.as_ptr(), std::ptr::null_mut(), 0, 0) };
    if error == 0 { Ok(()) } else { Err(format!("Windows не смог воспроизвести MP3 (код {error}). Выберите другой файл или проверьте аудиоустройство.")) }
}
#[cfg(not(windows))]
struct Audio;
#[cfg(not(windows))]
impl Audio {
    fn new() -> Self { Self }
    fn send(&self, _: Option<PathBuf>, _: u16, _: bool) -> Result<(), String> { Err("Воспроизведение киллсаунда доступно в Windows".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(kills: u64, time: u64) -> Value { json!({"provider":{"steamid":"own","timestamp":time},"map":{"name":"de_mirage"},"player":{"steamid":"own","activity":"playing","match_stats":{"kills":kills}}}) }
    #[test]
    fn own_kills_only_once_and_resets_are_silent() {
        let mut tracker = KillTracker::default(); let now = Instant::now();
        assert!(!tracker.update(&sample(9, 1), now));
        assert!(tracker.update(&sample(10, 2), now));
        assert!(!tracker.update(&sample(10, 2), now));
        assert!(!tracker.update(&sample(9, 2), now));
        assert!(!tracker.update(&sample(10, 2), now));
        assert!(!tracker.update(&sample(9, 1), now));
        assert!(!tracker.update(&sample(10, 2), now));
        assert!(!tracker.update(&sample(0, 3), now));
        assert!(tracker.update(&sample(1, 4), now));
        let mut spectated = sample(20, 5); spectated["player"]["steamid"] = json!("other");
        assert!(!tracker.update(&spectated, now));
        assert!(!tracker.update(&sample(2, 6), now));
        assert!(tracker.update(&sample(3, 6), now));
        assert!(!tracker.update(&sample(8, 7), now + Duration::from_secs(6)));
    }
    #[test]
    fn maps_menu_and_missing_data_rebaseline() {
        let mut tracker = KillTracker::default(); let now = Instant::now();
        tracker.update(&sample(1, 1), now);
        let mut next = sample(8, 2); next["map"]["name"] = json!("de_nuke");
        assert!(!tracker.update(&next, now));
        next["player"]["activity"] = json!("menu"); assert!(!tracker.update(&next, now));
        assert!(!tracker.update(&sample(20, 3), now));
        assert!(!tracker.update(&json!({"provider":{"timestamp":4}}), now));
        assert!(!tracker.update(&sample(22, 5), now));
    }
    #[test]
    #[cfg(not(windows))]
    fn files_and_settings_survive_restart_and_invalid_uploads_keep_the_old_sound() {
        let cache = std::env::temp_dir().join(format!("cas-killsound-{}", crate::session::random_token(12)));
        fs::create_dir_all(&cache).unwrap();
        let sound = KillSound::new(cache.clone());
        assert!(sound.update(&json!({"enabled":true})).is_err());
        let mp3 = include_bytes!("../scripts/fixtures/killsound.mp3");
        sound.upload("победа.mp3", mp3).unwrap();
        sound.update(&json!({"enabled":true,"volume":42})).unwrap();
        assert!(sound.upload("../escape.mp3", mp3).is_err());
        assert!(sound.upload("wrong.mp3", b"not an mp3").is_err());
        assert!(sound.upload("too-big.mp3", &vec![0; MAX_MP3_BYTES+1]).is_err());
        assert!(sound.update(&json!({"enabled":false,"volume":101})).is_err());
        drop(sound);
        let restored = KillSound::new(cache.clone());
        let status = restored.status();
        assert_eq!(status["name"], "победа.mp3");
        assert_eq!(status["volume"], 42); assert_eq!(status["enabled"], true);
        assert_eq!(fs::read(sound_path(&cache, 1)).unwrap(), mp3);
        fs::remove_dir_all(cache).unwrap();
    }
}
