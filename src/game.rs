//! Mojang version data, installing files, Java runtime, and building the launch command.
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::net::{self, download, download_all, Job, R};
use crate::store::{Instance, Paths, Settings};
use crate::sys;

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const RUNTIMES: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Clone)]
pub struct VerInfo {
    pub id: String,
    pub kind: String,
    pub url: String,
}

pub fn fetch_versions() -> R<Vec<VerInfo>> {
    let m = net::get_value(MANIFEST)?;
    Ok(m["versions"]
        .as_array()
        .ok_or("bad manifest")?
        .iter()
        .map(|v| VerInfo { id: v["id"].as_str().unwrap_or("").into(), kind: v["type"].as_str().unwrap_or("").into(), url: v["url"].as_str().unwrap_or("").into() })
        .collect())
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

// ---------------------------------------------------------------- rules
fn rules_ok(rules: &Value, features: &HashMap<&str, bool>) -> bool {
    let Some(arr) = rules.as_array() else { return true };
    if arr.is_empty() {
        return true;
    }
    let mut allowed = false;
    for r in arr {
        let mut m = true;
        if let Some(os) = r.get("os") {
            if let Some(n) = os["name"].as_str() {
                m &= n == sys::os_name();
            }
            match os["arch"].as_str() {
                Some("x86") => m &= std::env::consts::ARCH == "x86",
                Some("arm64") => m &= std::env::consts::ARCH == "aarch64",
                _ => {}
            }
            if os.get("version").is_some() {
                m = false;
            }
        }
        if let Some(f) = r.get("features").and_then(|f| f.as_object()) {
            for (k, v) in f {
                m &= features.get(k.as_str()).copied().unwrap_or(false) == v.as_bool().unwrap_or(false);
            }
        }
        if m {
            allowed = r["action"].as_str() == Some("allow");
        }
    }
    allowed
}

// ---------------------------------------------------------------- version json (with inheritance)
fn ver_json_path(p: &Paths, id: &str) -> PathBuf {
    p.versions.join(id).join(format!("{id}.json"))
}

fn lib_key(l: &Value) -> String {
    let n = s(l, "name");
    let mut it = n.split(':');
    let (g, a) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
    let _ver = it.next();
    format!("{g}:{a}:{}", it.next().unwrap_or(""))
}

pub fn load_version(p: &Paths, id: &str) -> R<Value> {
    let txt = std::fs::read_to_string(ver_json_path(p, id)).map_err(|_| format!("Version {id} is not installed"))?;
    let mut child: Value = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
    let Some(parent_id) = child["inheritsFrom"].as_str().map(String::from) else { return Ok(child) };
    let mut base = load_version(p, &parent_id)?;

    let mut libs: Vec<Value> = child["libraries"].as_array().cloned().unwrap_or_default();
    let keys: Vec<String> = libs.iter().map(lib_key).collect();
    for l in base["libraries"].as_array().cloned().unwrap_or_default() {
        if !keys.contains(&lib_key(&l)) {
            libs.push(l);
        }
    }
    base["libraries"] = Value::Array(libs);
    for part in ["jvm", "game"] {
        let mut v: Vec<Value> = base["arguments"][part].as_array().cloned().unwrap_or_default();
        v.extend(child["arguments"][part].as_array().cloned().unwrap_or_default());
        if !v.is_empty() {
            base["arguments"][part] = Value::Array(v);
        }
    }
    if let (Some(obj), Some(c)) = (base.as_object_mut(), child.as_object_mut()) {
        for k in ["id", "mainClass", "type", "releaseTime"] {
            if let Some(x) = c.remove(k) {
                obj.insert(k.into(), x);
            }
        }
        obj.insert("_parent".into(), Value::String(parent_id));
    }
    Ok(base)
}

pub fn fabric_id(mc: &str, lv: &str) -> String {
    format!("fabric-loader-{lv}-{mc}")
}

// ---------------------------------------------------------------- libraries
struct Lib {
    path: PathBuf,
    url: String,
    sha1: Option<String>,
    size: u64,
    native: bool,
    exclude: Vec<String>,
}

fn maven_path(name: &str) -> String {
    let (name, ext) = name.split_once('@').unwrap_or((name, "jar"));
    let p: Vec<&str> = name.split(':').collect();
    if p.len() < 3 {
        return name.replace(':', "/");
    }
    let cls = if p.len() > 3 { format!("-{}", p[3]) } else { String::new() };
    format!("{}/{}/{}/{}-{}{}.{}", p[0].replace('.', "/"), p[1], p[2], p[1], p[2], cls, ext)
}

fn features(inst: &Instance) -> HashMap<&'static str, bool> {
    let mut f = HashMap::new();
    f.insert("has_custom_resolution", !inst.fullscreen);
    f
}

