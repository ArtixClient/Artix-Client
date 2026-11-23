//! Application state, background workers and message handling. (Drawing lives in ui.rs / skin_ui.rs.)
use eframe::egui::{self, TextureHandle};
use serde_json::Value;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use crate::skin_ui::SkinEditor;
use crate::store::{self, Account, Instance, Paths, Settings};
use crate::{auth, game, mods, sys};

#[derive(PartialEq, Clone, Copy)]
pub enum Page {
    Play,
    Instances,
    Skins,
    Account,
    Settings,
    Console,
}

pub enum Msg {
    Versions(Vec<game::VerInfo>),
    Progress(String, f32),
    Done,
    Error(String),
    Info(String),
    InstanceUpdated(Instance),
    AuthCode(auth::DeviceCode),
    AuthDone(Account, Value),
    AuthFail(String),
    AccountUpdated(Account, Value),
    AccountSkin(Vec<u8>, bool),
    SkinLoaded(crate::skin::Skin),
    Log(String),
    GameStarted,
    GameExited(i32),
    CloseApp,
}

pub struct Toast {
    pub text: String,
    pub err: bool,
    pub until: f64,
}

pub struct Draft {
    pub open: bool,
    pub name: String,
    pub mc: String,
    pub fabric: bool,
}

impl Default for Draft {
    fn default() -> Self {
        // Fabric by default: Artix Client (and the one-click performance mods) both need it.
        Draft { open: false, name: String::new(), mc: String::new(), fabric: true }
    }
}

pub struct App {
    pub ctx: egui::Context,
    pub tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub paths: Paths,
    pub st: Settings,
    pub insts: Vec<Instance>,
    pub accts: Vec<Account>,
    pub profiles: std::collections::HashMap<String, Value>,
    pub page: Page,
    pub sel: usize,
    pub versions: Vec<game::VerInfo>,
    pub busy: Option<(String, f32)>,
    pub running: bool,
    pub toast: Option<Toast>,
    pub log: VecDeque<String>,
    pub draft: Draft,
    pub device: Option<auth::DeviceCode>,
    pub signing_in: bool,
    pub cancel: Arc<AtomicBool>,
    pub offline: bool,
    pub total_ram: u64,
    pub skin: SkinEditor,
    pub head: Option<TextureHandle>,
    pub head_px: Option<Vec<u8>>,
    pub mod_sel: Vec<bool>,
    pub confirm_delete: bool,
    pub inst_dir_edit: String,
}

impl App {
    pub fn new(ctx: egui::Context) -> App {
        let st = store::load_settings();
        let paths = Paths::new(&st);
        let insts = store::load_instances(&paths);
        let accts = store::load_accounts();
        let (tx, rx) = channel();
        let mut app = App {
            ctx,
            tx,
            rx,
            inst_dir_edit: st.instances_dir.clone(),
            paths,
            st,
            insts,
            accts,
            profiles: Default::default(),
            page: Page::Play,
            sel: 0,
            versions: vec![],
            busy: None,
            running: false,
            toast: None,
            log: VecDeque::new(),
            draft: Draft::default(),
            device: None,
            signing_in: false,
            cancel: Arc::new(AtomicBool::new(false)),
            offline: false,
            total_ram: sys::total_ram_mb(),
            skin: SkinEditor::default(),
            head: None,
            head_px: None,
            mod_sel: mods::CATALOG.iter().enumerate().map(|(i, _)| i < 5).collect(),
            confirm_delete: false,
        };
        app.refresh_versions();
        app.verify_account();
        app
    }

    // ------------------------------------------------------------ helpers
    pub fn toast(&mut self, text: impl Into<String>, err: bool) {
        let t = self.ctx.input(|i| i.time);
        self.toast = Some(Toast { text: text.into(), err, until: t + 5.0 });
    }

    pub fn recommended_ram(&self) -> u32 {
        match self.total_ram {
            0..=3500 => 1536,
            3501..=5500 => 2048,
            5501..=9000 => 3072,
            _ => 4096,
        }
    }

