//! All drawing: theme, sidebar and pages.
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, FontId, Layout, RichText, Sense, Stroke};

use crate::app::{App, Page};
use crate::skin_ui::{draw_head, SkinAction};
use crate::{auth, mods, skin, sys};

const ACCENT: Color32 = Color32::from_rgb(0, 229, 255);
const ACCENT2: Color32 = Color32::from_rgb(139, 92, 255);
const CARD: Color32 = Color32::from_rgb(26, 26, 44);
const CARD_HOVER: Color32 = Color32::from_rgb(33, 33, 56);
const DIM: Color32 = Color32::from_rgb(138, 138, 175);
const GOOD: Color32 = Color32::from_rgb(90, 220, 150);
const BAD: Color32 = Color32::from_rgb(255, 110, 120);

pub fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(14, 14, 22);
    v.window_fill = Color32::from_rgb(20, 20, 33);
    v.extreme_bg_color = Color32::from_rgb(10, 10, 17);
    v.faint_bg_color = Color32::from_rgb(20, 20, 32);
    v.window_rounding = 12.0.into();
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(20, 20, 33);
    v.widgets.inactive.bg_fill = Color32::from_rgb(32, 32, 52);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(32, 32, 52);
    v.widgets.hovered.bg_fill = Color32::from_rgb(46, 46, 74);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(46, 46, 74);
    v.widgets.active.bg_fill = Color32::from_rgb(64, 64, 100);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(64, 64, 100);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
        w.rounding = 8.0.into();
    }
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(139, 92, 255, 120);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    ctx.set_visuals(v);
    let mut s = (*ctx.style()).clone();
    s.spacing.item_spacing = vec2(10.0, 8.0);
    s.spacing.button_padding = vec2(14.0, 7.0);
    s.spacing.slider_width = 220.0;
    for (ts, size) in [(egui::TextStyle::Body, 15.0), (egui::TextStyle::Button, 15.0), (egui::TextStyle::Heading, 26.0), (egui::TextStyle::Small, 12.5)] {
        if let Some(f) = s.text_styles.get_mut(&ts) {
            f.size = size;
        }
    }
    ctx.set_style(s);
}

fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).strong().color(Color32::BLACK)).fill(ACCENT)
}

/// Soft drop shadow behind the rect a card is about to occupy at `top_left`/width `w`/height `h`.
fn shadow(ui: &mut egui::Ui, rect: egui::Rect, glow: Color32) {
    let p = ui.painter();
    for i in (1..=4).rev() {
        let o = i as f32 * 1.6;
        p.rect_filled(rect.expand(o), 11.0 + o * 0.4, Color32::from_black_alpha(14));
    }
    if glow.a() > 0 {
        p.rect_stroke(rect.expand(1.0), 11.0, Stroke::new(1.0, glow));
    }
}

fn card<R>(ui: &mut egui::Ui, selected: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::Response {
    let start = ui.next_widget_position();
    let frame = egui::Frame::none()
        .fill(CARD)
        .rounding(11.0)
        .inner_margin(14.0)
        .stroke(Stroke::new(1.4, if selected { ACCENT } else { Color32::from_rgb(42, 42, 68) }))
        .show(ui, add);
    shadow(ui, egui::Rect::from_min_size(start, frame.response.rect.size()), if selected { Color32::from_rgba_unmultiplied(0, 229, 255, 40) } else { Color32::TRANSPARENT });
    frame.response
}

/// Soft multi-colour glow blobs behind the whole content area. Drawn once per frame (cheap: a
/// handful of filled circles), not animated, so it costs nothing while the app is idle.
fn background(ui: &mut egui::Ui, rect: egui::Rect) {
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::from_rgb(13, 13, 21));
    let blobs: [(f32, f32, f32, Color32); 3] = [
        (0.08, 0.05, 0.42, Color32::from_rgba_unmultiplied(0, 229, 255, 20)),
        (0.95, 0.12, 0.34, Color32::from_rgba_unmultiplied(139, 92, 255, 22)),
        (0.65, 0.95, 0.46, Color32::from_rgba_unmultiplied(90, 60, 200, 14)),
    ];
    for (fx, fy, fr, col) in blobs {
        let center = rect.min + vec2(rect.width() * fx, rect.height() * fy);
        let r = rect.width().max(rect.height()) * fr;
        let steps = 10;
        for i in (0..steps).rev() {
            let t = i as f32 / steps as f32;
            let rr = r * (1.0 - t * 0.88);
            let a = (col.a() as f32 * (1.0 - t) * 0.16) as u8;
            p.circle_filled(center, rr, Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), a));
        }
    }
}

