//! Janela de edição: anotações sobre a captura, seleção, zoom, salvar e copiar.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, FontFamily, FontId, Key, KeyboardShortcut, Modifiers,
    Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2, vec2,
};

use crate::annot::{self, Annot, HIGHLIGHT_ALPHA, Shape};
use crate::stitch::Frame;

/// Altura de cada pedaço de textura (GPUs antigas limitam texturas a 8192 px).
const TILE_ROWS: usize = 2048;
const ANNOT_FONT: &str = "anot";
const UI_SYMBOLS_FONT: &str = "simbolos";
static UI_SYMBOLS_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
/// Tamanho das alças de redimensionamento, em pixels de tela.
const HANDLE_PX: f32 = 8.0;
/// Folga para acertar uma anotação com o clique, em pixels de tela.
const HIT_PX: f32 = 5.0;

const PALETTE: [[u8; 3]; 8] = [
    [0xe5, 0x39, 0x35], // vermelho
    [0xfb, 0x8c, 0x00], // laranja
    [0xfd, 0xd8, 0x35], // amarelo
    [0x43, 0xa0, 0x47], // verde
    [0x1e, 0x88, 0xe5], // azul
    [0x8e, 0x24, 0xaa], // roxo
    [0x21, 0x21, 0x21], // preto
    [0xff, 0xff, 0xff], // branco
];

const SELECTION_COLOR: Color32 = Color32::from_rgb(0x29, 0x9d, 0xff);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Arrow,
    Rect,
    Highlight,
    Pen,
    Text,
    Number,
    Pixelate,
}

impl Tool {
    const ALL: [Tool; 8] = [
        Tool::Select,
        Tool::Arrow,
        Tool::Rect,
        Tool::Highlight,
        Tool::Pen,
        Tool::Text,
        Tool::Number,
        Tool::Pixelate,
    ];

    fn label(self) -> &'static str {
        match self {
            Tool::Select => "↖ Selecionar",
            Tool::Arrow => "➜ Seta",
            Tool::Rect => "▭ Retângulo",
            Tool::Highlight => "▮ Marca-texto",
            Tool::Pen => "✎ Caneta",
            Tool::Text => "T Texto",
            Tool::Number => "① Número",
            Tool::Pixelate => "▦ Pixelar",
        }
    }

    fn key(self) -> Key {
        match self {
            Tool::Select => Key::V,
            Tool::Arrow => Key::A,
            Tool::Rect => Key::R,
            Tool::Highlight => Key::M,
            Tool::Pen => Key::P,
            Tool::Text => Key::T,
            Tool::Number => Key::N,
            Tool::Pixelate => Key::B,
        }
    }
}

struct Item {
    annot: Annot,
    /// Textura e posição (em coordenadas da imagem) dos tipos desenhados como sprite.
    tex: Option<(TextureHandle, Rect)>,
}

impl Item {
    fn new(annot: Annot) -> Self {
        Self { annot, tex: None }
    }

    fn sprite_rect(&self) -> Option<Rect> {
        self.tex.as_ref().map(|t| t.1)
    }
}

struct TextEditing {
    pos: Pos2,
    text: String,
    focus: bool,
    /// Texto que estava lá antes, quando é uma reedição (volta se cancelar).
    original: Option<Annot>,
}

#[derive(Clone, Copy)]
enum GrabKind {
    Move,
    Handle(usize),
}

/// Arraste em andamento de uma anotação selecionada.
struct Grab {
    kind: GrabKind,
    orig: Annot,
    orig_rect: Option<Rect>,
    start: Pos2,
}

struct Tile {
    tex: TextureHandle,
    y0: f32,
    rows: f32,
}