fn libraries(p: &Paths, ver: &Value, inst: &Instance) -> Vec<Lib> {
    let feat = features(inst);
    let mut out = Vec::new();
    for l in ver["libraries"].as_array().cloned().unwrap_or_default() {
        if !rules_ok(&l["rules"], &feat) {
            continue;
        }
        let dl = &l["downloads"];
        if let Some(nat) = l["natives"].as_object() {
            if let Some(cls) = nat.get(sys::os_name()).and_then(|c| c.as_str()) {
                let cls = cls.replace("${arch}", if cfg!(target_pointer_width = "64") { "64" } else { "32" });
                let a = &dl["classifiers"][&cls];
                if a["url"].is_string() {
                    out.push(Lib {
                        path: p.libs.join(s(a, "path")),
                        url: s(a, "url").into(),
                        sha1: a["sha1"].as_str().map(String::from),
                        size: a["size"].as_u64().unwrap_or(0),
                        native: true,
                        exclude: l["extract"]["exclude"].as_array().map(|x| x.iter().filter_map(|e| e.as_str().map(String::from)).collect()).unwrap_or_default(),
                    });
                }
            }
        }
        let a = &dl["artifact"];
        if a["url"].is_string() {
            out.push(Lib { path: p.libs.join(s(a, "path")), url: s(a, "url").into(), sha1: a["sha1"].as_str().map(String::from), size: a["size"].as_u64().unwrap_or(0), native: false, exclude: vec![] });
        } else if dl.is_null() && l["name"].is_string() {
            let rel = maven_path(s(&l, "name"));
            let base = l["url"].as_str().unwrap_or("https://libraries.minecraft.net/");
            out.push(Lib { path: p.libs.join(&rel), url: format!("{}{}", base.trim_end_matches('/').to_owned() + "/", rel), sha1: None, size: 0, native: false, exclude: vec![] });
        }
    }
    out
}

