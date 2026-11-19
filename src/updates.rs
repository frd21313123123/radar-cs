//! Check and install published Windows builds from GitHub Releases.
//!
//! The running EXE cannot replace itself, so the new binary is downloaded,
//! verified by the digest published by GitHub, and copied by a short-lived
//! PowerShell helper after the radar process exits.
use crate::assets;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub const REPO: &str = "frd21313123123/radar-cs";
const ASSET_NAME: &str = "cas-external-radar.exe";
const MAX_UPDATE_BYTES: usize = 20 * 1024 * 1024;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("Не удалось определить путь к EXE: {e}"))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 { break; }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn latest_release() -> Result<Value, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=10");
    let body = assets::download(&url, 2_000_000)?;
    let releases: Vec<Value> = serde_json::from_slice(&body)
        .map_err(|e| format!("GitHub releases: {e}"))?;
    releases.into_iter()
        .filter(|release| !release["draft"].as_bool().unwrap_or(false))
        .max_by(|a, b| a["published_at"].as_str().cmp(&b["published_at"].as_str()))
        .ok_or_else(|| "Нет опубликованных релизов".into())
}

pub fn check() -> Result<Value, String> {
    let release = latest_release()?;
    let assets = release["assets"].as_array().ok_or("В релизе нет файлов")?;
    let asset = assets.iter().find(|asset| asset["name"].as_str() == Some(ASSET_NAME))
        .ok_or_else(|| format!("В релизе нет {ASSET_NAME}"))?;
    let expected = asset["digest"].as_str().unwrap_or("")
        .strip_prefix("sha256:").unwrap_or("");
    let path = current_exe()?;
    let actual = sha256_file(&path)?;
    Ok(json!({
        "current_version": current_version(),
        "tag": release["tag_name"],
        "name": release["name"],
        "url": release["html_url"],
        "published_at": release["published_at"],
        "notes": release["body"],
        "asset_url": asset["browser_download_url"],
        "asset_size": asset["size"],
        "sha256": expected,
        "current_sha256": actual,
        "up_to_date": !expected.is_empty() && expected == actual,
    }))
}

fn updater_script(cache: &Path) -> Result<PathBuf, String> {
    let dir = cache.join("updates");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("install-update.ps1");
    let script = r#"param([int]$WaitPid,[string]$NewExe,[string]$OldExe)
$ErrorActionPreference = 'Stop'
while (Get-Process -Id $WaitPid -ErrorAction SilentlyContinue) { Start-Sleep -Milliseconds 400 }
for ($attempt = 0; $attempt -lt 40; $attempt++) {
  try {
    Copy-Item -LiteralPath $NewExe -Destination $OldExe -Force
    Start-Process -FilePath $OldExe
    exit 0
  } catch {
    Start-Sleep -Milliseconds 500
  }
}
exit 1
"#;
    fs::write(&path, script).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Download and verify the latest EXE, then hand replacement to the helper.
pub fn install(cache: &Path) -> Result<Value, String> {
    let info = check()?;
    if info["up_to_date"].as_bool().unwrap_or(false) {
        return Ok(json!({
            "installed": false, "up_to_date": true,
            "current_version": info["current_version"], "tag": info["tag"]
        }));
    }
    let url = info["asset_url"].as_str().ok_or("В релизе нет ссылки на EXE")?;
    let expected = info["sha256"].as_str().unwrap_or("");
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Релиз не содержит корректной SHA-256 для EXE".into());
    }
    let bytes = assets::download(url, MAX_UPDATE_BYTES)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if digest != expected {
        return Err("SHA-256 скачанного EXE не совпадает с релизом".into());
    }
    if !bytes.starts_with(b"MZ") {
        return Err("Скачанный файл не является Windows EXE".into());
    }
    let dir = cache.join("updates");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let new_exe = dir.join("cas-external-radar.exe.new");
    let temporary = dir.join("cas-external-radar.exe.part");
    fs::write(&temporary, &bytes).map_err(|e| e.to_string())?;
    fs::rename(&temporary, &new_exe).map_err(|e| e.to_string())?;
    let old_exe = current_exe()?;
    let script = updater_script(cache)?;
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-WaitPid").arg(std::process::id().to_string())
        .arg("-NewExe").arg(&new_exe)
        .arg("-OldExe").arg(&old_exe)
        .spawn()
        .map_err(|e| format!("Не удалось запустить установщик: {e}"))?;
    let _ = status;
    Ok(json!({
        "installed": true,
        "restarting": true,
        "version": info["current_version"],
        "tag": info["tag"],
        "sha256": expected,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_value() {
        let dir = std::env::temp_dir().join(format!("cas-update-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.bin");
        fs::write(&path, b"abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        fs::remove_dir_all(dir).unwrap();
    }
}