pub struct Editor {
    base: Frame,
    out: PathBuf,
    tiles: Vec<Tile>,
    items: Vec<Item>,
    /// Estados anteriores e posteriores, para desfazer e refazer.
    history: Vec<Vec<Annot>>,
    future: Vec<Vec<Annot>>,
    tool: Tool,
    color: [u8; 3],
    width: f32,
    text_size: f32,
    boxed: bool,
    draft: Option<Annot>,
    drag_start: Pos2,
    editing: Option<TextEditing>,
    selected: Option<usize>,
    grab: Option<Grab>,
    /// Já guardou o estado para desfazer as mudanças de propriedade desta seleção.
    props_snapshotted: bool,
    zoom: f32,
    fit_pending: bool,
    /// Há anotações que ainda não foram salvas.
    dirty: bool,
    status: String,
    /// Pedido para o app (ex.: voltar à galeria), lido depois de cada quadro.
    event: Option<EditorEvent>,
}

/// Pedidos do editor para o app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    Gallery,
}

/// Registra as fontes das anotações e dos símbolos da interface.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        ANNOT_FONT.into(),
        Arc::new(egui::FontData::from_static(annot::FONT_BYTES)),
    );
    fonts
        .families
        .insert(FontFamily::Name(ANNOT_FONT.into()), vec![ANNOT_FONT.into()]);
    // Reserva para os símbolos da barra (➜ ▭ ✎ ① ↶...), que as fontes do egui não têm.
    fonts.font_data.insert(
        UI_SYMBOLS_FONT.into(),
        Arc::new(egui::FontData::from_static(UI_SYMBOLS_BYTES)),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(UI_SYMBOLS_FONT.into());
    }
    ctx.set_fonts(fonts);
}

const HELP_STATUS: &str = "V seleciona (arraste move, alças redimensionam, Delete apaga, duplo clique edita texto) · A seta · R retângulo · M marca-texto · P caneta · T texto · N número · B pixelar · Ctrl+Z desfaz · Ctrl+S salva · Ctrl+C copia · Ctrl+roda dá zoom";

impl Editor {
    pub fn new(ctx: &egui::Context, base: Frame, out: PathBuf) -> Self {
        let mut tiles = Vec::new();
        for y0 in (0..base.height).step_by(TILE_ROWS) {
            let rows = TILE_ROWS.min(base.height - y0);
            let rgb: Vec<u8> = base.pixels[y0 * base.width..(y0 + rows) * base.width]
                .iter()
                .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
                .collect();
            let img = ColorImage::from_rgb([base.width, rows], &rgb);
            tiles.push(Tile {
                tex: ctx.load_texture(format!("tile{y0}"), img, TextureOptions::LINEAR),
                y0: y0 as f32,
                rows: rows as f32,
            });
        }

        Self {
            base,
            out,
            tiles,
            items: Vec::new(),
            history: Vec::new(),
            future: Vec::new(),
            tool: Tool::Arrow,
            color: PALETTE[0],
            width: 4.0,
            text_size: 22.0,
            boxed: true,
            draft: None,
            drag_start: Pos2::ZERO,
            editing: None,
            selected: None,
            grab: None,
            props_snapshotted: false,
            zoom: 1.0,
            fit_pending: true,
            dirty: false,
            event: None,
            status: HELP_STATUS.into(),
        }
    }

    fn annot(&self, shape: Shape) -> Annot {
        Annot {
            shape,
            color: self.color,
            width: self.width,
        }
    }

    fn annots(&self) -> Vec<Annot> {
        self.items.iter().map(|i| i.annot.clone()).collect()
    }

    /// Guarda o estado atual para poder desfazer a próxima mudança.
    fn snapshot(&mut self) {
        self.history.push(self.annots());
        self.future.clear();
        self.dirty = true;
    }

    fn restore(&mut self, state: Vec<Annot>) {
        self.items = state.into_iter().map(Item::new).collect();
        self.select(None);
        self.grab = None;
        self.dirty = true;
    }

    fn select(&mut self, idx: Option<usize>) {
        self.selected = idx;
        self.props_snapshotted = false;
        // A barra passa a mostrar as propriedades da anotação selecionada.
        if let Some(a) = idx
            .and_then(|i| self.items.get(i))
            .map(|it| it.annot.clone())
        {
            self.color = a.color;
            self.width = a.width;
            match a.shape {
                Shape::Text { size, boxed, .. } => {
                    self.text_size = size;
                    self.boxed = boxed;
                }
                Shape::Number { size, .. } => self.text_size = size,
                _ => {}
            }
        }
    }

