//! `conduit-picker` — a color picker window for one color param or palette color of a
//! Conduit Studio project. Changes are written to the node file as you go (so the preview
//! follows); `esc` puts the file back as it was. Launched by `conduit edit` (`C`).

mod target;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use conduit_core::{Channel, ColorModel, Rgba, parse_color, to_hex};
use egui::color_picker::{Alpha, color_picker_color32};
use egui::{Color32, Key, RichText, Sense, Stroke, vec2};

use target::Target;

/// How long to wait after a change before writing the file.
const WRITE_DELAY: Duration = Duration::from_millis(100);
const KEY_HINTS: &str =
    "h s l r g b a channel   ←/→ ±1 (shift ±10)   # hex   1–9 swatch   ⏎/q done   esc revert";

#[derive(Parser)]
#[command(
    name = "conduit-picker",
    version,
    about = "Color picker for Conduit Studio"
)]
struct Args {
    /// Project folder.
    dir: PathBuf,
    /// Node id.
    node: String,
    /// The color param, or the palette color's name with `--table`.
    name: String,
    /// The palette's table param (`colors`) when picking a palette color.
    #[arg(long)]
    table: Option<String>,
}

fn main() -> eframe::Result {
    let args = Args::parse();
    let target = Target {
        dir: args.dir,
        node: args.node,
        name: args.name,
        table: args.table,
    };
    let registry = conduit_nodes::registry();
    let loaded = match target.load(&registry) {
        Ok(loaded) => loaded,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let title = format!("{} — Conduit Picker", target.label());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_app_id("conduit-picker")
            .with_inner_size([520.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(move |_| Ok(Box::new(Picker::new(target, loaded)))),
    )
}

struct Picker {
    target: Target,
    original: Rgba<u8>,
    original_text: String,
    swatches: Vec<(String, Rgba<u8>)>,
    model: ColorModel,
    channel: Channel,
    hex: String,
    focus_hex: bool,
    hex_id: Option<egui::Id>,
    /// A change not written yet, and when it happened.
    pending: Option<Instant>,
    status: Option<String>,
}

impl Picker {
    fn new(target: Target, loaded: target::Loaded) -> Self {
        Self {
            target,
            original: loaded.color,
            original_text: loaded.original_text,
            swatches: loaded.swatches,
            model: ColorModel::new(loaded.color),
            channel: Channel::Hue,
            hex: to_hex(loaded.color),
            focus_hex: false,
            hex_id: None,
            pending: None,
            status: None,
        }
    }

    fn changed(&mut self) {
        self.hex = to_hex(self.model.color);
        self.pending = Some(Instant::now());
        // Editing a palette color: its own swatch follows.
        let label = self.target.label();
        if self.target.table.is_some()
            && let Some((_, c)) = self.swatches.iter_mut().find(|(name, _)| *name == label)
        {
            *c = self.model.color;
        }
    }

    fn flush(&mut self) {
        if self.pending.take().is_some()
            && let Err(e) = self.target.write(self.model.color)
        {
            self.status = Some(e);
        }
    }

    /// Keyboard shortcuts, unless a text field has the keyboard.
    fn handle_keys(&mut self, ui: &mut egui::Ui) {
        let typing = self
            .hex_id
            .is_some_and(|id| ui.ctx().memory(|m| m.has_focus(id)));
        if typing {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        let widget_focused = ui.ctx().memory(|m| m.focused()).is_some();
        for event in events {
            match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let step = if modifiers.shift { 10 } else { 1 };
                    match key {
                        Key::H => self.channel = Channel::Hue,
                        Key::S => self.channel = Channel::Saturation,
                        Key::L => self.channel = Channel::Lightness,
                        Key::R => self.channel = Channel::Red,
                        Key::G => self.channel = Channel::Green,
                        Key::B => self.channel = Channel::Blue,
                        Key::A => self.channel = Channel::Alpha,
                        // A focused slider moves itself with the arrows.
                        Key::ArrowRight | Key::ArrowUp if !widget_focused => self.nudge(step),
                        Key::ArrowLeft | Key::ArrowDown if !widget_focused => self.nudge(-step),
                        Key::Enter | Key::Q => {
                            self.flush();
                            ui.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Key::Escape => self.revert(ui),
                        _ => {
                            if let Some(i) = digit(key)
                                && let Some((_, c)) = self.swatches.get(i)
                            {
                                self.model.set(*c);
                                self.changed();
                            }
                        }
                    }
                }
                // The same nudge keys as the terminal editor.
                egui::Event::Text(text) => match text.as_str() {
                    "+" | "=" => self.nudge(1),
                    "-" | "_" => self.nudge(-1),
                    ">" => self.nudge(10),
                    "<" => self.nudge(-10),
                    "}" => self.nudge(50),
                    "{" => self.nudge(-50),
                    "#" => {
                        self.hex = "#".into();
                        self.focus_hex = true;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn nudge(&mut self, delta: i64) {
        self.model.nudge(self.channel, delta);
        self.changed();
    }

    fn revert(&mut self, ui: &egui::Ui) {
        self.pending = None;
        if let Err(e) = self.target.restore(&self.original_text) {
            self.status = Some(e);
            return;
        }
        ui.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn swatch_row(&mut self, ui: &mut egui::Ui) {
        let new = to_color32(self.model.color);
        let old = to_color32(self.original);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 56.0), Sense::hover());
            let middle = rect.center().x;
            let (left, right) = (
                egui::Rect::from_min_max(rect.min, egui::pos2(middle, rect.max.y)),
                egui::Rect::from_min_max(egui::pos2(middle, rect.min.y), rect.max),
            );
            ui.painter().rect_filled(left, 4.0, new);
            ui.painter().rect_filled(right, 4.0, old);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(to_hex(self.model.color))
                        .monospace()
                        .size(18.0),
                );
                ui.weak(format!("was {}", to_hex(self.original)));
            });
        });
    }

    fn hex_field(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("hex");
            let response = ui.add(egui::TextEdit::singleline(&mut self.hex).desired_width(120.0));
            self.hex_id = Some(response.id);
            if std::mem::take(&mut self.focus_hex) {
                response.request_focus();
            }
            if response.lost_focus() {
                match parse_color(&self.hex) {
                    Ok(c) if c != self.model.color => {
                        self.model.set(c);
                        self.changed();
                    }
                    Ok(_) => {}
                    Err(_) if self.hex.trim() == "#" => self.hex = to_hex(self.model.color),
                    Err(e) => {
                        self.status = Some(e);
                        self.hex = to_hex(self.model.color);
                    }
                }
            }
        });
    }

    fn sliders(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().slider_width = 320.0;
        for (group, channels) in [("HSL", Channel::HSLA), ("RGB", Channel::RGBA)] {
            ui.label(RichText::new(group).weak());
            for ch in channels {
                if group == "RGB" && ch == Channel::Alpha {
                    continue; // shown once, under HSL
                }
                let selected = ch == self.channel;
                ui.horizontal(|ui| {
                    let letter = RichText::new(ch.letter()).monospace().strong();
                    let letter = if selected {
                        letter.color(ui.visuals().selection.stroke.color)
                    } else {
                        letter
                    };
                    if ui.label(letter).clicked() {
                        self.channel = ch;
                    }
                    let max = match ch {
                        Channel::Hue => 360.0,
                        Channel::Red | Channel::Green | Channel::Blue => 255.0,
                        _ => 100.0,
                    };
                    let mut value = self.model.value(ch) as f64;
                    let slider = egui::Slider::new(&mut value, 0.0..=max)
                        .step_by(1.0)
                        .fixed_decimals(0);
                    let response = ui.add(slider);
                    if response.changed() {
                        self.channel = ch;
                        self.model.set_position(ch, value / max);
                        self.changed();
                    }
                    if selected {
                        ui.painter().rect_stroke(
                            response.rect.expand(2.0),
                            3.0,
                            Stroke::new(1.0, ui.visuals().selection.stroke.color),
                            egui::StrokeKind::Outside,
                        );
                    }
                });
            }
        }
    }

    fn palette(&mut self, ui: &mut egui::Ui) {
        if self.swatches.is_empty() {
            return;
        }
        ui.separator();
        ui.label(RichText::new("palette colors").weak());
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            for (i, (name, c)) in self.swatches.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
                ui.painter().rect_filled(rect, 4.0, to_color32(*c));
                if i < 9 {
                    ui.painter().text(
                        rect.left_top() + vec2(3.0, 1.0),
                        egui::Align2::LEFT_TOP,
                        (i + 1).to_string(),
                        egui::FontId::monospace(11.0),
                        contrast(*c),
                    );
                }
                if response
                    .on_hover_text(format!("{name}  {}", to_hex(*c)))
                    .clicked()
                {
                    picked = Some(*c);
                }
            }
        });
        if let Some(c) = picked {
            self.model.set(c);
            self.changed();
        }
    }
}

