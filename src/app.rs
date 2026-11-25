//! Janela do app: alterna entre a galeria e o editor.

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use eframe::egui::{self, ViewportCommand};

use crate::editor::{self, Editor, EditorEvent};
use crate::gallery::{Gallery, GalleryEvent};
use crate::stitch::Frame;

/// Por onde o app começa.
pub enum Start {
    Editor(Frame, PathBuf),
    Gallery,
}

enum Screen {
    Gallery(Gallery),
    Editor(Box<Editor>),
}

/// Para onde ir depois de confirmar o que fazer com anotações não salvas.
#[derive(Clone, Copy)]
enum Leave {
    Close,
    ToGallery,
}

struct App {
    dir: PathBuf,
    screen: Screen,
    leave: Option<Leave>,
}

pub fn run(start: Start, dir: PathBuf) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("scrollshot")
            .with_inner_size([1280.0, 900.0])
            .with_maximized(true),
        ..Default::default()
    };
    eframe::run_native(
        "scrollshot",
        options,
        Box::new(move |cc| {
            editor::install_fonts(&cc.egui_ctx);
            let screen = match start {
                Start::Editor(frame, path) => {
                    Screen::Editor(Box::new(Editor::new(&cc.egui_ctx, frame, path)))
                }
                Start::Gallery => Screen::Gallery(Gallery::new(&cc.egui_ctx, dir.clone())),
            };
            let app = App {
                dir,
                screen,
                leave: None,
            };
            app.update_title(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow!("falha ao abrir a janela: {e}"))
}

impl App {
    fn update_title(&self, ctx: &egui::Context) {
        let title = match &self.screen {
            Screen::Gallery(_) => format!("scrollshot: galeria ({})", self.dir.display()),
            Screen::Editor(e) => format!("scrollshot: {}", e.path().display()),
        };
        ctx.send_viewport_cmd(ViewportCommand::Title(title));
    }

    fn go_gallery(&mut self, ctx: &egui::Context) {
        self.screen = Screen::Gallery(Gallery::new(ctx, self.dir.clone()));
        self.update_title(ctx);
    }

    fn open(&mut self, ctx: &egui::Context, path: PathBuf) -> Result<()> {
        let frame = crate::load_png(&path)?;
        self.screen = Screen::Editor(Box::new(Editor::new(ctx, frame, path)));
        self.update_title(ctx);
        Ok(())
    }

    fn finish_leave(&mut self, ctx: &egui::Context, leave: Leave) {
        match leave {
            Leave::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
            Leave::ToGallery => self.go_gallery(ctx),
        }
    }

    /// Pergunta o que fazer com as anotações não salvas antes de sair do editor.
    fn leave_dialog(&mut self, ctx: &egui::Context) {
        let (Some(leave), Screen::Editor(ed)) = (self.leave, &mut self.screen) else {
            return;
        };
        let mut decided: Option<bool> = None; // Some(true) = pode sair
        egui::Modal::new(egui::Id::new("sair")).show(ctx, |ui| {
            ui.heading("Salvar as anotações?");
            ui.label(
                "As anotações se perdem se você descartar. A captura original continua salva.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Salvar").clicked() {
                    ed.save();
                    decided = Some(!ed.is_dirty());
                }
                if ui.button("Descartar").clicked() {
                    ed.discard();
                    decided = Some(true);
                }
                if ui.button("Cancelar").clicked() {
                    decided = Some(false);
                }
            });
        });
        match decided {
            Some(true) => {
                self.leave = None;
                self.finish_leave(ctx, leave);
            }
            Some(false) => self.leave = None,
            None => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested())
            && let Screen::Editor(ed) = &self.screen
            && ed.is_dirty()
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.leave = Some(Leave::Close);
        }

        let mut open = None;
        match &mut self.screen {
            Screen::Gallery(g) => {
                if let Some(GalleryEvent::Open(p)) = g.ui(ui) {
                    open = Some(p);
                }
            }
            Screen::Editor(ed) => {
                if ed.ui(ui) == Some(EditorEvent::Gallery) {
                    if ed.is_dirty() {
                        self.leave = Some(Leave::ToGallery);
                    } else {
                        self.go_gallery(&ctx);
                    }
                }
            }
        }
        if let Some(p) = open
            && let Err(e) = self.open(&ctx, p)
        {
            eprintln!("erro ao abrir: {e:#}");
        }
        self.leave_dialog(&ctx);
    }
}