    fn push(&mut self, a: Annot) {
        if annot::is_meaningful(&a) {
            self.snapshot();
            self.items.push(Item::new(a));
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.history.pop() {
            self.future.push(self.annots());
            self.restore(prev);
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.future.pop() {
            self.history.push(self.annots());
            self.restore(next);
        }
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.selected.filter(|&i| i < self.items.len()) {
            self.snapshot();
            self.items.remove(i);
            self.select(None);
        }
    }

    /// Aplica as propriedades da barra à anotação selecionada, se mudaram.
    fn apply_props(&mut self, before: ([u8; 3], f32, f32, bool)) {
        let now = (self.color, self.width, self.text_size, self.boxed);
        let Some(i) = self.selected.filter(|&i| i < self.items.len()) else {
            return;
        };
        if now == before {
            return;
        }
        if !self.props_snapshotted {
            self.snapshot();
            self.props_snapshotted = true;
        }
        self.dirty = true;
        let item = &mut self.items[i];
        let a = &mut item.annot;
        a.color = self.color;
        a.width = self.width;
        match &mut a.shape {
            Shape::Text { size, boxed, .. } => {
                *size = self.text_size;
                *boxed = self.boxed;
            }
            Shape::Number { size, .. } => *size = self.text_size,
            _ => {}
        }
        item.tex = None;
    }

    fn rendered(&self) -> Frame {
        annot::render(&self.base, &self.annots())
    }

    pub fn save(&mut self) {
        self.commit_text();
        match crate::save_png(&self.rendered(), &self.out) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("Salvo em {}", self.out.display());
            }
            Err(e) => self.status = format!("Erro ao salvar: {e:#}"),
        }
    }

    fn copy(&mut self) {
        self.commit_text();
        self.status = match copy_to_clipboard(&self.rendered()) {
            Ok(()) => "Imagem copiada para a área de transferência.".into(),
            Err(e) => format!("Erro ao copiar: {e:#}"),
        };
    }

    fn commit_text(&mut self) {
        if let Some(ed) = self.editing.take() {
            let a = self.annot(Shape::Text {
                pos: ed.pos,
                text: ed.text,
                size: self.text_size,
                boxed: self.boxed,
            });
            if annot::is_meaningful(&a) {
                // Na reedição o estado já foi guardado ao abrir a caixa.
                if ed.original.is_none() {
                    self.snapshot();
                }
                self.items.push(Item::new(a));
            }
        }
    }

    fn cancel_text(&mut self) {
        if let Some(original) = self.editing.take().and_then(|ed| ed.original) {
            self.items.push(Item::new(original));
        }
    }

    /// Abre a caixa de digitação sobre um texto existente.
    fn reedit_text(&mut self, i: usize) {
        let Shape::Text { pos, text, .. } = self.items[i].annot.shape.clone() else {
            return;
        };
        self.select(Some(i));
        self.snapshot();
        let original = self.items.remove(i).annot;
        self.select(None);
        self.editing = Some(TextEditing {
            pos,
            text,
            focus: true,
            original: Some(original),
        });
    }

    fn next_number(&self) -> u32 {
        1 + self
            .items
            .iter()
            .filter(|i| matches!(i.annot.shape, Shape::Number { .. }))
            .count() as u32
    }

    /// Escolher a ferramenta que já está ativa desliga ela e volta para a seleção.
    fn toggle_tool(&mut self, t: Tool) {
        self.set_tool(if self.tool == t { Tool::Select } else { t });
    }

