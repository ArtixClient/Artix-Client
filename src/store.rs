//! Persistent data: settings, instances, accounts. Everything stays on this PC.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::sys;

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    /// Where instances live. Empty = <data>/instances. Always separate from .minecraft.
    #[serde(default)]
    pub instances_dir: String,
    #[serde(default = "yes")]
    pub close_on_launch: bool,
    #[serde(default)]
    pub show_snapshots: bool,
    #[serde(default)]
    pub default_ram_mb: u32,
    #[serde(default)]
    pub java_path: String,
    #[serde(default)]
    pub selected_account: usize,
    /// Path to Artix Client's jar (your own mod). Auto-installed into every Fabric instance.
    #[serde(default)]
    pub artix_jar: String,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings { instances_dir: String::new(), close_on_launch: true, show_snapshots: false, default_ram_mb: 0, java_path: String::new(), selected_account: 0, artix_jar: String::new() }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub mc: String,
    /// "vanilla" or "fabric"
    pub loader: String,
    #[serde(default)]
    pub loader_ver: String,
    pub ram_min: u32,
    pub ram_max: u32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub jvm: String,
    #[serde(default)]
    pub java: String,
    /// 0 default, 1 low-memory (Serial GC), 2 smooth (tuned G1), 3 ultimate (Aikar's flags)
    #[serde(default)]
    pub gc: u8,
    /// Xms = Xmx (no heap resizing mid-game). Recommended with gc modes 2/3.
    #[serde(default)]
    pub fixed_mem: bool,
    #[serde(default)]
    pub last_played: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub uuid: String,
    pub name: String,
    pub refresh: String,
    #[serde(default)]
    pub mc_token: String,
    #[serde(default)]
    pub mc_expires: u64,
    #[serde(default)]
    pub owns: bool,
}

#[derive(Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub libs: PathBuf,
    pub assets: PathBuf,
    pub versions: PathBuf,
    pub runtime: PathBuf,
    pub instances: PathBuf,
}

impl Paths {
    pub fn new(settings: &Settings) -> Paths {
        let root = sys::data_root();
        let instances = if settings.instances_dir.trim().is_empty() { root.join("instances") } else { PathBuf::from(settings.instances_dir.trim()) };
        Paths { libs: root.join("libraries"), assets: root.join("assets"), versions: root.join("versions"), runtime: root.join("runtime"), instances, root }
    }
    pub fn inst_dir(&self, id: &str) -> PathBuf {
        self.instances.join(id)
    }
    pub fn game_dir(&self, id: &str) -> PathBuf {
        self.inst_dir(id).join("game")
    }
}

fn read<T: for<'a> Deserialize<'a>>(p: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

fn write<T: Serialize>(p: &Path, v: &T) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(v) {
        let _ = std::fs::write(p, s);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
        }
    }
}

pub fn load_settings() -> Settings {
    read(&sys::data_root().join("settings.json")).unwrap_or_default()
}
pub fn save_settings(s: &Settings) {
    write(&sys::data_root().join("settings.json"), s)
}
pub fn load_accounts() -> Vec<Account> {
    read(&sys::data_root().join("accounts.json")).unwrap_or_default()
}
pub fn save_accounts(a: &Vec<Account>) {
    write(&sys::data_root().join("accounts.json"), a)
}

pub fn load_instances(paths: &Paths) -> Vec<Instance> {
    let mut v: Vec<Instance> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&paths.instances) {
        for e in rd.flatten() {
            if let Some(i) = read::<Instance>(&e.path().join("instance.json")) {
                v.push(i);
            }
        }
    }
    v.sort_by(|a, b| b.last_played.cmp(&a.last_played).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

pub fn save_instance(paths: &Paths, i: &Instance) {
    write(&paths.inst_dir(&i.id).join("instance.json"), i)
}

pub fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "instance".into() } else { s }
}

pub fn unique_id(paths: &Paths, name: &str) -> String {
    let base = slug(name);
    let mut id = base.clone();
    let mut n = 2;
    while paths.inst_dir(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}
