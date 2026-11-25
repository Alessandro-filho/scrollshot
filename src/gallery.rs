//! Galeria das capturas salvas: miniaturas, abrir, copiar e mandar para a lixeira.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::time::SystemTime;

use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Rect, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, pos2, vec2,
};

/// Largura e altura da área da miniatura em cada card.
const THUMB_W: f32 = 240.0;
const THUMB_H: f32 = 260.0;

/// Pedidos da galeria para o app.
pub enum GalleryEvent {
    Open(PathBuf),
}

struct Entry {
    path: PathBuf,
    modified: SystemTime,
    dims: Option<[usize; 2]>,
    thumb: Option<TextureHandle>,
    failed: bool,
}

/// Resultado do carregamento de uma miniatura em segundo plano.
struct Loaded {
    path: PathBuf,
    result: Result<(ColorImage, [usize; 2]), String>,
}

pub struct Gallery {
    dir: PathBuf,
    entries: Vec<Entry>,
    rx: Receiver<Loaded>,
    status: String,
}

/// Capturas da pasta, da mais nova para a mais antiga.
fn scan(dir: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
        .map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Entry {
                path,
                modified,
                dims: None,
                thumb: None,
                failed: false,
            }
        })
        .collect();
    entries.sort_by(|a, b| b.modified.cmp(&a.modified));
    entries
}

/// Lê a imagem e gera a miniatura. Capturas compridas mostram só o topo.
fn make_thumb(path: &Path) -> Result<(ColorImage, [usize; 2]), String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgb8();
    let (w, h) = img.dimensions();
    let max_h = (w as f32 * THUMB_H / THUMB_W) as u32;
    let top = image::imageops::crop_imm(&img, 0, 0, w, h.min(max_h.max(1))).to_image();
    let tw = (THUMB_W as u32).min(w).max(1);
    let th = ((top.height() as f32 * tw as f32 / w as f32).round() as u32).max(1);
    let small = image::imageops::thumbnail(&top, tw, th);
    let color = ColorImage::from_rgb(
        [small.width() as usize, small.height() as usize],
        small.as_raw(),
    );
    Ok((color, [w as usize, h as usize]))
}

