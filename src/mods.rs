//! One-click performance mods from Modrinth, plus bundling Artix Client (your own mod) locally.
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::net::{self, R};
use crate::store::Settings;
use crate::sys;

/// (Modrinth slug, label)
pub const CATALOG: &[(&str, &str)] = &[
    ("fabric-api", "Fabric API (needed by most mods, incl. Artix Client)"),
    ("sodium", "Sodium - much faster rendering"),
    ("lithium", "Lithium - faster game logic"),
    ("ferrite-core", "FerriteCore - less RAM usage"),
    ("modernfix", "ModernFix - faster startup, less RAM"),
    ("entityculling", "Entity Culling - skips hidden entities"),
    ("immediatelyfast", "ImmediatelyFast - faster HUD/GUI"),
];

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn marker(game_dir: &Path) -> std::path::PathBuf {
    game_dir.join("mods").join(".artix-managed.json")
}

const ARTIX_KEY: &str = "artix-client";

/// Finds Artix Client's jar: the path set in Settings, else next to the launcher executable,
/// else the launcher's own data folder. Whichever is newest if more than one exists.
pub fn locate_artix(st: &Settings) -> Option<PathBuf> {
    let configured = st.artix_jar.trim();
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ArtixClient.jar"));
            candidates.push(dir.join("artix-client.jar"));
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    if n.starts_with("artix-client") && n.ends_with(".jar") {
                        candidates.push(e.path());
                    }
                }
            }
        }
    }
    candidates.push(sys::data_root().join("ArtixClient").join("artix-client.jar"));
    candidates.into_iter().filter(|p| p.is_file()).max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

pub struct ArtixStatus {
    pub found: bool,
    pub path: Option<PathBuf>,
    pub size_kb: u64,
}

pub fn artix_status(st: &Settings) -> ArtixStatus {
    match locate_artix(st) {
        Some(p) => {
            let size_kb = std::fs::metadata(&p).map(|m| m.len() / 1024).unwrap_or(0);
            ArtixStatus { found: true, path: Some(p), size_kb }
        }
        None => ArtixStatus { found: false, path: None, size_kb: 0 },
    }
}

/// Copies the current Artix Client jar into a Fabric instance's mods folder, replacing any older
/// copy, so every launch runs whatever you most recently built. No-op (Ok) if no jar is configured.
pub fn sync_artix(game_dir: &Path, st: &Settings) -> R<Option<String>> {
    let Some(src) = locate_artix(st) else { return Ok(None) };
    let mods = game_dir.join("mods");
    std::fs::create_dir_all(&mods).map_err(|e| e.to_string())?;
    let mut managed: serde_json::Map<String, Value> = std::fs::read_to_string(marker(game_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();

    let src_meta = std::fs::metadata(&src).map_err(|e| e.to_string())?;
    let name = "artix-client.jar".to_string();
    let dest = mods.join(&name);
    let up_to_date = dest.is_file()
        && std::fs::metadata(&dest).ok().map(|m| m.len()) == Some(src_meta.len())
        && std::fs::metadata(&dest).ok().and_then(|m| m.modified().ok()) >= src_meta.modified().ok();
    if let Some(old) = managed.get(ARTIX_KEY).and_then(|v| v.as_str()) {
        if old != name {
            let _ = std::fs::remove_file(mods.join(old));
        }
    }
    if !up_to_date {
        std::fs::copy(&src, &dest).map_err(|e| format!("Couldn't copy Artix Client: {e}"))?;
        if let Ok(t) = src_meta.modified() {
            let _ = filetime_set(&dest, t);
        }
    }
    managed.insert(ARTIX_KEY.into(), Value::String(name.clone()));
    let _ = std::fs::write(marker(game_dir), serde_json::to_string_pretty(&managed).unwrap_or_default());
    Ok(Some(name))
}

fn filetime_set(_p: &Path, _t: std::time::SystemTime) -> std::io::Result<()> {
    // Best-effort only; a mismatch just means the next launch re-copies the (identical) file.
    Ok(())
}

/// Installs (or updates) one mod for this Minecraft version. Returns the file name.
pub fn install(game_dir: &Path, mc: &str, slug: &str) -> R<String> {
    let url = format!(
        "https://api.modrinth.com/v2/project/{slug}/version?loaders={}&game_versions={}",
        enc("[\"fabric\"]"),
        enc(&format!("[\"{mc}\"]"))
    );
    let list = net::get_value(&url)?;
    let arr = list.as_array().ok_or("bad response")?;
    let ver = arr.iter().find(|v| v["version_type"] == "release").or(arr.first()).ok_or_else(|| format!("{slug}: no version for Minecraft {mc}"))?;
    let files = ver["files"].as_array().ok_or("no files")?;
    let f = files.iter().find(|f| f["primary"] == true).or(files.first()).ok_or("no files")?;
    let name = f["filename"].as_str().ok_or("no filename")?.to_string();
    let mods = game_dir.join("mods");
    std::fs::create_dir_all(&mods).map_err(|e| e.to_string())?;

    let mut managed: serde_json::Map<String, Value> = std::fs::read_to_string(marker(game_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if let Some(old) = managed.get(slug).and_then(|v| v.as_str()) {
        if old != name {
            let _ = std::fs::remove_file(mods.join(old));
        }
    }
    let dest = mods.join(&name);
    if !dest.exists() {
        net::download(f["url"].as_str().ok_or("no url")?, &dest, f["hashes"]["sha1"].as_str())?;
    }
    managed.insert(slug.into(), Value::String(name.clone()));
    let _ = std::fs::write(marker(game_dir), serde_json::to_string_pretty(&managed).unwrap_or_default());
    Ok(name)
}