    pub fn max_ram_slider(&self) -> u32 {
        ((self.total_ram * 3 / 4) as u32).clamp(1024, 16384) / 256 * 256
    }

    pub fn account(&self) -> Option<&Account> {
        self.accts.get(self.st.selected_account)
    }

    pub fn save_all(&self) {
        store::save_settings(&self.st);
        store::save_accounts(&self.accts);
        if let Some(i) = self.insts.get(self.sel) {
            store::save_instance(&self.paths, i);
        }
    }

    pub fn apply_instances_dir(&mut self) {
        self.st.instances_dir = self.inst_dir_edit.trim().to_string();
        self.paths = Paths::new(&self.st);
        self.insts = store::load_instances(&self.paths);
        self.sel = 0;
        store::save_settings(&self.st);
    }

    fn spawn<F: FnOnce(Sender<Msg>, egui::Context) + Send + 'static>(&self, f: F) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || f(tx, ctx));
    }

    pub fn refresh_versions(&mut self) {
        self.spawn(|tx, ctx| {
            if let Ok(v) = game::fetch_versions() {
                let _ = tx.send(Msg::Versions(v));
                ctx.request_repaint();
            }
        });
    }

    // ------------------------------------------------------------ accounts
    /// Official status check at startup: refresh tokens, confirm ownership through Minecraft Services, load the skin.
    pub fn verify_account(&mut self) {
        let Some(a) = self.account().cloned() else { return };
        if !auth::configured() {
            return;
        }
        self.spawn(move |tx, ctx| {
            let mut a = a;
            let res = (|| -> Result<(), String> {
                if sys::unix_now() >= a.mc_expires || a.mc_token.is_empty() {
                    let (ms, rt) = auth::refresh_ms(&a.refresh)?;
                    let s = auth::login_with_ms(&ms)?;
                    a.refresh = rt;
                    a.mc_token = s.token.clone();
                    a.mc_expires = s.expires_at;
                    a.uuid = s.uuid.clone();
                    a.name = s.name.clone();
                    a.owns = true;
                    let _ = tx.send(Msg::AccountUpdated(a.clone(), s.profile.clone()));
                    if let Ok(sk) = crate::skin::from_profile(&s.profile) {
                        let _ = tx.send(Msg::AccountSkin(sk.rgba, sk.slim));
                    }
                } else if let Ok((200, body)) = crate::net::get_bytes_auth("https://api.minecraftservices.com/minecraft/profile", &a.mc_token) {
                    let p: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let _ = tx.send(Msg::AccountUpdated(a.clone(), p.clone()));
                    if let Ok(sk) = crate::skin::from_profile(&p) {
                        let _ = tx.send(Msg::AccountSkin(sk.rgba, sk.slim));
                    }
                }
                Ok(())
            })();
            if let Err(e) = res {
                let _ = tx.send(Msg::Error(e));
            }
            ctx.request_repaint();
        });
    }

    pub fn start_sign_in(&mut self) {
        if !auth::configured() {
            self.toast("Microsoft sign-in isn't configured in this build (see README).", true);
            return;
        }
        self.signing_in = true;
        self.device = None;
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        self.spawn(move |tx, ctx| {
            let res = (|| -> Result<(Account, Value), String> {
                let dc = auth::start_device()?;
                let _ = tx.send(Msg::AuthCode(dc.clone()));
                ctx.request_repaint();
                let (ms, rt) = auth::poll_device(&dc, &cancel)?;
                let s = auth::login_with_ms(&ms)?;
                Ok((Account { uuid: s.uuid, name: s.name, refresh: rt, mc_token: s.token, mc_expires: s.expires_at, owns: s.owns }, s.profile))
            })();
            let _ = tx.send(match res {
                Ok((a, p)) => Msg::AuthDone(a, p),
                Err(e) => Msg::AuthFail(e),
            });
            ctx.request_repaint();
        });
    }

    // ------------------------------------------------------------ instances
    pub fn create_instance(&mut self) {
        let d = &self.draft;
        if d.mc.is_empty() {
            return;
        }
        let name = if d.name.trim().is_empty() { format!("Minecraft {}", d.mc) } else { d.name.trim().to_string() };
        let ram = self.recommended_ram().max(if self.st.default_ram_mb > 0 { self.st.default_ram_mb } else { 0 });
        let inst = Instance {
            id: store::unique_id(&self.paths, &name),
            name,
            mc: d.mc.clone(),
            loader: if d.fabric { "fabric".into() } else { "vanilla".into() },
            loader_ver: String::new(),
            ram_min: 512,
            ram_max: ram,
            width: 854,
            height: 480,
            fullscreen: false,
            jvm: String::new(),
            java: String::new(),
            gc: if self.total_ram <= 4500 { 1 } else { 0 },
            fixed_mem: false,
            last_played: 0,
        };
        store::save_instance(&self.paths, &inst);
        self.insts = store::load_instances(&self.paths);
        self.sel = self.insts.iter().position(|i| i.id == inst.id).unwrap_or(0);
        self.draft.open = false;
        self.start_install(inst);
    }

    pub fn delete_selected(&mut self) {
        if let Some(i) = self.insts.get(self.sel).cloned() {
            let _ = std::fs::remove_dir_all(self.paths.inst_dir(&i.id));
            self.insts = store::load_instances(&self.paths);
            self.sel = 0;
        }
    }

    pub fn duplicate_selected(&mut self) {
        if let Some(mut i) = self.insts.get(self.sel).cloned() {
            i.name = format!("{} copy", i.name);
            i.id = store::unique_id(&self.paths, &i.name);
            i.last_played = 0;
            store::save_instance(&self.paths, &i);
            self.insts = store::load_instances(&self.paths);
            self.sel = self.insts.iter().position(|x| x.id == i.id).unwrap_or(0);
        }
    }

    pub fn start_install(&mut self, inst: Instance) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(("Starting".into(), 0.0));
        let paths = self.paths.clone();
        self.spawn(move |tx, ctx| {
            let t2 = tx.clone();
            let c2 = ctx.clone();
            let prog = move |m: String, f: f32| {
                let _ = t2.send(Msg::Progress(m, f));
                c2.request_repaint();
            };
            match game::install(&paths, &inst, &prog) {
                Ok(done) => {
                    store::save_instance(&paths, &done);
                    let _ = std::fs::write(paths.inst_dir(&done.id).join(".installed"), env!("CARGO_PKG_VERSION"));
                    if done.loader == "fabric" {
                        let st = store::load_settings();
                        match mods::sync_artix(&paths.game_dir(&done.id), &st) {
                            Ok(Some(_)) => {}
                            Ok(None) => {}
                            Err(e) => {
                                let _ = tx.send(Msg::Log(format!("[artix-client] {e}")));
                            }
                        }
                    }
                    let _ = tx.send(Msg::InstanceUpdated(done));
                    let _ = tx.send(Msg::Info("Installed and ready to play".into()));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Error(e));
                }
            }
            let _ = tx.send(Msg::Done);
            ctx.request_repaint();
        });
    }

    pub fn start_mods(&mut self, inst: Instance, slugs: Vec<String>) {
        if self.busy.is_some() {
            return;
        }
        if inst.loader != "fabric" {
            self.toast("Mods need a Fabric instance. Change the loader first.", true);
            return;
        }
        self.busy = Some(("Installing mods".into(), 0.0));
        let gd = self.paths.game_dir(&inst.id);
        self.spawn(move |tx, ctx| {
            let (mut ok, mut failed) = (0, Vec::new());
            for (n, slug) in slugs.iter().enumerate() {
                let _ = tx.send(Msg::Progress(format!("Installing {slug}"), n as f32 / slugs.len().max(1) as f32));
                ctx.request_repaint();
                match mods::install(&gd, &inst.mc, slug) {
                    Ok(_) => ok += 1,
                    Err(e) => failed.push(format!("{slug}: {e}")),
                }
            }
            let _ = tx.send(if failed.is_empty() { Msg::Info(format!("Installed {ok} mods")) } else { Msg::Error(format!("Installed {ok}, failed: {}", failed.join("; "))) });
            let _ = tx.send(Msg::Done);
            ctx.request_repaint();
        });
    }

    pub fn play(&mut self) {
        let Some(inst) = self.insts.get(self.sel).cloned() else { return };
        if self.busy.is_some() || self.running {
            return;
        }
        let acct = self.account().cloned();
        if acct.is_none() {
            self.toast("Sign in with a Microsoft account first (Account tab).", true);
            self.page = Page::Account;
            return;
        }
        let offline = self.offline;
        self.busy = Some(("Preparing".into(), 0.0));
        let (paths, st) = (self.paths.clone(), self.st.clone());
        self.spawn(move |tx, ctx| {
            let res = launch(&tx, &ctx, &paths, &st, inst, acct.unwrap(), offline);
            if let Err(e) = res {
                let _ = tx.send(Msg::Error(e));
            }
            let _ = tx.send(Msg::Done);
            ctx.request_repaint();
        });
    }

    // ------------------------------------------------------------ message pump
    pub fn pump(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Versions(v) => {
                    if self.draft.mc.is_empty() {
                        if let Some(r) = v.iter().find(|x| x.kind == "release") {
                            self.draft.mc = r.id.clone();
                        }
                    }
                    self.versions = v;
                }
                Msg::Progress(t, f) => self.busy = Some((t, f)),
                Msg::Done => self.busy = None,
                Msg::Error(e) => {
                    self.log.push_back(format!("[launcher] {e}"));
                    self.toast(e, true);
                }
                Msg::Info(t) => self.toast(t, false),
                Msg::InstanceUpdated(i) => {
                    if let Some(x) = self.insts.iter_mut().find(|x| x.id == i.id) {
                        *x = i;
                    }
                }
                Msg::AuthCode(dc) => self.device = Some(dc),
                Msg::AuthDone(a, p) => {
                    self.signing_in = false;
                    self.device = None;
                    self.accts.retain(|x| x.uuid != a.uuid);
                    self.accts.push(a.clone());
                    self.st.selected_account = self.accts.len() - 1;
                    self.profiles.insert(a.uuid.clone(), p.clone());
                    store::save_accounts(&self.accts);
                    store::save_settings(&self.st);
                    self.toast(format!("Signed in as {}", a.name), false);
                    if let Ok(sk) = crate::skin::from_profile(&p) {
                        self.set_head(sk.rgba, sk.slim);
                    }
                }
                Msg::AuthFail(e) => {
                    self.signing_in = false;
                    self.device = None;
                    if e != "Cancelled" {
                        self.toast(e, true);
                    }
                }
                Msg::AccountUpdated(a, p) => {
                    if let Some(x) = self.accts.iter_mut().find(|x| x.refresh == a.refresh || x.uuid == a.uuid) {
                        *x = a.clone();
                    }
                    self.profiles.insert(a.uuid.clone(), p);
                    store::save_accounts(&self.accts);
                }
                Msg::AccountSkin(px, slim) => self.set_head(px, slim),
                Msg::SkinLoaded(s) => {
                    self.skin.load(s);
                    self.busy = None;
                }
                Msg::Log(l) => {
                    self.log.push_back(l);
                    while self.log.len() > 3000 {
                        self.log.pop_front();
                    }
                }
                Msg::GameStarted => self.running = true,
                Msg::GameExited(c) => {
                    self.running = false;
                    self.log.push_back(format!("[launcher] Game exited with code {c}"));
                    if c != 0 {
                        self.toast(format!("The game closed unexpectedly (code {c}). See the Console tab."), true);
                    }
                }
                Msg::CloseApp => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }

    fn set_head(&mut self, px: Vec<u8>, slim: bool) {
        self.head_px = Some(px.clone());
        self.skin.set_account_skin(px, slim);
    }
}

