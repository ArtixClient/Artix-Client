//! Small OS helpers (no heavy dependencies).
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn data_root() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ArtixLauncher")
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Mojang rule name for this OS.
pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

/// Key used by Mojang's official Java runtime manifest.
pub fn runtime_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", _) => "windows-x86",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        (_, "x86") => "linux-i386",
        _ => "linux",
    }
}

pub fn cp_sep() -> &'static str {
    if cfg!(windows) { ";" } else { ":" }
}

pub fn total_ram_mb() -> u64 {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct MemStatus {
            len: u32,
            load: u32,
            total_phys: u64,
            avail_phys: u64,
            total_page: u64,
            avail_page: u64,
            total_virt: u64,
            avail_virt: u64,
            avail_ext: u64,
        }
        extern "system" {
            fn GlobalMemoryStatusEx(buf: *mut MemStatus) -> i32;
        }
        let mut m = MemStatus { len: std::mem::size_of::<MemStatus>() as u32, load: 0, total_phys: 0, avail_phys: 0, total_page: 0, avail_page: 0, total_virt: 0, avail_virt: 0, avail_ext: 0 };
        if unsafe { GlobalMemoryStatusEx(&mut m) } != 0 {
            return m.total_phys / 1024 / 1024;
        }
        4096
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
            for l in s.lines() {
                if let Some(r) = l.strip_prefix("MemTotal:") {
                    let kb: u64 = r.trim().trim_end_matches("kB").trim().parse().unwrap_or(0);
                    if kb > 0 {
                        return kb / 1024;
                    }
                }
            }
        }
        4096
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = Command::new("sysctl").args(["-n", "hw.memsize"]).output() {
            if let Ok(b) = String::from_utf8_lossy(&o.stdout).trim().parse::<u64>() {
                return b / 1024 / 1024;
            }
        }
        4096
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        4096
    }
}

pub fn open_path(p: &Path) {
    let _ = std::fs::create_dir_all(p);
    open_target(&p.to_string_lossy());
}

pub fn open_url(url: &str) {
    open_target(url);
}

fn open_target(t: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("explorer").arg(t).creation_flags(0x0800_0000).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(t).spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(t).spawn();
    }
}

/// Native file picker where available (Windows/macOS); None elsewhere.
pub fn pick_png() -> Option<PathBuf> {
    pick_file("PNG image", &["png"])
}

/// Generic native file picker with a named filter (e.g. "Jar file", &["jar"]). None where no dialog is available.
pub fn pick_file(filter_name: &str, exts: &[&str]) -> Option<PathBuf> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        return rfd::FileDialog::new().add_filter(filter_name, exts).pick_file();
    }
    #[allow(unreachable_code)]
    {
        let _ = (filter_name, exts);
        None
    }
}

pub fn save_png_dialog(name: &str) -> Option<PathBuf> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        return rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name(name).save_file();
    }
    #[allow(unreachable_code)]
    {
        let _ = name;
        None
    }
}

pub fn pick_folder() -> Option<PathBuf> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        return rfd::FileDialog::new().pick_folder();
    }
    #[allow(unreachable_code)]
    None
}
