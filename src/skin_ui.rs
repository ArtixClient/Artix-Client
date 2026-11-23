//! Skin editor: 64x64 pixel canvas, front/back preview, tools, undo/redo.
use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, TextureHandle, TextureOptions};

use crate::skin::Skin;

#[derive(PartialEq, Clone, Copy)]
enum Tool {
    Pencil,
    Eraser,
    Fill,
    Picker,
}

pub enum SkinAction {
    None,
    LoadAccount,
    LoadUser(String),
    Apply,
    Open,
    Save,
    Cape(Option<String>),
}

pub struct SkinEditor {
    px: Vec<u8>,
    pub slim: bool,
    tex: Option<TextureHandle>,
    dirty: bool,
    tool: Tool,
    color: Color32,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
    overlay: bool,
    grid: bool,
    pub username: String,
    was_down: bool,
    pub loaded: bool,
}

impl Default for SkinEditor {
    fn default() -> Self {
        SkinEditor { px: vec![0; 64 * 64 * 4], slim: false, tex: None, dirty: true, tool: Tool::Pencil, color: Color32::from_rgb(200, 60, 60), undo: vec![], redo: vec![], overlay: true, grid: true, username: String::new(), was_down: false, loaded: false }
    }
}

impl SkinEditor {
    pub fn load(&mut self, s: Skin) {
        self.px = s.rgba;
        self.slim = s.slim;
        self.dirty = true;
        self.undo.clear();
        self.redo.clear();
        self.loaded = true;
    }

    pub fn set_account_skin(&mut self, px: Vec<u8>, slim: bool) {
        if !self.loaded {
            self.load(Skin { rgba: px, slim });
        }
    }

    pub fn rgba(&self) -> &[u8] {
        &self.px
    }

    fn snapshot(&mut self) {
        self.undo.push(self.px.clone());
        if self.undo.len() > 40 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * 64 + x) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    fn put(&mut self, x: usize, y: usize, c: [u8; 4]) {
        let i = (y * 64 + x) * 4;
        self.px[i..i + 4].copy_from_slice(&c);
        self.dirty = true;
    }

    fn flood(&mut self, x: usize, y: usize, c: [u8; 4]) {
        let target = self.at(x, y);
        if target == c {
            return;
        }
        let mut stack = vec![(x, y)];
        while let Some((cx, cy)) = stack.pop() {
            if self.at(cx, cy) != target {
                continue;
            }
            self.put(cx, cy, c);
            if cx > 0 { stack.push((cx - 1, cy)); }
            if cx < 63 { stack.push((cx + 1, cy)); }
            if cy > 0 { stack.push((cx, cy - 1)); }
            if cy < 63 { stack.push((cx, cy + 1)); }
        }
    }

    fn color_px(&self) -> [u8; 4] {
        let c = self.color.to_srgba_unmultiplied();
        [c[0], c[1], c[2], c[3]]
    }

