//! `conduit-preview` — a keyboard-driven window showing images sent by the runner on stdin
//! (see `conduit-ipc`). Exits when stdin closes.

mod view;

use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use conduit_ipc::{Msg, read_msg};
use egui::{Color32, Key, Rect, Sense, TextureHandle, TextureOptions, Vec2, pos2, vec2};

use view::Zoom;

const PAN_STEP: f32 = 60.0;
const STATUS_DURATION: Duration = Duration::from_secs(4);
const KEY_HINTS: &str = "+/- zoom   0 1:1   f fit   arrows pan   s save   q quit";

#[derive(Parser)]
#[command(
    name = "conduit-preview",
    version,
    about = "Preview window for Conduit Studio"
)]
struct Args {
    /// Id of the preview node this window belongs to (used in saved file names).
    #[arg(long, default_value = "preview")]
    node_id: String,
    /// Window title; defaults to the node id.
    #[arg(long)]
    title: Option<String>,
    /// Folder that `s` saves PNGs into.
    #[arg(long, default_value = ".")]
    save_dir: PathBuf,
}

struct Frame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// State written by the stdin reader thread and picked up by the UI.
#[derive(Default)]
struct Incoming {
    frame: Option<Frame>,
    error: Option<String>,
}

fn main() -> eframe::Result {
    let args = Args::parse();
    let title = format!(
        "{} — Conduit Preview",
        args.title.as_deref().unwrap_or(&args.node_id)
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_app_id("conduit-preview")
            .with_inner_size([960.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(|cc| {
            let incoming = Arc::new(Mutex::new(Incoming::default()));
            spawn_reader(cc.egui_ctx.clone(), incoming.clone());
            Ok(Box::new(PreviewApp::new(args, incoming)))
        }),
    )
}

fn spawn_reader(ctx: egui::Context, incoming: Arc<Mutex<Incoming>>) {
    std::thread::spawn(move || {
        let mut stdin = BufReader::new(std::io::stdin().lock());
        loop {
            let msg = read_msg(&mut stdin);
            {
                let mut inc = incoming.lock().expect("lock poisoned");
                match msg {
                    Ok(Some(Msg::Frame {
                        width,
                        height,
                        rgba,
                    })) => {
                        inc.frame = Some(Frame {
                            width,
                            height,
                            rgba,
                        });
                        inc.error = None;
                    }
                    Ok(Some(Msg::Error(e))) => inc.error = Some(e),
                    Ok(Some(Msg::Close)) | Ok(None) => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        break;
                    }
                    Err(e) => {
                        inc.error = Some(format!("lost connection to runner: {e}"));
                        break;
                    }
                }
            }
            ctx.request_repaint();
        }
        ctx.request_repaint();
    });
}

struct PreviewApp {
    args: Args,
    incoming: Arc<Mutex<Incoming>>,
    frame: Option<Frame>,
    texture: Option<TextureHandle>,
    error: Option<String>,
    zoom: Zoom,
    /// Scale used in the last paint, so zoom steps start from what's on screen.
    last_scale: f32,
    status: Option<(String, Instant)>,
}

impl PreviewApp {
    fn new(args: Args, incoming: Arc<Mutex<Incoming>>) -> Self {
        Self {
            args,
            incoming,
            frame: None,
            texture: None,
            error: None,
            zoom: Zoom::Fit,
            last_scale: 1.0,
            status: None,
        }
    }

    fn take_incoming(&mut self, ctx: &egui::Context) {
        let mut inc = self.incoming.lock().expect("lock poisoned");
        if let Some(frame) = inc.frame.take() {
            let size = [frame.width as usize, frame.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.rgba);
            let options = TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                ..TextureOptions::LINEAR
            };
            match &mut self.texture {
                Some(tex) => tex.set(image, options),
                None => self.texture = Some(ctx.load_texture("preview", image, options)),
            }
            self.frame = Some(frame);
        }
        self.error = inc.error.clone();
    }

    fn handle_keys(&mut self, ui: &mut egui::Ui) -> Vec2 {
        let mut pan = Vec2::ZERO;
        let pressed: Vec<Key> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => Some(*key),
                    _ => None,
                })
                .collect()
        });
        for key in pressed {
            match key {
                Key::Plus | Key::Equals => self.zoom = Zoom::zoom_in(self.last_scale),
                Key::Minus => self.zoom = Zoom::zoom_out(self.last_scale),
                Key::Num0 => self.zoom = Zoom::Scale(1.0),
                Key::F => self.zoom = Zoom::Fit,
                Key::ArrowLeft => pan.x += PAN_STEP,
                Key::ArrowRight => pan.x -= PAN_STEP,
                Key::ArrowUp => pan.y += PAN_STEP,
                Key::ArrowDown => pan.y -= PAN_STEP,
                Key::S => self.save(),
                Key::Q | Key::Escape => ui.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
        }
        pan
    }

    fn save(&mut self) {
        let Some(frame) = &self.frame else {
            self.set_status("nothing to save yet".into());
            return;
        };
        let result = view::save_png(
            &self.args.save_dir,
            &self.args.node_id,
            frame.width,
            frame.height,
            &frame.rgba,
        );
        self.set_status(match result {
            Ok(path) => format!("saved {}", path.display()),
            Err(e) => e,
        });
    }

    fn set_status(&mut self, text: String) {
        self.status = Some((text, Instant::now()));
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        if self
            .status
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > STATUS_DURATION)
        {
            self.status = None;
        }
        egui::Panel::bottom("footer").show(ui, |ui| {
            ui.horizontal(|ui| {
                match &self.frame {
                    Some(f) => ui.monospace(format!(
                        "{}×{}  {:>4.0}%",
                        f.width,
                        f.height,
                        self.last_scale * 100.0
                    )),
                    None => ui.monospace("waiting for image…"),
                };
                if let Some((text, at)) = &self.status {
                    ui.separator();
                    ui.label(text);
                    ui.ctx()
                        .request_repaint_after(STATUS_DURATION.saturating_sub(at.elapsed()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(KEY_HINTS);
                });
            });
        });
    }

    fn error_banner(&self, ui: &mut egui::Ui) {
        let Some(error) = &self.error else { return };
        let frame = egui::Frame::new()
            .fill(Color32::from_rgb(140, 20, 20))
            .inner_margin(8.0);
        egui::Panel::top("error").frame(frame).show(ui, |ui| {
            ui.colored_label(Color32::WHITE, error);
        });
    }

    fn image_view(&mut self, ui: &mut egui::Ui, pan: Vec2) {
        egui::CentralPanel::default().show(ui, |ui| {
            let (Some(tex), Some(frame)) = (&self.texture, &self.frame) else {
                ui.centered_and_justified(|ui| ui.weak("waiting for image…"));
                return;
            };
            let image_size = [frame.width as f32, frame.height as f32];
            let available = ui.available_size();
            let scale = self.zoom.scale(image_size, available.into());
            self.last_scale = scale;
            let shown = vec2(image_size[0], image_size[1]) * scale;

            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                let canvas = shown.max(ui.available_size());
                let (rect, _) = ui.allocate_exact_size(canvas, Sense::hover());
                let image_rect = Rect::from_center_size(rect.center(), shown);
                paint_checkerboard(ui, image_rect);
                ui.painter().image(
                    tex.id(),
                    image_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                if pan != Vec2::ZERO {
                    ui.scroll_with_delta(pan);
                }
            });
        });
    }
}