    fn set_tool(&mut self, t: Tool) {
        self.commit_text();
        if t != Tool::Select {
            self.select(None);
        }
        self.tool = t;
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = self.editing.is_some();
        let cmd = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
        let shift_cmd = Modifiers::COMMAND | Modifiers::SHIFT;
        let act = |sc: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&sc));

        if act(cmd(Modifiers::COMMAND, Key::S)) {
            self.save();
        }
        if typing {
            return;
        }
        if act(cmd(shift_cmd, Key::Z)) || act(cmd(Modifiers::COMMAND, Key::Y)) {
            self.redo();
        }
        if act(cmd(Modifiers::COMMAND, Key::Z)) {
            self.undo();
        }
        if act(cmd(Modifiers::COMMAND, Key::C)) {
            self.copy();
        }
        let (tool, delete, escape) = ctx.input(|i| {
            let tool = if i.modifiers.is_none() {
                Tool::ALL.into_iter().find(|t| i.key_pressed(t.key()))
            } else {
                None
            };
            let delete = i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace);
            (tool, delete, i.key_pressed(Key::Escape))
        });
        if let Some(t) = tool {
            self.toggle_tool(t);
        }
        if delete {
            self.delete_selected();
        }
        if escape {
            self.select(None);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let before = (self.color, self.width, self.text_size, self.boxed);
        ui.horizontal_wrapped(|ui| {
            if ui.button("← Galeria").clicked() {
                self.commit_text();
                self.event = Some(EditorEvent::Gallery);
            }
            ui.separator();
            for t in Tool::ALL {
                if ui.selectable_label(self.tool == t, t.label()).clicked() {
                    self.toggle_tool(t);
                }
            }
            ui.separator();
            for c in PALETTE {
                let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                let col = Color32::from_rgb(c[0], c[1], c[2]);
                ui.painter().rect_filled(rect, 4.0, col);
                let selected = self.color == c;
                let border = if selected {
                    Stroke::new(2.5, ui.visuals().strong_text_color())
                } else {
                    Stroke::new(1.0, Color32::GRAY)
                };
                ui.painter().rect_stroke(
                    rect.expand(if selected { 2.0 } else { 0.0 }),
                    4.0,
                    border,
                    StrokeKind::Outside,
                );
                if resp.clicked() {
                    self.color = c;
                }
            }
            ui.separator();
            // Os controles seguem a ferramenta ou, no modo seleção, o tipo selecionado.
            let kind = match self.tool {
                Tool::Select => {
                    self.selected
                        .and_then(|i| self.items.get(i))
                        .map(|it| match it.annot.shape {
                            Shape::Text { .. } => Tool::Text,
                            Shape::Number { .. } => Tool::Number,
                            Shape::Highlight { .. } => Tool::Highlight,
                            Shape::Pixelate { .. } => Tool::Pixelate,
                            _ => Tool::Arrow,
                        })
                }
                t => Some(t),
            };
            match kind {
                Some(Tool::Text) | Some(Tool::Number) => {
                    ui.add(egui::Slider::new(&mut self.text_size, 6.0..=120.0).text("tamanho"));
                    if kind == Some(Tool::Text) {
                        ui.checkbox(&mut self.boxed, "caixa");
                    }
                }
                Some(Tool::Arrow) | Some(Tool::Rect) | Some(Tool::Pen) => {
                    ui.add(egui::Slider::new(&mut self.width, 1.0..=20.0).text("espessura"));
                }
                _ => {}
            }
            if self.tool == Tool::Select && self.selected.is_some() && ui.button("Apagar").clicked()
            {
                self.delete_selected();
            }
            ui.separator();
            if ui
                .add_enabled(!self.history.is_empty(), egui::Button::new("↶ Desfazer"))
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(!self.future.is_empty(), egui::Button::new("↷ Refazer"))
                .clicked()
            {
                self.redo();
            }
            ui.separator();
            if ui.button("−").clicked() {
                self.zoom = (self.zoom / 1.25).max(0.05);
            }
            ui.label(format!("{:.0}%", self.zoom * 100.0));
            if ui.button("+").clicked() {
                self.zoom = (self.zoom * 1.25).min(4.0);
            }
            if ui.button("Ajustar").clicked() {
                self.fit_pending = true;
            }
            if ui.button("100%").clicked() {
                self.zoom = 1.0;
            }
            ui.separator();
            if ui.button("Copiar").clicked() {
                self.copy();
            }
            if ui.button("Salvar").clicked() {
                self.save();
            }
        });
        self.apply_props(before);
    }

    /// Qual alça da anotação selecionada está sob `p` (coordenadas da imagem)?
    fn handle_at(&self, p: Pos2) -> Option<usize> {
        let it = self.items.get(self.selected?)?;
        let tol = HANDLE_PX / self.zoom;
        annot::handles(&it.annot, it.sprite_rect())
            .iter()
            .position(|h| (h.x - p.x).abs() <= tol && (h.y - p.y).abs() <= tol)
    }

    /// Cursor que indica a direção em que a alça redimensiona.
    fn handle_cursor(&self, idx: usize) -> CursorIcon {
        let shape = self
            .selected
            .and_then(|i| self.items.get(i))
            .map(|it| &it.annot.shape);
        match shape {
            Some(Shape::Rect { .. } | Shape::Highlight { .. } | Shape::Pixelate { .. }) => {
                match idx {
                    0 | 2 => CursorIcon::ResizeNwSe,
                    1 | 3 => CursorIcon::ResizeNeSw,
                    4 | 6 => CursorIcon::ResizeVertical,
                    _ => CursorIcon::ResizeHorizontal,
                }
            }
            Some(Shape::Text { .. } | Shape::Number { .. }) => CursorIcon::ResizeNwSe,
            _ => CursorIcon::Crosshair,
        }
    }

    /// Anotação mais de cima sob `p`.
    fn item_at(&self, p: Pos2) -> Option<usize> {
        let tol = HIT_PX / self.zoom;
        self.items
            .iter()
            .rposition(|it| annot::hit(&it.annot, it.sprite_rect(), p, tol))
    }

    fn select_mode_input(
        &mut self,
        ctx: &egui::Context,
        resp: &egui::Response,
        to_img: impl Fn(Pos2) -> Pos2,
    ) {
        // Cursor indicando o que o arraste vai fazer.
        if let Some(h) = resp.hover_pos().map(&to_img) {
            if let Some(idx) = self.handle_at(h) {
                ctx.set_cursor_icon(self.handle_cursor(idx));
            } else if self.item_at(h).is_some() {
                ctx.set_cursor_icon(CursorIcon::Move);
            }
        }

        if resp.double_clicked()
            && let Some(p) = resp.interact_pointer_pos().map(&to_img)
            && let Some(i) = self.item_at(p)
            && matches!(self.items[i].annot.shape, Shape::Text { .. })
        {
            self.reedit_text(i);
            return;
        }

        if resp.clicked() {
            let p = resp.interact_pointer_pos().map(&to_img);
            let hit = p.and_then(|p| self.item_at(p));
            self.select(hit);
        }

        if resp.drag_started() {
            let Some(p) = ctx.input(|i| i.pointer.press_origin()).map(&to_img) else {
                return;
            };
            let kind = match self.handle_at(p) {
                Some(h) => Some(GrabKind::Handle(h)),
                None => {
                    let hit = self.item_at(p);
                    if hit != self.selected {
                        self.select(hit);
                    }
                    hit.map(|_| GrabKind::Move)
                }
            };
            if let (Some(kind), Some(i)) = (kind, self.selected) {
                self.snapshot();
                let it = &self.items[i];
                self.grab = Some(Grab {
                    kind,
                    orig: it.annot.clone(),
                    orig_rect: it.sprite_rect(),
                    start: p,
                });
            }
        } else if resp.dragged()
            && let (Some(g), Some(i), Some(p)) = (
                &self.grab,
                self.selected,
                resp.interact_pointer_pos().map(&to_img),
            )
        {
            let item = &mut self.items[i];
            match g.kind {
                GrabKind::Move => {
                    let d = p - g.start;
                    let mut a = g.orig.clone();
                    annot::translate(&mut a, d);
                    item.annot = a;
                    // Texto e número só mudam de lugar: reaproveita a textura.
                    if let (Some((_, r)), Some(orig)) = (&mut item.tex, g.orig_rect) {
                        *r = orig.translate(d);
                    }
                }
                GrabKind::Handle(h) => {
                    item.annot = annot::drag_handle(&g.orig, g.orig_rect, h, p);
                    item.tex = None;
                }
            }
        }

        if resp.drag_stopped() {
            if let (Some(_), Some(i)) = (self.grab.take(), self.selected)
                && matches!(self.items[i].annot.shape, Shape::Pixelate { .. })
            {
                // A pixelação depende do que está embaixo: refaz no lugar novo.
                self.items[i].tex = None;
            }
            // Atualiza a barra (ex.: tamanho do texto depois de redimensionar).
            let sel = self.selected;
            let snap = self.props_snapshotted;
            self.select(sel);
            self.props_snapshotted = snap;
        }
    }

    fn draw_mode_input(
        &mut self,
        ctx: &egui::Context,
        resp: &egui::Response,
        to_img: impl Fn(Pos2) -> Pos2,
    ) {
        let Some(p) = resp.interact_pointer_pos().map(&to_img) else {
            if resp.drag_stopped()
                && let Some(d) = self.draft.take()
            {
                self.push(d);
            }
            return;
        };
        if resp.drag_started() {
            self.commit_text();
            // O arraste só é reconhecido depois de alguns pixels: começa do
            // ponto onde o botão foi pressionado, não de onde está agora.
            let p = ctx
                .input(|i| i.pointer.press_origin())
                .map(&to_img)
                .unwrap_or(p);
            self.drag_start = p;
            let r = Rect::from_two_pos(p, p);
            self.draft = match self.tool {
                Tool::Arrow => Some(self.annot(Shape::Arrow { from: p, to: p })),
                Tool::Rect => Some(self.annot(Shape::Rect { rect: r })),
                Tool::Highlight => Some(self.annot(Shape::Highlight { rect: r })),
                Tool::Pixelate => Some(self.annot(Shape::Pixelate { rect: r })),
                Tool::Pen => Some(self.annot(Shape::Pen { points: vec![p] })),
                Tool::Text | Tool::Number | Tool::Select => None,
            };
        } else if resp.dragged() {
            let s = self.drag_start;
            if let Some(d) = &mut self.draft {
                match &mut d.shape {
                    Shape::Arrow { to, .. } => *to = p,
                    Shape::Rect { rect } | Shape::Highlight { rect } | Shape::Pixelate { rect } => {
                        *rect = Rect::from_two_pos(s, p)
                    }
                    Shape::Pen { points } => {
                        if points.last().is_none_or(|l| l.distance(p) >= 1.5) {
                            points.push(p);
                        }
                    }
                    _ => {}
                }
            }
        }
        if resp.drag_stopped()
            && let Some(d) = self.draft.take()
        {
            self.push(d);
        }
        if resp.clicked() {
            match self.tool {
                Tool::Text => {
                    self.commit_text();
                    self.editing = Some(TextEditing {
                        pos: p,
                        text: String::new(),
                        focus: true,
                        original: None,
                    });
                }
                Tool::Number => {
                    let a = self.annot(Shape::Number {
                        center: p,
                        n: self.next_number(),
                        size: self.text_size,
                    });
                    self.push(a);
                }
                Tool::Pen => {
                    let a = self.annot(Shape::Pen { points: vec![p] });
                    self.push(a);
                }
                _ => {}
            }
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        if self.fit_pending {
            let avail = ui.available_width() - 20.0;
            self.zoom = (avail / self.base.width as f32).clamp(0.05, 1.0);
            self.fit_pending = false;
        }
        let ctx = ui.ctx().clone();
        let zd = ui.input(|i| i.zoom_delta());
        if zd != 1.0 {
            self.zoom = (self.zoom * zd).clamp(0.05, 4.0);
        }
        let escape = ui.input(|i| i.key_pressed(Key::Escape));

        egui::ScrollArea::both()
            .auto_shrink(false)
            .scroll_source(
                egui::scroll_area::ScrollSource::SCROLL_BAR
                    | egui::scroll_area::ScrollSource::MOUSE_WHEEL,
            )
            .show(ui, |ui| {
                let z = self.zoom;
                let size = vec2(self.base.width as f32, self.base.height as f32) * z;
                let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
                let origin = resp.rect.min;
                let to_screen = |p: Pos2| origin + p.to_vec2() * z;
                let to_img = |p: Pos2| pos2((p.x - origin.x) / z, (p.y - origin.y) / z);
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

                for t in &self.tiles {
                    let r = Rect::from_min_size(
                        origin + vec2(0.0, t.y0 * z),
                        vec2(self.base.width as f32 * z, t.rows * z),
                    );
                    painter.image(t.tex.id(), r, uv, Color32::WHITE);
                }

                // Entrada antes de desenhar, para o desenho já refletir o arraste.
                for item in &mut self.items {
                    ensure_texture(&ctx, item, &self.base);
                }
                if self.tool == Tool::Select {
                    self.select_mode_input(&ctx, &resp, to_img);
                } else {
                    self.draw_mode_input(&ctx, &resp, to_img);
                }
                for item in &mut self.items {
                    ensure_texture(&ctx, item, &self.base);
                }

                for item in &self.items {
                    paint(&painter, item, to_screen, z);
                }
                if let Some(d) = &self.draft {
                    paint_draft(&painter, d, to_screen, z);
                }
                if let Some(it) = self.selected.and_then(|i| self.items.get(i)) {
                    paint_selection(&painter, it, to_screen);
                }

                // Caixa de digitação do texto, no lugar onde ele vai ficar.
                if escape && self.editing.is_some() {
                    self.cancel_text();
                }
                let mut commit = false;
                if let Some(ed) = &mut self.editing {
                    let font = FontId::new(
                        (self.text_size * z).max(8.0),
                        FontFamily::Name(ANNOT_FONT.into()),
                    );
                    egui::Area::new(egui::Id::new("texto"))
                        .fixed_pos(to_screen(ed.pos))
                        .order(egui::Order::Foreground)
                        .show(&ctx, |ui| {
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut ed.text)
                                    .font(font)
                                    .hint_text("digite e tecle Enter")
                                    .desired_width((self.text_size * z * 12.0).max(160.0)),
                            );
                            if ed.focus {
                                r.request_focus();
                                ed.focus = false;
                            }
                            if r.lost_focus() {
                                commit = true;
                            }
                        });
                }
                if commit {
                    self.commit_text();
                }
            });
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
            || self
                .editing
                .as_ref()
                .is_some_and(|e| !e.text.trim().is_empty())
    }

    /// Descarta as anotações não salvas (para poder sair sem perguntar).
    pub fn discard(&mut self) {
        self.editing = None;
        self.dirty = false;
    }

    pub fn path(&self) -> &std::path::Path {
        &self.out
    }

    /// Desenha o editor e devolve um pedido para o app, se houver.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<EditorEvent> {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        egui::Panel::top("ferramentas").show(ui, |ui| {
            ui.add_space(4.0);
            self.toolbar(ui);
            ui.add_space(2.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(&self.status);
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        self.event.take()
    }
}