impl Gallery {
    pub fn new(ctx: &egui::Context, dir: PathBuf) -> Self {
        let entries = scan(&dir);
        let paths: Vec<PathBuf> = entries.iter().map(|e| e.path.clone()).collect();
        let (tx, rx) = channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            for path in paths {
                let result = make_thumb(&path);
                if tx.send(Loaded { path, result }).is_err() {
                    break; // a galeria foi fechada
                }
                ctx2.request_repaint();
            }
        });
        let status = format!("{} capturas em {}", entries.len(), dir.display());
        Self {
            dir,
            entries,
            rx,
            status,
        }
    }

    fn receive_thumbs(&mut self, ctx: &egui::Context) {
        while let Ok(l) = self.rx.try_recv() {
            let Some(e) = self.entries.iter_mut().find(|e| e.path == l.path) else {
                continue;
            };
            match l.result {
                Ok((img, dims)) => {
                    e.thumb = Some(ctx.load_texture(
                        l.path.display().to_string(),
                        img,
                        TextureOptions::LINEAR,
                    ));
                    e.dims = Some(dims);
                }
                Err(_) => e.failed = true,
            }
        }
    }

    fn trash(&mut self, path: &Path) {
        let ok = Command::new("gio")
            .arg("trash")
            .arg(path)
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            self.entries.retain(|e| e.path != path);
            self.status = format!("{} foi para a lixeira.", file_name(path));
        } else {
            self.status = format!("Não consegui mandar {} para a lixeira.", file_name(path));
        }
    }

    fn copy(&mut self, path: &Path) {
        self.status = match std::fs::read(path)
            .map_err(anyhow::Error::from)
            .and_then(|png| crate::editor::copy_png_bytes(&png))
        {
            Ok(()) => format!("{} copiada para a área de transferência.", file_name(path)),
            Err(e) => format!("Erro ao copiar: {e:#}"),
        };
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<GalleryEvent> {
        let ctx = ui.ctx().clone();
        self.receive_thumbs(&ctx);
        let mut event = None;
        let mut refresh = false;

        egui::Panel::top("galeria_topo").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Galeria");
                ui.separator();
                if ui.button("⟳ Atualizar").clicked() {
                    refresh = true;
                }
                if ui.button("Abrir pasta").clicked() {
                    let _ = Command::new("xdg-open").arg(&self.dir).spawn();
                }
            });
            ui.add_space(2.0);
        });
        egui::Panel::bottom("galeria_status").show(ui, |ui| ui.label(&self.status));

        let mut to_trash = None;
        let mut to_copy = None;
        egui::CentralPanel::default().show(ui, |ui| {
            if self.entries.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(format!(
                        "Nenhuma captura ainda.\nAs capturas novas são salvas em {}",
                        self.dir.display()
                    ));
                });
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                    // Quantos cards cabem por linha (card = miniatura + moldura + espaço).
                    let card_w = THUMB_W + 2.0 * ui.spacing().window_margin.leftf() + 12.0;
                    let cols = ((ui.available_width() / card_w).floor() as usize).max(1);
                    for row in self.entries.chunks(cols) {
                        ui.horizontal(|ui| {
                            for e in row {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.set_width(THUMB_W);
                                    ui.vertical(|ui| {
                                        let (rect, resp) = ui.allocate_exact_size(
                                            vec2(THUMB_W, THUMB_H),
                                            Sense::click(),
                                        );
                                        paint_thumb(ui, rect, e, resp.hovered());
                                        if resp.hovered() {
                                            ctx.set_cursor_icon(CursorIcon::PointingHand);
                                        }
                                        if resp.clicked() {
                                            event = Some(GalleryEvent::Open(e.path.clone()));
                                        }
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(file_name(&e.path)).strong(),
                                            )
                                            .truncate(),
                                        );
                                        let date: chrono::DateTime<chrono::Local> =
                                            e.modified.into();
                                        let dims = e
                                            .dims
                                            .map(|[w, h]| format!(" · {w}×{h}"))
                                            .unwrap_or_default();
                                        ui.weak(format!("{}{dims}", date.format("%d/%m/%Y %H:%M")));
                                        ui.horizontal(|ui| {
                                            if ui.button("Editar").clicked() {
                                                event = Some(GalleryEvent::Open(e.path.clone()));
                                            }
                                            if ui.button("Copiar").clicked() {
                                                to_copy = Some(e.path.clone());
                                            }
                                            if ui
                                                .button("Excluir")
                                                .on_hover_text("Manda para a lixeira")
                                                .clicked()
                                            {
                                                to_trash = Some(e.path.clone());
                                            }
                                        });
                                    });
                                });
                            }
                        });
                    }
                });
        });

        if let Some(p) = to_copy {
            self.copy(&p);
        }
        if let Some(p) = to_trash {
            self.trash(&p);
        }
        if refresh {
            *self = Gallery::new(&ctx, self.dir.clone());
        }
        event
    }
}

fn paint_thumb(ui: &egui::Ui, rect: Rect, e: &Entry, hovered: bool) {
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
    match &e.thumb {
        Some(tex) => {
            // Encaixa mantendo a proporção, alinhada ao topo.
            let [tw, th] = tex.size().map(|v| v as f32);
            let scale = (rect.width() / tw).min(rect.height() / th);
            let size = vec2(tw, th) * scale;
            let r = Rect::from_min_size(pos2(rect.center().x - size.x / 2.0, rect.min.y), size);
            painter.image(
                tex.id(),
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            let msg = if e.failed {
                "não consegui abrir"
            } else {
                "carregando…"
            };
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                msg,
                egui::FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
        }
    }
    if hovered {
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(2.0, ui.visuals().selection.bg_fill),
            StrokeKind::Inside,
        );
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