// ---- tiny vector icons (so the sidebar doesn't need an icon font / image assets) ----
fn icon_play(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-s * 0.35, -s * 0.5), c + vec2(-s * 0.35, s * 0.5), c + vec2(s * 0.55, 0.0)], col, Stroke::NONE));
}
fn icon_grid(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    let q = s * 0.21;
    for dx in [-1.0f32, 1.0] {
        for dy in [-1.0f32, 1.0] {
            p.rect_filled(egui::Rect::from_center_size(c + vec2(dx * q * 1.3, dy * q * 1.3), vec2(q * 1.8, q * 1.8)), 2.0, col);
        }
    }
}
fn icon_palette(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    p.circle_filled(c, s * 0.5, col);
    p.circle_filled(c + vec2(s * 0.08, -s * 0.1), s * 0.26, Color32::from_rgb(13, 13, 21));
    for (dx, dy, cc) in [(-0.3, 0.05, ACCENT2), (0.05, 0.35, Color32::WHITE), (-0.05, -0.33, BAD)] {
        p.circle_filled(c + vec2(dx * s, dy * s), s * 0.1, cc);
    }
}
fn icon_person(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    p.circle_filled(c - vec2(0.0, s * 0.18), s * 0.24, col);
    p.add(egui::Shape::convex_polygon(
        vec![c + vec2(-s * 0.4, s * 0.5), c + vec2(s * 0.4, s * 0.5), c + vec2(s * 0.28, s * 0.08), c + vec2(-s * 0.28, s * 0.08)],
        col,
        Stroke::NONE,
    ));
}
fn icon_gear(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        p.circle_filled(c + vec2(a.cos(), a.sin()) * s * 0.42, s * 0.14, col);
    }
    p.circle_filled(c, s * 0.42, col);
    p.circle_filled(c, s * 0.18, Color32::from_rgb(13, 13, 21));
}
fn icon_console(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    let r = egui::Rect::from_center_size(c, vec2(s * 1.0, s * 0.8));
    p.rect_stroke(r, 2.0, Stroke::new(1.6, col));
    p.line_segment([c + vec2(-s * 0.28, -s * 0.12), c + vec2(-s * 0.06, 0.0)], Stroke::new(1.6, col));
    p.line_segment([c + vec2(-s * 0.28, 0.12 * s), c + vec2(-s * 0.06, 0.0)], Stroke::new(1.6, col));
}

fn fmt_gb(mb: u32) -> String {
    format!("{:.1} GB", mb as f32 / 1024.0)
}

