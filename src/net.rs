//! Thin HTTP helpers over ureq (blocking, rustls, no system dependencies).
use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

pub type R<T> = Result<T, String>;

const UA: &str = concat!("ArtixLauncher/", env!("CARGO_PKG_VERSION"));

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::AgentBuilder::new()
            .user_agent(UA)
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            .build()
    })
}

fn err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(c, _) => format!("HTTP {c}"),
        other => other.to_string(),
    }
}

pub fn get_value(url: &str) -> R<Value> {
    agent().get(url).call().map_err(err)?.into_json::<Value>().map_err(|e| e.to_string())
}

pub fn get_bytes(url: &str) -> R<Vec<u8>> {
    let mut v = Vec::new();
    agent().get(url).call().map_err(err)?.into_reader().take(64 << 20).read_to_end(&mut v).map_err(|e| e.to_string())?;
    Ok(v)
}

pub fn get_bytes_auth(url: &str, token: &str) -> R<(u16, Vec<u8>)> {
    match agent().get(url).set("Authorization", &format!("Bearer {token}")).call() {
        Ok(r) => {
            let s = r.status();
            let mut v = Vec::new();
            r.into_reader().take(8 << 20).read_to_end(&mut v).map_err(|e| e.to_string())?;
            Ok((s, v))
        }
        Err(ureq::Error::Status(c, r)) => {
            let mut v = Vec::new();
            let _ = r.into_reader().take(1 << 20).read_to_end(&mut v);
            Ok((c, v))
        }
        Err(e) => Err(e.to_string()),
    }
}

fn finish(r: Result<ureq::Response, ureq::Error>) -> R<(u16, Value)> {
    match r {
        Ok(r) => {
            let s = r.status();
            let t = r.into_string().unwrap_or_default();
            Ok((s, serde_json::from_str(&t).unwrap_or(Value::Null)))
        }
        Err(ureq::Error::Status(c, r)) => {
            let t = r.into_string().unwrap_or_default();
            Ok((c, serde_json::from_str(&t).unwrap_or(Value::Null)))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// POST application/x-www-form-urlencoded; returns (status, json) even on HTTP errors.
pub fn post_form(url: &str, form: &[(&str, &str)]) -> R<(u16, Value)> {
    finish(agent().post(url).send_form(form))
}

pub fn post_json(url: &str, body: &Value, bearer: Option<&str>) -> R<(u16, Value)> {
    let mut req = agent().post(url).set("Accept", "application/json");
    if let Some(t) = bearer {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    finish(req.send_json(body))
}

pub fn put_json(url: &str, body: &Value, bearer: &str) -> R<(u16, Value)> {
    finish(agent().put(url).set("Authorization", &format!("Bearer {bearer}")).send_json(body))
}

pub fn delete(url: &str, bearer: &str) -> R<u16> {
    finish(agent().delete(url).set("Authorization", &format!("Bearer {bearer}")).call()).map(|x| x.0)
}

pub fn post_bytes(url: &str, content_type: &str, body: &[u8], bearer: &str) -> R<(u16, Value)> {
    finish(agent().post(url).set("Authorization", &format!("Bearer {bearer}")).set("Content-Type", content_type).send_bytes(body))
}

fn sha1_hex(path: &Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha1::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads to `path` (via a .part file). Verifies SHA-1 when given. Retries 3 times.
pub fn download(url: &str, path: &Path, sha1: Option<&str>) -> R<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("{}part", path.extension().and_then(|e| e.to_str()).map(|e| format!("{e}.")).unwrap_or_default()));
    let mut last = String::new();
    for _ in 0..3 {
        let res = (|| -> R<()> {
            let resp = agent().get(url).call().map_err(err)?;
            let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            std::io::copy(&mut resp.into_reader(), &mut out).map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
            drop(out);
            if let Some(want) = sha1 {
                if sha1_hex(&tmp).as_deref() != Some(&want.to_lowercase()) {
                    return Err("checksum mismatch".into());
                }
            }
            std::fs::rename(&tmp, path).map_err(|e| e.to_string())
        })();
        match res {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(format!("{url}: {last}"))
}

pub struct Job {
    pub url: String,
    pub path: std::path::PathBuf,
    pub sha1: Option<String>,
    pub size: u64,
    pub exec: bool,
}

/// Downloads many files with a small thread pool. Skips files that already exist with the right size.
pub fn download_all(jobs: Vec<Job>, threads: usize, progress: &(dyn Fn(usize, usize) + Sync)) -> R<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let total = jobs.len();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let first_err: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= total || first_err.lock().unwrap().is_some() {
                    break;
                }
                let j = &jobs[i];
                let have = std::fs::metadata(&j.path).map(|m| j.size == 0 || m.len() == j.size).unwrap_or(false);
                if !have {
                    if let Err(e) = download(&j.url, &j.path, j.sha1.as_deref()) {
                        *first_err.lock().unwrap() = Some(e);
                        break;
                    }
                }
                #[cfg(unix)]
                if j.exec {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&j.path, std::fs::Permissions::from_mode(0o755));
                }
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(d, total);
            });
        }
    });
    match first_err.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