impl eframe::App for PreviewApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.take_incoming(ui.ctx());
        let pan = self.handle_keys(ui);
        self.footer(ui);
        self.error_banner(ui);
        self.image_view(ui, pan);
    }
}

/// Grey checkerboard behind the image so transparency is visible. Only the visible
/// part is painted, so cost is bounded by window size, not zoom.
fn paint_checkerboard(ui: &egui::Ui, image_rect: Rect) {
    const CELL: f32 = 10.0;
    let visible = image_rect.intersect(ui.clip_rect());
    if !visible.is_positive() {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(visible, 0.0, Color32::from_gray(200));
    let first_col = ((visible.min.x - image_rect.min.x) / CELL).floor() as i64;
    let first_row = ((visible.min.y - image_rect.min.y) / CELL).floor() as i64;
    let last_col = ((visible.max.x - image_rect.min.x) / CELL).ceil() as i64;
    let last_row = ((visible.max.y - image_rect.min.y) / CELL).ceil() as i64;
    for row in first_row..last_row {
        for col in first_col..last_col {
            if (row + col) % 2 == 0 {
                continue;
            }
            let min = image_rect.min + vec2(col as f32, row as f32) * CELL;
            let cell = Rect::from_min_size(min, Vec2::splat(CELL)).intersect(visible);
            painter.rect_filled(cell, 0.0, Color32::from_gray(150));
        }
    }
}