fn color32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn ensure_texture(ctx: &egui::Context, item: &mut Item, base: &Frame) {
    if item.tex.is_some() {
        return;
    }
    if let Some(s) = annot::sprite(&item.annot, base) {
        let size = [s.pixmap.width() as usize, s.pixmap.height() as usize];
        let img = ColorImage::from_rgba_premultiplied(size, s.pixmap.data());
        let tex = ctx.load_texture("anotacao", img, TextureOptions::LINEAR);
        item.tex = Some((tex, s.rect()));
    }
}

fn paint(painter: &egui::Painter, item: &Item, to_screen: impl Fn(Pos2) -> Pos2, z: f32) {
    if let Some((tex, r)) = &item.tex {
        let sr = Rect::from_min_max(to_screen(r.min), to_screen(r.max));
        painter.image(
            tex.id(),
            sr,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        return;
    }
    paint_vector(painter, &item.annot, to_screen, z);
}

fn paint_selection(painter: &egui::Painter, item: &Item, to_screen: impl Fn(Pos2) -> Pos2) {
    let b = annot::bounds(&item.annot, item.sprite_rect());
    let sb = Rect::from_min_max(to_screen(b.min), to_screen(b.max)).expand(3.0);
    painter.rect_stroke(
        sb,
        0.0,
        Stroke::new(1.0, SELECTION_COLOR),
        StrokeKind::Middle,
    );
    for h in annot::handles(&item.annot, item.sprite_rect()) {
        let r = Rect::from_center_size(to_screen(h), vec2(HANDLE_PX, HANDLE_PX));
        painter.rect_filled(r, 1.0, Color32::WHITE);
        painter.rect_stroke(
            r,
            1.0,
            Stroke::new(1.5, SELECTION_COLOR),
            StrokeKind::Middle,
        );
    }
}

fn paint_draft(painter: &egui::Painter, a: &Annot, to_screen: impl Fn(Pos2) -> Pos2, z: f32) {
    if let Shape::Pixelate { rect } = &a.shape {
        // Prévia da pixelação: só o contorno da área.
        let sr = Rect::from_min_max(to_screen(rect.min), to_screen(rect.max));
        painter.rect_stroke(
            sr,
            0.0,
            Stroke::new(1.5, Color32::from_gray(128)),
            StrokeKind::Middle,
        );
        return;
    }
    paint_vector(painter, a, to_screen, z);
}

fn paint_vector(painter: &egui::Painter, a: &Annot, to_screen: impl Fn(Pos2) -> Pos2, z: f32) {
    let col = color32(a.color);
    let stroke = Stroke::new(a.width * z, col);
    match &a.shape {
        Shape::Arrow { from, to } => {
            let g = annot::arrow_geom(*from, *to, a.width);
            if let Some((p, q)) = g.shaft {
                painter.line_segment([to_screen(p), to_screen(q)], stroke);
                painter.circle_filled(to_screen(p), a.width * z / 2.0, col);
            }
            painter.add(egui::Shape::convex_polygon(
                g.head.iter().map(|&p| to_screen(p)).collect(),
                col,
                Stroke::NONE,
            ));
        }
        Shape::Rect { rect } => {
            let sr = Rect::from_min_max(to_screen(rect.min), to_screen(rect.max));
            painter.rect_stroke(sr, 0.0, stroke, StrokeKind::Middle);
        }
        Shape::Highlight { rect } => {
            let sr = Rect::from_min_max(to_screen(rect.min), to_screen(rect.max));
            let c = a.color;
            painter.rect_filled(
                sr,
                0.0,
                Color32::from_rgba_unmultiplied(c[0], c[1], c[2], HIGHLIGHT_ALPHA),
            );
        }
        Shape::Pixelate { rect } => {
            // Só aparece aqui se a área ficou fora da imagem.
            let sr = Rect::from_min_max(to_screen(rect.min), to_screen(rect.max));
            painter.rect_stroke(sr, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Middle);
        }
        Shape::Pen { points } => {
            let pts: Vec<Pos2> = points.iter().map(|&p| to_screen(p)).collect();
            if pts.len() == 1 {
                painter.circle_filled(pts[0], a.width * z / 2.0, col);
            } else {
                painter.line(pts, stroke);
            }
        }
        _ => {}
    }
}

fn copy_to_clipboard(f: &Frame) -> Result<()> {
    let mut png = Vec::new();
    crate::encode_png(f, &mut png)?;
    copy_png_bytes(&png)
}

/// Põe um PNG na área de transferência.
pub fn copy_png_bytes(png: &[u8]) -> Result<()> {
    // O xclip fica em segundo plano servindo a imagem depois que o editor fecha.
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "image/png", "-i"])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("não consegui rodar o xclip: {e}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("sem stdin do xclip"))?
        .write_all(png)?;
    child.wait()?;
    Ok(())
}