impl eframe::App for Picker {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_keys(ui);
        if ui.input(|i| i.viewport().close_requested()) {
            self.flush();
        }
        egui::Panel::bottom("keys").show(ui, |ui| {
            if let Some(status) = &self.status {
                ui.colored_label(Color32::from_rgb(220, 80, 80), status);
            }
            ui.weak(KEY_HINTS);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(self.target.label());
            self.swatch_row(ui);
            ui.add_space(6.0);
            let mut c32 = to_color32(self.model.color);
            if color_picker_color32(ui, &mut c32, Alpha::OnlyBlend) {
                self.model.set(from_color32(c32));
                self.changed();
            }
            ui.add_space(6.0);
            self.hex_field(ui);
            self.sliders(ui);
            self.palette(ui);
        });
        if let Some(at) = self.pending {
            let waited = at.elapsed();
            if waited >= WRITE_DELAY {
                self.flush();
            } else {
                ui.ctx().request_repaint_after(WRITE_DELAY - waited);
            }
        }
    }
}

fn to_color32(c: Rgba<u8>) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn from_color32(c: Color32) -> Rgba<u8> {
    Rgba(c.to_srgba_unmultiplied())
}

/// Black or white, whichever reads better on `c`.
fn contrast(c: Rgba<u8>) -> Color32 {
    let luma = 0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2]);
    if luma > 140.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// `1`–`9` → swatch index 0–8.
fn digit(key: Key) -> Option<usize> {
    let keys = [
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
    ];
    keys.iter().position(|k| *k == key)
}