/// Runs on a worker thread: install if needed, verify the account, spawn the game.
fn launch(tx: &Sender<Msg>, ctx: &egui::Context, paths: &Paths, st: &Settings, mut inst: Instance, mut acct: Account, offline: bool) -> Result<(), String> {
    let prog = |m: &str, f: f32| {
        let _ = tx.send(Msg::Progress(m.to_string(), f));
        ctx.request_repaint();
    };
    let marker = paths.inst_dir(&inst.id).join(".installed");
    if !marker.exists() {
        let t2 = tx.clone();
        let c2 = ctx.clone();
        let p = move |m: String, f: f32| {
            let _ = t2.send(Msg::Progress(m, f));
            c2.request_repaint();
        };
        inst = game::install(paths, &inst, &p)?;
        store::save_instance(paths, &inst);
        std::fs::write(&marker, env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    }
    if inst.loader == "fabric" {
        if let Err(e) = mods::sync_artix(&paths.game_dir(&inst.id), st) {
            let _ = tx.send(Msg::Log(format!("[artix-client] {e}")));
        }
    }

    let auth = if offline && acct.owns {
        let mut h = <sha1::Sha1 as sha1::Digest>::new();
        sha1::Digest::update(&mut h, format!("OfflinePlayer:{}", acct.name).as_bytes());
        let mut d: Vec<u8> = sha1::Digest::finalize(h).to_vec();
        d.truncate(16);
        d[6] = (d[6] & 0x0f) | 0x30;
        d[8] = (d[8] & 0x3f) | 0x80;
        game::LaunchAuth { name: acct.name.clone(), uuid: d.iter().map(|b| format!("{b:02x}")).collect(), token: "0".into(), offline: true }
    } else {
        if sys::unix_now() >= acct.mc_expires || acct.mc_token.is_empty() {
            prog("Checking your account", 0.98);
            let (ms, rt) = auth::refresh_ms(&acct.refresh)?;
            let s = auth::login_with_ms(&ms)?;
            acct.refresh = rt;
            acct.mc_token = s.token.clone();
            acct.mc_expires = s.expires_at;
            acct.uuid = s.uuid.clone();
            acct.name = s.name.clone();
            let _ = tx.send(Msg::AccountUpdated(acct.clone(), s.profile));
        }
        game::LaunchAuth { name: acct.name.clone(), uuid: acct.uuid.clone(), token: acct.mc_token.clone(), offline: false }
    };

    prog("Starting Minecraft", 1.0);
    let cmd = game::build_command(paths, &inst, st, &auth)?;
    let mut c = std::process::Command::new(&cmd.java);
    c.args(&cmd.args).current_dir(&cmd.dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let to_file = st.close_on_launch;
    if to_file {
        let f = std::fs::File::create(paths.inst_dir(&inst.id).join("launcher-output.txt")).map_err(|e| e.to_string())?;
        c.stdout(f.try_clone().map_err(|e| e.to_string())?).stderr(f);
    } else {
        c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    }
    let mut child = c.spawn().map_err(|e| format!("Could not start Java: {e}"))?;
    inst.last_played = sys::unix_now();
    store::save_instance(paths, &inst);
    let _ = tx.send(Msg::InstanceUpdated(inst));
    let _ = tx.send(Msg::GameStarted);
    let _ = tx.send(Msg::Done);

    if !to_file {
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                for l in BufReader::new(stream).lines().map_while(Result::ok) {
                    let _ = tx.send(Msg::Log(l));
                    ctx.request_repaint();
                }
            });
        }
    }
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => {
                let _ = tx.send(Msg::GameExited(s.code().unwrap_or(-1)));
                return Ok(());
            }
            Ok(None) => {
                if to_file && started.elapsed().as_secs() >= 10 {
                    let _ = tx.send(Msg::CloseApp);
                    return Ok(());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Ok(())
}