    fn preview(&self, ui: &mut egui::Ui, back: bool) {
        let s = 6.0;
        let (rect, _) = ui.allocate_exact_size(vec2(16.0 * s, 32.0 * s), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 6.0, Color32::from_rgb(18, 18, 30));
        let arm_w = if self.slim { 3 } else { 4 };
        // (src x, src y, w, h, dst x, dst y)
        let (rarm_b, larm_b, rarm_o, larm_o) = if back { (if self.slim { 51 } else { 52 }, if self.slim { 43 } else { 44 }, if self.slim { 51 } else { 52 }, if self.slim { 59 } else { 60 }) } else { (44, 36, 44, 52) };
        let mut parts: Vec<(usize, usize, usize, usize, i32, i32, bool)> = Vec::new();
        let (head, body, rleg, lleg) = if back { ((24, 8), (32, 20), (12, 20), (28, 52)) } else { ((8, 8), (20, 20), (4, 20), (20, 52)) };
        let (head_o, body_o, rleg_o, lleg_o) = if back { ((56, 8), (32, 36), (12, 36), (12, 52)) } else { ((40, 8), (20, 36), (4, 36), (4, 52)) };
        // viewer-left arm is the character's right arm from the front, left arm from the back
        let (lx, rx) = (if self.slim { 1 } else { 0 }, 12);
        for (overlay, h, b, rl, ll, ra, la) in [
            (false, head, body, rleg, lleg, (rarm_b, 20), (larm_b, 52)),
            (true, head_o, body_o, rleg_o, lleg_o, (rarm_o, 36), (larm_o, 52)),
        ] {
            if overlay && !self.overlay {
                continue;
            }
            parts.push((h.0, h.1, 8, 8, 4, 0, overlay));
            parts.push((b.0, b.1, 8, 12, 4, 8, overlay));
            let (ra_dx, la_dx) = if back { (rx, lx) } else { (lx, rx) };
            parts.push((ra.0, ra.1, arm_w, 12, ra_dx, 8, overlay));
            parts.push((la.0, la.1, arm_w, 12, la_dx, 8, overlay));
            let (rl_dx, ll_dx) = if back { (8, 4) } else { (4, 8) };
            parts.push((rl.0, rl.1, 4, 12, rl_dx, 20, overlay));
            parts.push((ll.0, ll.1, 4, 12, ll_dx, 20, overlay));
        }
        for (sx, sy, w, h, dx, dy, _) in parts {
            for y in 0..h {
                for x in 0..w {
                    let i = ((sy + y) * 64 + sx + x) * 4;
                    if self.px[i + 3] == 0 {
                        continue;
                    }
                    let c = Color32::from_rgba_unmultiplied(self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]);
                    let r = Rect::from_min_size(rect.min + vec2((dx as usize + x) as f32 * s, (dy as usize + y) as f32 * s), vec2(s, s));
                    p.rect_filled(r, 0.0, c);
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, capes: &[(String, String)], signed_in: bool) -> SkinAction {
        let mut action = SkinAction::None;
        let ctx = ui.ctx().clone();
        if self.dirty || self.tex.is_none() {
            let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], &self.px);
            match &mut self.tex {
                Some(t) => t.set(img, TextureOptions::NEAREST),
                None => self.tex = Some(ctx.load_texture("skin", img, TextureOptions::NEAREST)),
            }
            self.dirty = false;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.do_undo();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Y)) {
            self.do_redo();
        }