// ---------------------------------------------------------------- install
pub fn install(p: &Paths, inst: &Instance, progress: &(dyn Fn(String, f32) + Sync)) -> R<Instance> {
    let mut inst = inst.clone();
    let step = |name: &str, f: f32| progress(name.to_string(), f);
    step("Reading version list", 0.01);
    let versions = fetch_versions()?;
    let info = versions.iter().find(|v| v.id == inst.mc).ok_or_else(|| format!("Unknown Minecraft version {}", inst.mc))?;
    let vjson = ver_json_path(p, &inst.mc);
    if !vjson.exists() {
        download(&info.url, &vjson, None)?;
    }
    let mut game_id = inst.mc.clone();
    if inst.loader == "fabric" {
        step("Setting up Fabric", 0.03);
        if inst.loader_ver.is_empty() {
            let list = net::get_value(&format!("https://meta.fabricmc.net/v2/versions/loader/{}", inst.mc))?;
            let arr = list.as_array().ok_or("No Fabric loader for this version")?;
            let pick = arr.iter().find(|e| e["loader"]["stable"].as_bool() == Some(true)).or(arr.first()).ok_or("No Fabric loader for this version")?;
            inst.loader_ver = pick["loader"]["version"].as_str().unwrap_or("").into();
        }
        game_id = fabric_id(&inst.mc, &inst.loader_ver);
        let fj = ver_json_path(p, &game_id);
        if !fj.exists() {
            let url = format!("https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json", inst.mc, inst.loader_ver);
            download(&url, &fj, None)?;
        }
    }
    let ver = load_version(p, &game_id)?;

    // client jar
    step("Downloading game", 0.05);
    let client = &ver["downloads"]["client"];
    let base_id = ver["_parent"].as_str().unwrap_or(&game_id).to_string();
    let jar = p.versions.join(&base_id).join(format!("{base_id}.jar"));
    if !jar.exists() || std::fs::metadata(&jar).map(|m| m.len()).unwrap_or(0) != client["size"].as_u64().unwrap_or(0) {
        download(s(client, "url"), &jar, client["sha1"].as_str())?;
    }

    // libraries
    let libs = libraries(p, &ver, &inst);
    let jobs: Vec<Job> = libs.iter().map(|l| Job { url: l.url.clone(), path: l.path.clone(), sha1: l.sha1.clone(), size: l.size, exec: false }).collect();
    download_all(jobs, 6, &|d, t| progress(format!("Libraries {d}/{t}"), 0.08 + 0.12 * d as f32 / t.max(1) as f32))?;

    // assets
    let ai = &ver["assetIndex"];
    let idx_path = p.assets.join("indexes").join(format!("{}.json", s(ai, "id")));
    if !idx_path.exists() {
        download(s(ai, "url"), &idx_path, ai["sha1"].as_str())?;
    }
    let idx: Value = serde_json::from_str(&std::fs::read_to_string(&idx_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let objs = idx["objects"].as_object().ok_or("bad asset index")?;
    let jobs: Vec<Job> = objs
        .values()
        .map(|o| {
            let h = s(o, "hash");
            Job { url: format!("https://resources.download.minecraft.net/{}/{}", &h[..2], h), path: p.assets.join("objects").join(&h[..2]).join(h), sha1: None, size: o["size"].as_u64().unwrap_or(0), exec: false }
        })
        .collect();
    download_all(jobs, 16, &|d, t| progress(format!("Assets {d}/{t}"), 0.20 + 0.55 * d as f32 / t.max(1) as f32))?;
    if idx["map_to_resources"].as_bool() == Some(true) || idx["virtual"].as_bool() == Some(true) {
        let vdir = p.assets.join("virtual").join(s(ai, "id"));
        for (name, o) in objs {
            let h = s(o, "hash");
            let dst = vdir.join(name);
            if !dst.exists() {
                if let Some(d) = dst.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::copy(p.assets.join("objects").join(&h[..2]).join(h), dst);
            }
        }
    }

    // Java runtime
    let comp = ver["javaVersion"]["component"].as_str().unwrap_or("jre-legacy").to_string();
    if inst.java.trim().is_empty() {
        ensure_runtime(p, &comp, progress)?;
    }
    progress("Ready".into(), 1.0);
    let _ = std::fs::create_dir_all(p.game_dir(&inst.id));
    Ok(inst)
}

// ---------------------------------------------------------------- java runtime (Mojang's official builds)
fn runtime_dir(p: &Paths, comp: &str) -> PathBuf {
    p.runtime.join(comp).join(sys::runtime_platform())
}

pub fn java_exe(p: &Paths, comp: &str) -> PathBuf {
    let d = runtime_dir(p, comp);
    if cfg!(windows) {
        d.join("bin").join("java.exe")
    } else if cfg!(target_os = "macos") {
        d.join("jre.bundle/Contents/Home/bin/java")
    } else {
        d.join("bin/java")
    }
}

fn ensure_runtime(p: &Paths, comp: &str, progress: &(dyn Fn(String, f32) + Sync)) -> R<()> {
    if java_exe(p, comp).exists() {
        return Ok(());
    }
    progress("Finding Java".into(), 0.76);
    let all = net::get_value(RUNTIMES)?;
    let entry = &all[sys::runtime_platform()][comp][0];
    let murl = entry["manifest"]["url"].as_str().ok_or_else(|| format!("No Java runtime '{comp}' for this system"))?;
    let manifest = net::get_value(murl)?;
    let dir = runtime_dir(p, comp);
    let mut jobs = Vec::new();
    for (path, f) in manifest["files"].as_object().ok_or("bad runtime manifest")? {
        match f["type"].as_str() {
            Some("directory") => {
                let _ = std::fs::create_dir_all(dir.join(path));
            }
            Some("file") => {
                let raw = &f["downloads"]["raw"];
                jobs.push(Job { url: s(raw, "url").into(), path: dir.join(path), sha1: raw["sha1"].as_str().map(String::from), size: raw["size"].as_u64().unwrap_or(0), exec: f["executable"].as_bool() == Some(true) });
            }
            _ => {}
        }
    }
    download_all(jobs, 8, &|d, t| progress(format!("Java {d}/{t}"), 0.76 + 0.23 * d as f32 / t.max(1) as f32))?;
    if java_exe(p, comp).exists() { Ok(()) } else { Err("Java runtime download is incomplete".into()) }
}

// ---------------------------------------------------------------- launch
pub struct LaunchAuth {
    pub name: String,
    pub uuid: String,
    pub token: String,
    pub offline: bool,
}

pub struct Command {
    pub java: PathBuf,
    pub args: Vec<String>,
    pub dir: PathBuf,
}

fn extract_natives(libs: &[Lib], dest: &Path) {
    let _ = std::fs::create_dir_all(dest);
    for l in libs.iter().filter(|l| l.native) {
        let Ok(f) = std::fs::File::open(&l.path) else { continue };
        let Ok(mut z) = zip::ZipArchive::new(f) else { continue };
        for i in 0..z.len() {
            let Ok(mut e) = z.by_index(i) else { continue };
            let n = e.name().to_string();
            if e.is_dir() || n.starts_with("META-INF") || l.exclude.iter().any(|x| n.starts_with(x.as_str())) {
                continue;
            }
            let Some(fname) = Path::new(&n).file_name() else { continue };
            if let Ok(mut out) = std::fs::File::create(dest.join(fname)) {
                let _ = std::io::copy(&mut e, &mut out);
            }
        }
    }
}

fn subst(s: &str, m: &HashMap<&str, String>) -> String {
    let mut out = s.to_string();
    for (k, v) in m {
        out = out.replace(&format!("${{{k}}}"), v);
    }
    out
}

fn collect_args(v: &Value, feat: &HashMap<&str, bool>, m: &HashMap<&str, String>, out: &mut Vec<String>) {
    for a in v.as_array().cloned().unwrap_or_default() {
        match &a {
            Value::String(x) => out.push(subst(x, m)),
            Value::Object(_) => {
                if rules_ok(&a["rules"], feat) {
                    match &a["value"] {
                        Value::String(x) => out.push(subst(x, m)),
                        Value::Array(xs) => out.extend(xs.iter().filter_map(|x| x.as_str()).map(|x| subst(x, m))),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn build_command(p: &Paths, inst: &Instance, st: &Settings, auth: &LaunchAuth) -> R<Command> {
    let game_id = if inst.loader == "fabric" { fabric_id(&inst.mc, &inst.loader_ver) } else { inst.mc.clone() };
    let ver = load_version(p, &game_id).map_err(|_| "Files aren't installed yet. Press Play again after the install finishes.".to_string())?;
    let libs = libraries(p, &ver, inst);
    let game_dir = p.game_dir(&inst.id);
    let natives = p.inst_dir(&inst.id).join("natives");
    std::fs::create_dir_all(&game_dir).map_err(|e| e.to_string())?;
    extract_natives(&libs, &natives);

    let base_id = ver["_parent"].as_str().unwrap_or(&game_id).to_string();
    let mut cp: Vec<String> = libs.iter().filter(|l| !l.native).map(|l| l.path.to_string_lossy().into_owned()).collect();
    cp.push(p.versions.join(&base_id).join(format!("{base_id}.jar")).to_string_lossy().into_owned());
    let classpath = cp.join(sys::cp_sep());

    let comp = ver["javaVersion"]["component"].as_str().unwrap_or("jre-legacy");
    let java = if !inst.java.trim().is_empty() {
        PathBuf::from(inst.java.trim())
    } else if !st.java_path.trim().is_empty() {
        PathBuf::from(st.java_path.trim())
    } else {
        java_exe(p, comp)
    };
    if !java.exists() {
        return Err("Java not found. Press Play again to download it, or set a Java path in the instance settings.".into());
    }

    let asset_id = s(&ver["assetIndex"], "id").to_string();
    let legacy_assets = p.assets.join("virtual").join(&asset_id);
    let mut m: HashMap<&str, String> = HashMap::new();
    m.insert("auth_player_name", auth.name.clone());
    m.insert("auth_uuid", auth.uuid.clone());
    m.insert("auth_access_token", auth.token.clone());
    m.insert("auth_session", format!("token:{}:{}", auth.token, auth.uuid));
    m.insert("user_type", if auth.offline { "legacy".into() } else { "msa".into() });
    m.insert("user_properties", "{}".into());
    m.insert("clientid", String::new());
    m.insert("auth_xuid", String::new());
    m.insert("version_name", game_id.clone());
    m.insert("version_type", ver["type"].as_str().unwrap_or("release").into());
    m.insert("game_directory", game_dir.to_string_lossy().into_owned());
    m.insert("assets_root", p.assets.to_string_lossy().into_owned());
    m.insert("game_assets", legacy_assets.to_string_lossy().into_owned());
    m.insert("assets_index_name", asset_id);
    m.insert("natives_directory", natives.to_string_lossy().into_owned());
    m.insert("launcher_name", "ArtixLauncher".into());
    m.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    m.insert("classpath", classpath.clone());
    m.insert("classpath_separator", sys::cp_sep().into());
    m.insert("library_directory", p.libs.to_string_lossy().into_owned());
    m.insert("resolution_width", inst.width.to_string());
    m.insert("resolution_height", inst.height.to_string());

    let feat = features(inst);
    let min_ram = if inst.fixed_mem { inst.ram_max } else { inst.ram_min.min(inst.ram_max) };
    let mut args: Vec<String> = vec![format!("-Xms{min_ram}M"), format!("-Xmx{}M", inst.ram_max)];
    match inst.gc {
        1 => args.extend(["-XX:+UseSerialGC", "-XX:-UsePerfData"].map(String::from)),
        2 => args.extend(
            [
                "-XX:+UseG1GC",
                "-XX:+ParallelRefProcEnabled",
                "-XX:MaxGCPauseMillis=100",
                "-XX:G1HeapRegionSize=16M",
                "-XX:-UsePerfData",
            ]
            .map(String::from),
        ),
        // "Ultimate": the well-known Aikar's G1 flags, tuned for low pause times and steady frame pacing.
        3 => args.extend(
            [
                "-XX:+UseG1GC",
                "-XX:+ParallelRefProcEnabled",
                "-XX:MaxGCPauseMillis=130",
                "-XX:+UnlockExperimentalVMOptions",
                "-XX:+DisableExplicitGC",
                "-XX:+AlwaysPreTouch",
                "-XX:G1NewSizePercent=30",
                "-XX:G1MaxNewSizePercent=40",
                "-XX:G1HeapRegionSize=8M",
                "-XX:G1ReservePercent=20",
                "-XX:G1HeapWastePercent=5",
                "-XX:G1MixedGCCountTarget=4",
                "-XX:InitiatingHeapOccupancyPercent=15",
                "-XX:G1MixedGCLiveThresholdPercent=90",
                "-XX:G1RSetUpdatingPauseTimePercent=5",
                "-XX:SurvivorRatio=32",
                "-XX:MaxTenuringThreshold=1",
                "-XX:-UsePerfData",
            ]
            .map(String::from),
        ),
        _ => {}
    }
    args.extend(inst.jvm.split_whitespace().map(String::from));

    let mut game_args: Vec<String> = Vec::new();
    if ver["arguments"].is_object() {
        collect_args(&ver["arguments"]["jvm"], &feat, &m, &mut args);
        collect_args(&ver["arguments"]["game"], &feat, &m, &mut game_args);
    } else {
        args.push(format!("-Djava.library.path={}", natives.display()));
        args.push("-cp".into());
        args.push(classpath);
        game_args.extend(s(&ver, "minecraftArguments").split_whitespace().map(|x| subst(x, &m)));
        if !inst.fullscreen {
            game_args.extend(["--width".into(), inst.width.to_string(), "--height".into(), inst.height.to_string()]);
        }
    }
    if inst.fullscreen {
        game_args.push("--fullscreen".into());
    }
    args.push(s(&ver, "mainClass").to_string());
    args.extend(game_args);
    Ok(Command { java, args, dir: game_dir })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inst() -> Instance {
        Instance { id: "t".into(), name: "t".into(), mc: "1.21".into(), loader: "fabric".into(), loader_ver: "0.16.0".into(), ram_min: 512, ram_max: 2048, width: 1280, height: 720, fullscreen: false, jvm: "-Dfoo=1".into(), java: String::new(), gc: 1, fixed_mem: false, last_played: 0 }
    }

    #[test]
    fn rules_and_maven() {
        let f = HashMap::new();
        assert!(rules_ok(&Value::Null, &f));
        assert!(!rules_ok(&json!([{"action":"allow","os":{"name":"definitely-not-this-os"}}]), &f));
        assert!(rules_ok(&json!([{"action":"allow"},{"action":"disallow","os":{"name":"definitely-not-this-os"}}]), &f));
        assert_eq!(maven_path("net.fabricmc:fabric-loader:0.16.0"), "net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar");
        assert_eq!(maven_path("org.lwjgl:lwjgl:3.3.3:natives-linux"), "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar");
    }

    #[test]
    fn builds_fabric_command() {
        let root = std::env::temp_dir().join(format!("artix-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let st = Settings::default();
        let mut p = Paths::new(&st);
        p.root = root.clone();
        p.libs = root.join("libraries");
        p.assets = root.join("assets");
        p.versions = root.join("versions");
        p.runtime = root.join("runtime");
        p.instances = root.join("instances");
        std::fs::create_dir_all(p.versions.join("1.21")).unwrap();
        std::fs::create_dir_all(p.versions.join("fabric-loader-0.16.0-1.21")).unwrap();
        let vanilla = json!({
            "id":"1.21","type":"release","mainClass":"net.minecraft.client.main.Main",
            "javaVersion":{"component":"java-runtime-delta","majorVersion":21},
            "assetIndex":{"id":"17","url":"x","sha1":"y"},
            "downloads":{"client":{"url":"x","sha1":"y","size":1}},
            "libraries":[
                {"name":"a:b:1","downloads":{"artifact":{"path":"a/b/1/b-1.jar","url":"u","sha1":"s","size":1}}},
                {"name":"c:d:1","rules":[{"action":"allow","os":{"name":"definitely-not-this-os"}}],"downloads":{"artifact":{"path":"c/d/1/d-1.jar","url":"u","sha1":"s","size":1}}}
            ],
            "arguments":{
                "jvm":["-Djava.library.path=${natives_directory}","-cp","${classpath}"],
                "game":["--username","${auth_player_name}","--version","${version_name}","--assetsDir","${assets_root}",
                        {"rules":[{"action":"allow","features":{"has_custom_resolution":true}}],"value":["--width","${resolution_width}","--height","${resolution_height}"]},
                        {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"}]
            }
        });
        let fabric = json!({
            "id":"fabric-loader-0.16.0-1.21","inheritsFrom":"1.21","type":"release","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries":[{"name":"net.fabricmc:fabric-loader:0.16.0","url":"https://maven.fabricmc.net/"}],
            "arguments":{"game":[],"jvm":["-DFabricMcEmu= net.minecraft.client.main.Main "]}
        });
        std::fs::write(ver_json_path(&p, "1.21"), vanilla.to_string()).unwrap();
        std::fs::write(ver_json_path(&p, "fabric-loader-0.16.0-1.21"), fabric.to_string()).unwrap();
        let java = java_exe(&p, "java-runtime-delta");
        std::fs::create_dir_all(java.parent().unwrap()).unwrap();
        std::fs::write(&java, "x").unwrap();

        let auth = LaunchAuth { name: "Steve".into(), uuid: "abc".into(), token: "tok".into(), offline: false };
        let c = build_command(&p, &inst(), &st, &auth).unwrap();
        let a = c.args.join(" ");
        assert!(a.contains("-Xmx2048M") && a.contains("-Xms512M"), "{a}");
        assert!(a.contains("-XX:+UseSerialGC") && a.contains("-Dfoo=1"));
        assert!(a.contains("net.fabricmc.loader.impl.launch.knot.KnotClient"));
        assert!(a.contains("--username Steve") && a.contains("--version fabric-loader-0.16.0-1.21"));
        assert!(a.contains("--width 1280 --height 720"));
        assert!(!a.contains("--demo"));
        assert!(a.contains("b-1.jar") && a.contains("fabric-loader-0.16.0.jar") && a.contains("1.21.jar"), "{a}");
        assert!(!a.contains("d-1.jar"));
        assert!(c.args.iter().any(|x| x == "-DFabricMcEmu= net.minecraft.client.main.Main "));
        assert!(c.dir.ends_with("game"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