fn ago(ts: u64) -> String {
    if ts == 0 {
        return "never played".into();
    }
    let d = sys::unix_now().saturating_sub(ts);
    match d {
        0..=119 => "just now".into(),
        120..=7199 => format!("{} min ago", d / 60),
        7200..=172799 => format!("{} h ago", d / 3600),
        _ => format!("{} days ago", d / 86400),
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        self.pump();
        egui::SidePanel::left("nav")
            .exact_width(196.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(Color32::from_rgb(10, 10, 17)).inner_margin(16.0))
            .show(ctx, |ui| self.sidebar(ui));
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            background(ui, ui.max_rect());
            egui::Frame::none().inner_margin(24.0).show(ui, |ui| match self.page {
                Page::Play => self.page_play(ui),
                Page::Instances => self.page_instances(ui),
                Page::Skins => self.page_skins(ui),
                Page::Account => self.page_account(ui),
                Page::Settings => self.page_settings(ui),
                Page::Console => self.page_console(ui),
            });
        });
        self.new_instance_window(ctx);
        self.toasts(ctx);
        if self.busy.is_some() || self.signing_in {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

impl App {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
            let p = ui.painter();
            for i in (1..=3).rev() {
                p.circle_filled(r.center(), 15.0 + i as f32 * 1.6, Color32::from_rgba_unmultiplied(0, 229, 255, 10));
            }
            p.circle_filled(r.center(), 15.0, Color32::from_rgb(20, 20, 34));
            p.circle_stroke(r.center(), 15.0, Stroke::new(1.6, ACCENT));
            icon_play(&p, r.center(), 15.0, ACCENT2);
            let mut job = egui::text::LayoutJob::default();
            let f = FontId::proportional(25.0);
            job.append("ART", 0.0, egui::TextFormat { font_id: f.clone(), color: ACCENT, ..Default::default() });
            job.append("IX", 0.0, egui::TextFormat { font_id: f, color: ACCENT2, ..Default::default() });
            ui.add_space(2.0);
            ui.label(job);
        });
        ui.label(RichText::new("L A U N C H E R").size(10.5).color(DIM));
        ui.add_space(20.0);
        let icons: [fn(&egui::Painter, egui::Pos2, f32, Color32); 6] = [icon_play, icon_grid, icon_palette, icon_person, icon_gear, icon_console];
        for ((p, n), icon) in [(Page::Play, "Play"), (Page::Instances, "Instances"), (Page::Skins, "Skins"), (Page::Account, "Account"), (Page::Settings, "Settings"), (Page::Console, "Console")].into_iter().zip(icons) {
            let sel = self.page == p;
            let w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 38.0), Sense::click());
            let painter = ui.painter();
            if sel {
                painter.rect_filled(rect, 8.0, ACCENT);
                painter.rect_filled(egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 2.0, ACCENT2);
            } else if resp.hovered() {
                painter.rect_filled(rect, 8.0, Color32::from_rgb(28, 28, 46));
            }
            let col = if sel { Color32::from_rgb(10, 10, 18) } else if resp.hovered() { Color32::WHITE } else { Color32::from_gray(190) };
            icon(painter, rect.min + vec2(20.0, rect.height() / 2.0), 15.0, col);
            painter.text(rect.min + vec2(38.0, rect.height() / 2.0), Align2::LEFT_CENTER, n, FontId::proportional(15.0), col);
            if resp.clicked() {
                self.page = p;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
            let p = ui.painter();
            p.rect_filled(rect, 10.0, if resp.hovered() { Color32::from_rgb(34, 34, 56) } else { CARD });
            let head = egui::Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(32.0, 32.0));
            match &self.head_px {
                Some(px) => draw_head(p, head, px),
                None => {
                    p.rect_filled(head, 4.0, Color32::from_rgb(50, 50, 80));
                }
            }
            let (name, sub, col) = match self.account() {
                Some(a) => (a.name.clone(), if a.owns { "Minecraft owned" } else { "Not verified" }, if a.owns { Color32::from_rgb(90, 220, 140) } else { Color32::YELLOW }),
                None => ("Not signed in".to_string(), "Open Account", DIM),
            };
            p.text(rect.min + vec2(52.0, 11.0), Align2::LEFT_TOP, name, FontId::proportional(14.5), Color32::WHITE);
            p.text(rect.min + vec2(52.0, 29.0), Align2::LEFT_TOP, sub, FontId::proportional(12.0), col);
            if resp.clicked() {
                self.page = Page::Account;
            }
        });
    }

    // ------------------------------------------------------------------ Play
    fn page_play(&mut self, ui: &mut egui::Ui) {
        ui.heading("Play");
        ui.add_space(6.0);
        if self.insts.is_empty() {
            card(ui, false, |ui| {
                ui.label(RichText::new("No instances yet").size(18.0).strong());
                ui.label(RichText::new("Instances are separate game folders, kept away from your normal .minecraft.").color(DIM));
                ui.add_space(8.0);
                if ui.add(primary("Create your first instance")).clicked() {
                    self.draft.open = true;
                }
            });
            return;
        }
        let artix_on = self.insts.get(self.sel).map(|i| i.loader == "fabric").unwrap_or(false) && mods::artix_status(&self.st).found;
        let list_h = (ui.available_height() - 150.0).max(120.0);
        egui::ScrollArea::vertical().max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
            for i in 0..self.insts.len() {
                let inst = self.insts[i].clone();
                let r = card(ui, i == self.sel, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (badge, bcol) = if inst.loader == "fabric" { ("FABRIC", ACCENT2) } else { ("VANILLA", DIM) };
                        let (brect, _) = ui.allocate_exact_size(vec2(64.0, 40.0), Sense::hover());
                        ui.painter().rect_filled(brect, 8.0, Color32::from_rgba_unmultiplied(bcol.r(), bcol.g(), bcol.b(), 36));
                        ui.painter().text(brect.center(), Align2::CENTER_CENTER, badge, FontId::proportional(11.5), bcol);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&inst.name).size(17.0).strong());
                            ui.horizontal(|ui| {
                                let loader = if inst.loader == "fabric" { "Fabric" } else { "Vanilla" };
                                let chips = [format!("MC {}", inst.mc), loader.to_string(), fmt_gb(inst.ram_max), ago(inst.last_played)];
                                for (n, chip) in chips.iter().enumerate() {
                                    if n > 0 {
                                        ui.label(RichText::new("-").color(Color32::from_gray(60)));
                                    }
                                    ui.label(RichText::new(chip).size(12.5).color(DIM));
                                }
                            });
                        });
                    });
                });
                if r.interact(Sense::click()).clicked() {
                    self.sel = i;
                }
                ui.add_space(3.0);
            }
        });
        ui.add_space(12.0);
        card(ui, false, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    match &self.busy {
                        Some((t, f)) => {
                            ui.add(egui::ProgressBar::new(*f).desired_width(ui.available_width() - 230.0).fill(ACCENT2).text(t.clone()));
                        }
                        None if self.running => {
                            ui.label(RichText::new("Minecraft is running").strong().color(GOOD));
                        }
                        None => {
                            let who = self.account().map(|a| a.name.clone()).unwrap_or_else(|| "no account".into());
                            ui.label(RichText::new(format!("Playing as {who}")).color(DIM));
                            ui.horizontal(|ui| {
                                if self.account().map(|a| a.owns).unwrap_or(false) {
                                    ui.checkbox(&mut self.offline, "Offline mode");
                                }
                                if artix_on {
                                    ui.add_space(8.0);
                                    ui.label(RichText::new("* Artix Client ready").size(12.0).color(ACCENT));
                                }
                            });
                        }
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can = self.busy.is_none() && !self.running;
                    let (rect, resp) = ui.allocate_exact_size(vec2(210.0, 56.0), if can { Sense::click() } else { Sense::hover() });
                    let p = ui.painter();
                    let hov = resp.hovered() && can;
                    let base = if can { ACCENT } else { Color32::from_gray(66) };
                    if hov {
                        for i in (1..=3).rev() {
                            p.rect_filled(rect.expand(i as f32 * 2.2), 14.0, Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 16));
                        }
                    }
                    p.rect_filled(rect, 12.0, base);
                    if can {
                        // soft highlight, top-left, and an accent2 strip along the bottom - no hard seams
                        p.circle_filled(rect.min + vec2(28.0, 10.0), 34.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
                        p.rect_filled(egui::Rect::from_min_max(rect.min + vec2(10.0, rect.height() - 5.0), rect.max - vec2(10.0, 2.0)), 3.0, ACCENT2);
                    }
                    p.rect_stroke(rect, 12.0, Stroke::new(1.0, Color32::from_black_alpha(60)));
                    let ty = rect.center().y - if hov { 1.0 } else { 0.0 };
                    p.text(pos2(rect.center().x, ty), Align2::CENTER_CENTER, "PLAY", FontId::proportional(22.0), Color32::from_rgb(8, 8, 14));
                    if resp.clicked() {
                        self.play();
                    }
                });
            });
        });
    }

    // ------------------------------------------------------------------ Instances
    fn page_instances(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Instances");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(primary("New instance")).clicked() {
                    self.draft.open = true;
                }
            });
        });
        if self.insts.is_empty() {
            ui.label(RichText::new("Nothing here yet. Press New instance.").color(DIM));
            return;
        }
        let h = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.allocate_ui(vec2(210.0, h), |ui| {
                egui::ScrollArea::vertical().id_salt("ilist").show(ui, |ui| {
                    for i in 0..self.insts.len() {
                        let sel = i == self.sel;
                        let b = egui::Button::new(RichText::new(&self.insts[i].name).color(if sel { Color32::BLACK } else { Color32::WHITE })).fill(if sel { ACCENT } else { CARD }).min_size(vec2(196.0, 34.0));
                        if ui.add(b).clicked() {
                            self.sel = i;
                            self.confirm_delete = false;
                        }
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| self.instance_editor(ui));
        });
    }

    fn instance_editor(&mut self, ui: &mut egui::Ui) {
        let max_ram = self.max_ram_slider();
        let rec = self.recommended_ram();
        let total = self.total_ram;
        let sel = self.sel;
        let mut changed = false;
        let mut action = 0; // 1 open, 2 reinstall, 3 duplicate, 4 delete, 5 mods, 6 mods folder, 7 fabric, 8 resync artix, 9 best performance
        let artix = mods::artix_status(&self.st);
        let (artix_found, artix_kb) = (artix.found, artix.size_kb);
        let total_ram = self.total_ram;
        let mod_sel = &mut self.mod_sel;
        let Some(inst) = self.insts.get_mut(sel) else { return };
        egui::ScrollArea::vertical().id_salt("iedit").auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("ig").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                ui.label("Name");
                changed |= ui.add(egui::TextEdit::singleline(&mut inst.name).desired_width(280.0)).changed();
                ui.end_row();
                ui.label("Version");
                ui.label(RichText::new(format!("{}  ({})", inst.mc, if inst.loader == "fabric" { format!("Fabric {}", inst.loader_ver) } else { "Vanilla".into() })).color(DIM));
                ui.end_row();
                ui.label("Max memory");
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::Slider::new(&mut inst.ram_max, 512..=max_ram).step_by(256.0).show_value(false)).changed();
                    ui.label(fmt_gb(inst.ram_max));
                });
                ui.end_row();
                ui.label("Start memory");
                ui.horizontal(|ui| {
                    changed |= ui.add(egui::Slider::new(&mut inst.ram_min, 256..=max_ram).step_by(256.0).show_value(false)).changed();
                    ui.label(fmt_gb(inst.ram_min));
                });
                ui.end_row();
                ui.label("");
                ui.label(RichText::new(format!("Your PC has {} of RAM. Recommended: {}.", fmt_gb(total as u32), fmt_gb(rec))).small().color(DIM));
                ui.end_row();
                ui.label("Resolution");
                ui.horizontal(|ui| {
                    changed |= ui.add_enabled(!inst.fullscreen, egui::DragValue::new(&mut inst.width).range(320..=7680).suffix(" w")).changed();
                    changed |= ui.add_enabled(!inst.fullscreen, egui::DragValue::new(&mut inst.height).range(240..=4320).suffix(" h")).changed();
                    changed |= ui.checkbox(&mut inst.fullscreen, "Fullscreen").changed();
                });
                ui.end_row();
                ui.label("Memory mode");
                let gc_opts = ["Default", "Low memory (Serial GC)", "Smooth (tuned G1)", "Ultimate (best performance)"];
                egui::ComboBox::from_id_salt("gc").selected_text(gc_opts[inst.gc.min(3) as usize]).show_ui(ui, |ui| {
                    for (n, t) in gc_opts.iter().enumerate() {
                        changed |= ui.selectable_value(&mut inst.gc, n as u8, *t).changed();
                    }
                });
                ui.end_row();
                ui.label("");
                changed |= ui.checkbox(&mut inst.fixed_mem, "Fixed memory (start = max, avoids mid-game heap resizing)").changed();
                ui.end_row();
                ui.label("Extra JVM args");
                changed |= ui.add(egui::TextEdit::singleline(&mut inst.jvm).desired_width(280.0).hint_text("optional")).changed();
                ui.end_row();
                ui.label("Java path");
                changed |= ui.add(egui::TextEdit::singleline(&mut inst.java).desired_width(280.0).hint_text("empty = Mojang's Java, downloaded for you")).changed();
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Open folder").clicked() { action = 1; }
                if ui.button("Verify / reinstall files").clicked() { action = 2; }
                if ui.button("Duplicate").clicked() { action = 3; }
                if inst.loader != "fabric" && ui.button("Switch to Fabric").clicked() { action = 7; }
                if ui.button("Best performance preset").on_hover_text("Ultimate GC flags, fixed memory, high RAM allocation").clicked() { action = 9; }
            });
            ui.add_space(6.0);
            if inst.loader == "fabric" {
                ui.separator();
                ui.horizontal(|ui| {
                    let (dot, txt, col) = if artix_found { ("\u{25cf}", format!("Artix Client ready ({artix_kb} KB)"), GOOD) } else { ("\u{25cf}", "Artix Client jar not found - set its path in Settings".to_string(), Color32::YELLOW) };
                    ui.colored_label(col, dot);
                    ui.label(RichText::new(txt).color(if artix_found { Color32::WHITE } else { Color32::YELLOW }));
                    if artix_found && ui.small_button("Resync now").clicked() { action = 8; }
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Performance mods").strong().size(16.0));
                ui.label(RichText::new("Downloaded from Modrinth for this exact Minecraft version.").small().color(DIM));
                for (i, (_, label)) in mods::CATALOG.iter().enumerate() {
                    ui.checkbox(&mut mod_sel[i], *label);
                }
                ui.horizontal(|ui| {
                    if ui.add(primary("Install selected")).clicked() { action = 5; }
                    if ui.button("Open mods folder").clicked() { action = 6; }
                });
            }
            ui.add_space(10.0);
            ui.separator();
            let id = inst.id.clone();
            let del = if self.confirm_delete { "Click again to delete forever" } else { "Delete instance" };
            let _ = id;
            if ui.add(egui::Button::new(RichText::new(del).color(Color32::from_rgb(255, 110, 110)))).clicked() {
                if self.confirm_delete { action = 4; } else { self.confirm_delete = true; }
            }
        });
        if inst.ram_min > inst.ram_max {
            inst.ram_min = inst.ram_max;
        }
        let inst = inst.clone();
        if changed {
            crate::store::save_instance(&self.paths, &inst);
        }
        match action {
            1 => sys::open_path(&self.paths.game_dir(&inst.id)),
            2 => {
                let _ = std::fs::remove_file(self.paths.inst_dir(&inst.id).join(".installed"));
                self.start_install(inst);
            }
            3 => self.duplicate_selected(),
            4 => {
                self.confirm_delete = false;
                self.delete_selected();
            }
            5 => {
                let slugs: Vec<String> = mods::CATALOG.iter().enumerate().filter(|(i, _)| self.mod_sel[*i]).map(|(_, (s, _))| s.to_string()).collect();
                self.start_mods(inst, slugs);
            }
            6 => sys::open_path(&self.paths.game_dir(&inst.id).join("mods")),
            7 => {
                let mut i2 = inst;
                i2.loader = "fabric".into();
                i2.loader_ver.clear();
                let _ = std::fs::remove_file(self.paths.inst_dir(&i2.id).join(".installed"));
                crate::store::save_instance(&self.paths, &i2);
                self.insts[self.sel] = i2.clone();
                self.start_install(i2);
            }
            8 => match mods::sync_artix(&self.paths.game_dir(&inst.id), &self.st) {
                Ok(Some(_)) => self.toast("Artix Client re-synced", false),
                Ok(None) => self.toast("No Artix Client jar configured (see Settings)", true),
                Err(e) => self.toast(e, true),
            },
            9 => {
                let mut i2 = inst;
                i2.gc = 3;
                i2.fixed_mem = true;
                i2.ram_max = ((total_ram * 3 / 4) as u32).clamp(1024, 16384) / 256 * 256;
                i2.ram_min = i2.ram_max;
                crate::store::save_instance(&self.paths, &i2);
                self.insts[self.sel] = i2;
                self.toast("Best performance preset applied", false);
            }
            _ => {}
        }
    }

    fn new_instance_window(&mut self, ctx: &egui::Context) {
        if !self.draft.open {
            return;
        }
        let mut open = true;
        let mut create = false;
        let snaps = self.st.show_snapshots;
        egui::Window::new("New instance").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Grid::new("nw").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.draft.name).hint_text("My instance").desired_width(240.0));
                ui.end_row();
                ui.label("Minecraft");
                let vs: Vec<String> = self.versions.iter().filter(|v| snaps || v.kind == "release").map(|v| v.id.clone()).collect();
                egui::ComboBox::from_id_salt("ver").selected_text(if self.draft.mc.is_empty() { "loading..." } else { &self.draft.mc }).width(240.0).show_ui(ui, |ui| {
                    for v in vs {
                        ui.selectable_value(&mut self.draft.mc, v.clone(), v);
                    }
                });
                ui.end_row();
                ui.label("");
                let mut s = self.st.show_snapshots;
                if ui.checkbox(&mut s, "Show snapshots and old versions").changed() {
                    self.st.show_snapshots = s;
                    crate::store::save_settings(&self.st);
                }
                ui.end_row();
                ui.label("Loader");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.draft.fabric, false, "Vanilla");
                    ui.radio_value(&mut self.draft.fabric, true, "Fabric (mods)");
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Files download automatically, including Java. Your normal .minecraft is never touched.").small().color(DIM));
            ui.add_space(6.0);
            if ui.add_enabled(!self.draft.mc.is_empty(), primary("Create and install")).clicked() {
                create = true;
            }
        });
        if create {
            self.create_instance();
        } else if !open {
            self.draft.open = false;
        }
    }

    // ------------------------------------------------------------------ Skins
    fn page_skins(&mut self, ui: &mut egui::Ui) {
        ui.heading("Skin editor");
        let capes: Vec<(String, String)> = self
            .account()
            .and_then(|a| self.profiles.get(&a.uuid))
            .and_then(|p| p["capes"].as_array())
            .map(|c| c.iter().filter_map(|x| Some((x["id"].as_str()?.to_string(), x["alias"].as_str().unwrap_or("Cape").to_string()))).collect())
            .unwrap_or_default();
        let signed = self.account().is_some();
        let action = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.skin.ui(ui, &capes, signed)).inner;
        match action {
            SkinAction::None => {}
            SkinAction::LoadAccount => {
                if let Some(p) = self.account().and_then(|a| self.profiles.get(&a.uuid)).cloned() {
                    self.skin_task(move || skin::from_profile(&p));
                } else {
                    self.toast("Sign in first (Account tab).", true);
                }
            }
            SkinAction::LoadUser(n) => self.skin_task(move || skin::from_username(&n)),
            SkinAction::Open => match sys::pick_png() {
                Some(path) => match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| skin::decode_png(&b)).and_then(|(w, h, px)| Ok(skin::Skin { rgba: skin::to_64(w, h, px)?, slim: self.skin.slim })) {
                    Ok(s) => self.skin.load(s),
                    Err(e) => self.toast(e, true),
                },
                None => self.toast("No file picker on this system. Use 'Import from username' instead.", true),
            },
            SkinAction::Save => {
                if let Some(path) = sys::save_png_dialog("skin.png") {
                    match skin::encode_png(self.skin.rgba(), 64, 64).and_then(|b| std::fs::write(path, b).map_err(|e| e.to_string())) {
                        Ok(()) => self.toast("Saved", false),
                        Err(e) => self.toast(e, true),
                    }
                } else {
                    self.toast("No save dialog on this system.", true);
                }
            }
            SkinAction::Apply => {
                let bytes = skin::encode_png(self.skin.rgba(), 64, 64);
                let slim = self.skin.slim;
                self.account_task("Skin applied to your account", move |a| skin::upload(&a.mc_token, &bytes?, slim));
            }
            SkinAction::Cape(id) => self.account_task("Cape updated", move |a| skin::set_cape(&a.mc_token, id.as_deref())),
        }
    }

    fn skin_task(&mut self, f: impl FnOnce() -> Result<skin::Skin, String> + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(match f() {
                Ok(s) => crate::app::Msg::SkinLoaded(s),
                Err(e) => crate::app::Msg::Error(e),
            });
            ctx.request_repaint();
        });
    }

    /// Runs `f` with a fresh account token on a worker thread.
    fn account_task(&mut self, done: &'static str, f: impl FnOnce(&crate::store::Account) -> Result<(), String> + Send + 'static) {
        let Some(mut a) = self.account().cloned() else {
            self.toast("Sign in first (Account tab).", true);
            return;
        };
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let res = auth::ensure(&mut a).and_then(|p| {
                if let Some(p) = p {
                    let _ = tx.send(crate::app::Msg::AccountUpdated(a.clone(), p));
                }
                f(&a)
            });
            let _ = tx.send(match res {
                Ok(()) => crate::app::Msg::Info(done.into()),
                Err(e) => crate::app::Msg::Error(e),
            });
            ctx.request_repaint();
        });
    }

    // ------------------------------------------------------------------ Account
    fn page_account(&mut self, ui: &mut egui::Ui) {
        ui.heading("Account");
        ui.add_space(4.0);
        if !auth::configured() {
            card(ui, false, |ui| {
                ui.label(RichText::new("Sign-in isn't configured in this build").strong().color(Color32::YELLOW));
                ui.label("Build the launcher with your own Azure app ID (ARTIX_CLIENT_ID). The README explains the 5 minute setup.");
            });
            ui.add_space(8.0);
        }
        let n = self.accts.len();
        for i in 0..n {
            let a = self.accts[i].clone();
            let sel = i == self.st.selected_account;
            card(ui, sel, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&a.name).size(17.0).strong());
                        ui.label(RichText::new(if a.owns { "Minecraft: Java Edition verified with Mojang" } else { "Ownership not verified" }).color(if a.owns { Color32::from_rgb(90, 220, 140) } else { Color32::YELLOW }));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Remove").clicked() {
                            self.accts.remove(i);
                            self.st.selected_account = 0;
                            crate::store::save_accounts(&self.accts);
                            crate::store::save_settings(&self.st);
                            return;
                        }
                        if !sel && ui.button("Use").clicked() {
                            self.st.selected_account = i;
                            crate::store::save_settings(&self.st);
                            self.head_px = None;
                            self.verify_account();
                        }
                    });
                });
            });
            ui.add_space(4.0);
            if self.accts.len() != n {
                break;
            }
        }
        ui.add_space(6.0);
        if self.signing_in {
            card(ui, true, |ui| match self.device.clone() {
                None => {
                    ui.label("Contacting Microsoft...");
                }
                Some(dc) => {
                    ui.label("1. Open this page and enter the code:");
                    ui.label(RichText::new(&dc.uri).color(ACCENT));
                    ui.label(RichText::new(&dc.user_code).size(34.0).strong().monospace().color(Color32::WHITE));
                    ui.horizontal(|ui| {
                        if ui.add(primary("Copy code and open page")).clicked() {
                            ui.ctx().copy_text(dc.user_code.clone());
                            sys::open_url(&dc.uri);
                        }
                        if ui.button("Cancel").clicked() {
                            self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                    ui.label(RichText::new("Waiting for you to finish signing in...").color(DIM));
                }
            });
        } else if ui.add_enabled(auth::configured(), primary("Sign in with Microsoft")).clicked() {
            self.start_sign_in();
        }
        ui.add_space(14.0);
        ui.label(RichText::new("Privacy").strong());
        ui.label(RichText::new("You sign in directly with Microsoft, Xbox and Mojang using their official sign-in. This launcher has no server: nothing is sent to its developer. Tokens are stored only on this PC, in the launcher's data folder.").color(DIM));
    }

    // ------------------------------------------------------------------ Settings
    fn page_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.add_space(6.0);
        let mut changed = false;
        ui.label(RichText::new("Instances folder").strong());
        ui.label(RichText::new("Where your instances and worlds are stored. Never your .minecraft folder.").small().color(DIM));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.inst_dir_edit).desired_width(360.0).hint_text(self.paths.instances.to_string_lossy().to_string()));
            if ui.button("Browse").clicked() {
                if let Some(p) = sys::pick_folder() {
                    self.inst_dir_edit = p.to_string_lossy().to_string();
                }
            }
            if ui.button("Apply").clicked() {
                self.apply_instances_dir();
            }
            if ui.button("Open").clicked() {
                sys::open_path(&self.paths.instances);
            }
        });
        ui.add_space(10.0);
        ui.label(RichText::new("Default memory for new instances").strong());
        let max = self.max_ram_slider();
        if self.st.default_ram_mb == 0 {
            self.st.default_ram_mb = self.recommended_ram();
        }
        ui.horizontal(|ui| {
            changed |= ui.add(egui::Slider::new(&mut self.st.default_ram_mb, 512..=max).step_by(256.0).show_value(false)).changed();
            ui.label(fmt_gb(self.st.default_ram_mb));
        });
        ui.add_space(10.0);
        changed |= ui.checkbox(&mut self.st.close_on_launch, "Close the launcher when the game starts (saves RAM)").changed();
        ui.label(RichText::new("Game output is written to launcher-output.txt in the instance folder.").small().color(DIM));
        changed |= ui.checkbox(&mut self.st.show_snapshots, "Show snapshots and old versions").changed();
        ui.add_space(10.0);
        ui.label(RichText::new("Java path for all instances").strong());
        changed |= ui.add(egui::TextEdit::singleline(&mut self.st.java_path).desired_width(360.0).hint_text("empty = Mojang's Java, downloaded for you")).changed();
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(RichText::new("Artix Client").strong());
        let status = mods::artix_status(&self.st);
        ui.label(RichText::new(match &status.path {
            Some(p) if status.found => format!("Found: {} ({} KB)", p.display(), status.size_kb),
            _ => "Not found yet. Point this at your built artix-client jar, or drop it next to the launcher exe as ArtixClient.jar.".into(),
        }).small().color(if status.found { GOOD } else { DIM }));
        ui.horizontal(|ui| {
            changed |= ui.add(egui::TextEdit::singleline(&mut self.st.artix_jar).desired_width(360.0).hint_text("optional - auto-detected next to the launcher if empty")).changed();
            if ui.button("Browse").clicked() {
                if let Some(p) = sys::pick_file("Jar file", &["jar"]) {
                    self.st.artix_jar = p.to_string_lossy().to_string();
                    changed = true;
                } else {
                    self.toast("No file picker on this system. Type the path instead.", true);
                }
            }
        });
        ui.label(RichText::new("Installed automatically into every Fabric instance, and refreshed each time you press Play - so a freshly built jar is always used.").small().color(DIM));
        ui.add_space(14.0);
        ui.separator();
        ui.label(RichText::new(format!("Artix Launcher {}  -  {} RAM detected  -  data: {}", env!("CARGO_PKG_VERSION"), fmt_gb(self.total_ram as u32), self.paths.root.display())).small().color(DIM));
        if ui.button("Open launcher data folder").clicked() {
            sys::open_path(&self.paths.root);
        }
        if changed {
            crate::store::save_settings(&self.st);
        }
    }

    // ------------------------------------------------------------------ Console
    fn page_console(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Console");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    self.log.clear();
                }
                if ui.button("Copy").clicked() {
                    let t: Vec<&str> = self.log.iter().map(|s| s.as_str()).collect();
                    ui.ctx().copy_text(t.join("\n"));
                }
            });
        });
        if self.log.is_empty() {
            ui.label(RichText::new("Game output appears here when 'Close the launcher when the game starts' is off.").color(DIM));
        }
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            for l in &self.log {
                ui.label(RichText::new(l).monospace().size(12.5));
            }
        });
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(t) = &self.toast {
            if now > t.until {
                self.toast = None;
                return;
            }
            egui::Area::new(egui::Id::new("toast")).anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -18.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                egui::Frame::none()
                    .fill(if t.err { Color32::from_rgb(70, 24, 34) } else { Color32::from_rgb(20, 56, 48) })
                    .stroke(Stroke::new(1.0, if t.err { Color32::from_rgb(255, 100, 120) } else { Color32::from_rgb(90, 220, 140) }))
                    .rounding(10.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(RichText::new(&t.text).color(Color32::WHITE));
                    });
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let _ = pos2(0.0, 0.0);
    }
}