        ui.horizontal_top(|ui| {
            // ---- canvas
            ui.vertical(|ui| {
                let zoom = 6.0;
                let (rect, resp) = ui.allocate_exact_size(vec2(64.0 * zoom, 64.0 * zoom), Sense::click_and_drag());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 4.0, Color32::from_gray(24));
                for y in 0..32 {
                    for x in 0..32 {
                        if (x + y) % 2 == 0 {
                            p.rect_filled(Rect::from_min_size(rect.min + vec2(x as f32, y as f32) * zoom * 2.0, vec2(zoom * 2.0, zoom * 2.0)), 0.0, Color32::from_gray(32));
                        }
                    }
                }
                if let Some(t) = &self.tex {
                    p.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                if self.grid {
                    for i in 0..=64 {
                        let c = if i % 8 == 0 { Color32::from_white_alpha(46) } else { Color32::from_white_alpha(10) };
                        let o = i as f32 * zoom;
                        p.line_segment([pos2(rect.min.x + o, rect.min.y), pos2(rect.min.x + o, rect.max.y)], (1.0, c));
                        p.line_segment([pos2(rect.min.x, rect.min.y + o), pos2(rect.max.x, rect.min.y + o)], (1.0, c));
                    }
                }
                let primary = ctx.input(|i| i.pointer.primary_down());
                let secondary = ctx.input(|i| i.pointer.secondary_down());
                let down = (primary || secondary) && resp.is_pointer_button_down_on();
                if down {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let (fx, fy) = ((pos.x - rect.min.x) / zoom, (pos.y - rect.min.y) / zoom);
                        if fx >= 0.0 && fy >= 0.0 && fx < 64.0 && fy < 64.0 {
                            let (x, y) = (fx as usize, fy as usize);
                            let press = !self.was_down;
                            let tool = if secondary && !primary { Tool::Eraser } else { self.tool };
                            match tool {
                                Tool::Pencil | Tool::Eraser => {
                                    if press {
                                        self.snapshot();
                                    }
                                    let c = if tool == Tool::Eraser { [0, 0, 0, 0] } else { self.color_px() };
                                    self.put(x, y, c);
                                }
                                Tool::Fill if press => {
                                    self.snapshot();
                                    let c = self.color_px();
                                    self.flood(x, y, c);
                                }
                                Tool::Picker if press => {
                                    let c = self.at(x, y);
                                    self.color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3].max(1));
                                    self.tool = Tool::Pencil;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                self.was_down = down;
                ui.add_space(4.0);
                ui.weak("Left click paints, right click erases. Ctrl+Z / Ctrl+Y undo / redo.");
            });

            ui.add_space(10.0);

            // ---- side panel
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.weak("Front");
                        self.preview(ui, false);
                    });
                    ui.vertical(|ui| {
                        ui.weak("Back");
                        self.preview(ui, true);
                    });
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for (t, n) in [(Tool::Pencil, "Pencil"), (Tool::Eraser, "Eraser"), (Tool::Fill, "Fill"), (Tool::Picker, "Picker")] {
                        ui.selectable_value(&mut self.tool, t, n);
                    }
                });
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgba(&mut self.color);
                    ui.label("Color");
                });
                ui.checkbox(&mut self.slim, "Slim arms (Alex)");
                ui.checkbox(&mut self.overlay, "Show outer layer in preview");
                ui.checkbox(&mut self.grid, "Pixel grid");
                ui.horizontal(|ui| {
                    if ui.button("Undo").clicked() { self.do_undo(); }
                    if ui.button("Redo").clicked() { self.do_redo(); }
                    if ui.button("Clear").clicked() {
                        self.snapshot();
                        self.px = vec![0; 64 * 64 * 4];
                        self.dirty = true;
                    }
                });
            });
        });

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Load my skin").clicked() { action = SkinAction::LoadAccount; }
            if ui.button("Open PNG").clicked() { action = SkinAction::Open; }
            if ui.button("Save PNG").clicked() { action = SkinAction::Save; }
            ui.add(egui::TextEdit::singleline(&mut self.username).hint_text("username").desired_width(120.0));
            if ui.button("Import from username").clicked() { action = SkinAction::LoadUser(self.username.clone()); }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let apply = egui::Button::new(egui::RichText::new("Apply to my account").strong().color(Color32::BLACK)).fill(Color32::from_rgb(0, 229, 255));
            if ui.add_enabled(signed_in, apply).clicked() { action = SkinAction::Apply; }
            if !capes.is_empty() {
                ui.label("Cape:");
                if ui.button("None").clicked() { action = SkinAction::Cape(None); }
                for (id, name) in capes {
                    if ui.button(name).clicked() { action = SkinAction::Cape(Some(id.clone())); }
                }
            }
        });
        action
    }

    fn do_undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.px, p));
            self.dirty = true;
        }
    }

    fn do_redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.px, p));
            self.dirty = true;
        }
    }
}

/// Draws the 8x8 face (with hat layer) of a skin into `rect`.
pub fn draw_head(p: &egui::Painter, rect: Rect, px: &[u8]) {
    let s = rect.width() / 8.0;
    for layer in [(8usize, 8usize), (40, 8)] {
        for y in 0..8 {
            for x in 0..8 {
                let i = ((layer.1 + y) * 64 + layer.0 + x) * 4;
                if px.len() < i + 4 || px[i + 3] == 0 {
                    continue;
                }
                p.rect_filled(Rect::from_min_size(rect.min + vec2(x as f32 * s, y as f32 * s), vec2(s.ceil(), s.ceil())), 0.0, Color32::from_rgba_unmultiplied(px[i], px[i + 1], px[i + 2], px[i + 3]));
            }
        }
    }
}
