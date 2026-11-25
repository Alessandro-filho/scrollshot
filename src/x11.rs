//! Acesso ao X11: captura de região, overlay de seleção e rolagem via XTest.

use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

use crate::selection::{Hit, Sel};
use crate::stitch::Frame;

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

pub struct X {
    conn: RustConnection,
    root: Window,
    root_w: u16,
    root_h: u16,
    depth: u8,
}

/// Códigos de tecla no layout evdev padrão.
const KEY_ESC: u8 = 9;
const KEY_ENTER: u8 = 36;
const KEY_KP_ENTER: u8 = 104;
const KEY_SPACE: u8 = 65;
/// Lado mínimo de uma seleção válida.
const MIN_SEL: i32 = 10;
/// Folga para acertar uma alça da seleção, em pixels.
const HANDLE_TOL: i32 = 7;
/// Tamanho das alças desenhadas.
const HANDLE_SIZE: i32 = 9;
fn select_events() -> EventMask {
    EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION
}
const HELP_TEXT: &str = " Arraste para selecionar; ajuste pelas alças ou arraste por dentro.  Espaço ou duplo clique: print com rolagem  |  Enter: print normal  |  Esc: cancelar ";

/// Como capturar a área escolhida.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Rola o conteúdo e emenda (Espaço ou duplo clique).
    Scroll,
    /// Só a área visível, sem rolar (Enter).
    Single,
}

#[derive(Clone, Copy)]
enum SelOp {
    New { start: (i32, i32) },
    Move { orig: Sel, start: (i32, i32) },
    Resize { orig: Sel, idx: usize },
}

impl From<Sel> for Rect {
    fn from(s: Sel) -> Self {
        Rect {
            x: s.x0 as i16,
            y: s.y0 as i16,
            w: s.width() as u16,
            h: s.height() as u16,
        }
    }
}

/// Converte para Latin-1, a codificação da fonte "fixed" do X.
fn latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
        .collect()
}

/// Desenho do overlay de seleção.
struct Overlay<'a> {
    conn: &'a RustConnection,
    win: Window,
    pen: Gcontext,
    fill: Gcontext,
    text: Gcontext,
}

impl Overlay<'_> {
    /// Região ocupada pela seleção, alças e etiqueta de tamanho.
    fn area(s: Sel) -> (i16, i16, u16, u16) {
        let m = HANDLE_SIZE;
        let x = (s.x0 - m).max(0);
        let y = (s.y0 - 24).max(0);
        let w = s.x1 + m - x + 1;
        let h = s.y1 + 24 - y + 1;
        (x as i16, y as i16, w.max(1) as u16, h.max(1) as u16)
    }

    fn erase(&self, s: Sel) -> Result<()> {
        let (x, y, w, h) = Self::area(s);
        self.conn.clear_area(false, self.win, x, y, w, h)?;
        Ok(())
    }

    fn draw(&self, s: Sel) -> Result<()> {
        let c = self.conn;
        c.poly_rectangle(
            self.win,
            self.pen,
            &[Rectangle {
                x: s.x0 as i16,
                y: s.y0 as i16,
                width: s.width() as u16,
                height: s.height() as u16,
            }],
        )?;
        let half = HANDLE_SIZE / 2;
        let boxes: Vec<Rectangle> = s
            .handles()
            .iter()
            .map(|&(hx, hy)| Rectangle {
                x: (hx - half) as i16,
                y: (hy - half) as i16,
                width: HANDLE_SIZE as u16,
                height: HANDLE_SIZE as u16,
            })
            .collect();
        c.poly_fill_rectangle(self.win, self.fill, &boxes)?;
        c.poly_rectangle(self.win, self.pen, &boxes)?;
        let label = latin1(&format!(" {} x {} ", s.width(), s.height()));
        let ly = if s.y0 >= 22 { s.y0 - 9 } else { s.y1 + 18 };
        c.image_text8(self.win, self.text, s.x0 as i16, ly as i16, &label)?;
        Ok(())
    }

    fn help(&self) -> Result<()> {
        self.conn
            .image_text8(self.win, self.text, 16, 24, &latin1(HELP_TEXT))?;
        Ok(())
    }
}
/// Botões do X11 que correspondem à roda do mouse.
pub const WHEEL_UP: u8 = 4;
pub const WHEEL_DOWN: u8 = 5;

impl X {
    pub fn connect() -> Result<Self> {
        let (conn, n) = x11rb::connect(None)
            .context("não foi possível conectar ao X11 (a sessão é Wayland?)")?;
        let setup = conn.setup();
        let screen = &setup.roots[n];
        let depth = screen.root_depth;
        let bpp = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == depth)
            .map(|f| f.bits_per_pixel);
        if setup.image_byte_order != ImageOrder::LSB_FIRST || bpp != Some(32) || depth < 24 {
            bail!("formato de tela não suportado (profundidade {depth}, bpp {bpp:?})");
        }
        if conn
            .extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)?
            .is_none()
        {
            bail!("a extensão XTest não está disponível no servidor X");
        }
        Ok(Self {
            root: screen.root,
            root_w: screen.width_in_pixels,
            root_h: screen.height_in_pixels,
            depth,
            conn,
        })
    }

    pub fn capture(&self, r: Rect) -> Result<Frame> {
        let reply = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, self.root, r.x, r.y, r.w, r.h, !0)?
            .reply()?;
        let pixels = reply
            .data
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], 0]))
            .collect();
        Ok(Frame::new(r.w as usize, r.h as usize, pixels))
    }

    /// Mostra a tela congelada em tela cheia e deixa o usuário arrastar um
    /// retângulo, que pode ser ajustado pelas alças ou movido antes de confirmar.
    /// Retorna `None` se cancelar com Esc.
    pub fn select_region(&self) -> Result<Option<(Rect, Mode)>> {
        let conn = &self.conn;
        let (w, h) = (self.root_w, self.root_h);
        let full = self.capture(Rect { x: 0, y: 0, w, h })?;

        // Copia o print da tela para um pixmap, que vira o fundo do overlay.
        let pix = conn.generate_id()?;
        conn.create_pixmap(self.depth, pix, self.root, w, h)?;
        let gc = conn.generate_id()?;
        conn.create_gc(gc, pix, &CreateGCAux::new().graphics_exposures(0))?;
        let bytes: Vec<u8> = full.pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
        let stride = w as usize * 4;
        let rows_per_req = ((conn.maximum_request_bytes() - 64) / stride).max(1);
        for y0 in (0..h as usize).step_by(rows_per_req) {
            let rows = rows_per_req.min(h as usize - y0);
            conn.put_image(
                ImageFormat::Z_PIXMAP,
                pix,
                gc,
                w,
                rows as u16,
                0,
                y0 as i16,
                0,
                self.depth,
                &bytes[y0 * stride..(y0 + rows) * stride],
            )?;
        }

        let cursor_font = conn.generate_id()?;
        conn.open_font(cursor_font, b"cursor")?;
        // Glifos da fonte "cursor": mira, mover, e as 8 alças na ordem de
        // `Sel::handles` (cantos sup. esq., sup. dir., inf. dir., inf. esq.,
        // depois lados de cima, direita, baixo, esquerda).
        let glyphs: [u16; 10] = [34, 52, 134, 136, 14, 12, 138, 96, 16, 70];
        let mut cursors = Vec::new();
        for g in glyphs {
            let c = conn.generate_id()?;
            conn.create_glyph_cursor(
                c,
                cursor_font,
                cursor_font,
                g,
                g + 1,
                0,
                0,
                0,
                0xffff,
                0xffff,
                0xffff,
            )?;
            cursors.push(c);
        }
        let text_font = conn.generate_id()?;
        conn.open_font(text_font, b"fixed")?;

        let win = conn.generate_id()?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            win,
            self.root,
            0,
            0,
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            COPY_FROM_PARENT,
            &CreateWindowAux::new()
                .background_pixmap(pix)
                .override_redirect(1)
                .cursor(cursors[0])
                .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS),
        )?;
        let ov = Overlay {
            conn,
            win,
            pen: conn.generate_id()?,
            fill: conn.generate_id()?,
            text: conn.generate_id()?,
        };
        conn.create_gc(
            ov.pen,
            win,
            &CreateGCAux::new().foreground(0xff3b30).line_width(2),
        )?;
        conn.create_gc(ov.fill, win, &CreateGCAux::new().foreground(0xffffff))?;
        conn.create_gc(
            ov.text,
            win,
            &CreateGCAux::new()
                .foreground(0xffffff)
                .background(0x202020)
                .font(text_font),
        )?;
        conn.map_window(win)?;
        conn.flush()?;

        let mut grabbed = false;
        for _ in 0..50 {
            let status = conn
                .grab_pointer(
                    false,
                    win,
                    select_events(),
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    win,
                    cursors[0],
                    CURRENT_TIME,
                )?
                .reply()?
                .status;
            if status == GrabStatus::SUCCESS {
                grabbed = true;
                break;
            }
            sleep(Duration::from_millis(20));
        }
        // Chamado por atalho de teclado, o ambiente ainda segura o teclado por
        // um instante: tenta de novo, senão Esc/Espaço/Enter não chegam.
        for _ in 0..50 {
            let status = conn
                .grab_keyboard(false, win, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)?
                .reply()?
                .status;
            if status == GrabStatus::SUCCESS {
                break;
            }
            sleep(Duration::from_millis(20));
        }

        let result = if grabbed {
            self.drag_loop(&ov, &cursors)
        } else {
            Ok(None)
        };

        conn.ungrab_pointer(CURRENT_TIME)?;
        conn.ungrab_keyboard(CURRENT_TIME)?;
        conn.destroy_window(win)?;
        for g in [ov.pen, ov.fill, ov.text, gc] {
            conn.free_gc(g)?;
        }
        conn.free_pixmap(pix)?;
        for c in cursors {
            conn.free_cursor(c)?;
        }
        conn.close_font(cursor_font)?;
        conn.close_font(text_font)?;
        conn.flush()?;

        if !grabbed {
            bail!("não consegui capturar o mouse para a seleção");
        }
        result
    }

    fn drag_loop(&self, ov: &Overlay, cursors: &[Cursor]) -> Result<Option<(Rect, Mode)>> {
        let conn = &self.conn;
        let bounds = (self.root_w as i32, self.root_h as i32);
        let usable = |s: &Sel| s.width() >= MIN_SEL && s.height() >= MIN_SEL;
        let mut sel: Option<Sel> = None;
        let mut op: Option<SelOp> = None;
        let mut last_press: Option<(Instant, (i32, i32))> = None;
        let mut current_cursor = cursors[0];
        ov.help()?;
        conn.flush()?;
        loop {
            match conn.wait_for_event()? {
                Event::KeyPress(e) => match e.detail {
                    KEY_ESC => return Ok(None),
                    KEY_ENTER | KEY_KP_ENTER | KEY_SPACE => {
                        let mode = if e.detail == KEY_SPACE {
                            Mode::Scroll
                        } else {
                            Mode::Single
                        };
                        if let Some(s) = sel.filter(usable) {
                            return Ok(Some((s.into(), mode)));
                        }
                    }
                    _ => {}
                },
                Event::Expose(_) => {
                    if let Some(s) = sel {
                        ov.draw(s)?;
                    }
                    ov.help()?;
                    conn.flush()?;
                }
                Event::ButtonPress(e) if e.detail == 1 => {
                    let p = (e.root_x as i32, e.root_y as i32);
                    let hit = sel.map_or(Hit::Outside, |s| s.hit(p, HANDLE_TOL));
                    let now = Instant::now();
                    let double = last_press.is_some_and(|(t, q)| {
                        now - t < Duration::from_millis(400)
                            && (q.0 - p.0).abs() + (q.1 - p.1).abs() < 6
                    });
                    if double
                        && hit == Hit::Inside
                        && let Some(s) = sel.filter(usable)
                    {
                        return Ok(Some((s.into(), Mode::Scroll)));
                    }
                    last_press = Some((now, p));
                    op = Some(match (hit, sel) {
                        (Hit::Handle(idx), Some(orig)) => SelOp::Resize { orig, idx },
                        (Hit::Inside, Some(orig)) => SelOp::Move { orig, start: p },
                        _ => SelOp::New { start: p },
                    });
                }
                Event::MotionNotify(e) => {
                    let p = (e.root_x as i32, e.root_y as i32);
                    match &op {
                        Some(o) => {
                            let new = match *o {
                                SelOp::New { start } => {
                                    // Ignora tremidas de um clique simples fora da seleção.
                                    if (p.0 - start.0).abs() + (p.1 - start.1).abs() < 4 {
                                        continue;
                                    }
                                    Sel::from_points(start, p)
                                }
                                SelOp::Move { orig, start } => {
                                    orig.translate_clamped(p.0 - start.0, p.1 - start.1, bounds)
                                }
                                SelOp::Resize { orig, idx } => orig.drag_handle(idx, p),
                            };
                            if let Some(old) = sel {
                                ov.erase(old)?;
                            }
                            ov.draw(new)?;
                            ov.help()?;
                            conn.flush()?;
                            sel = Some(new);
                        }
                        None => {
                            // Cursor indica o que acontece ao arrastar a partir daqui.
                            let c = match sel.map(|s| s.hit(p, HANDLE_TOL)) {
                                Some(Hit::Handle(i)) => cursors[2 + i],
                                Some(Hit::Inside) => cursors[1],
                                _ => cursors[0],
                            };
                            if c != current_cursor {
                                conn.change_active_pointer_grab(c, CURRENT_TIME, select_events())?;
                                conn.flush()?;
                                current_cursor = c;
                            }
                        }
                    }
                }
                Event::ButtonRelease(e) if e.detail == 1 => {
                    op = None;
                    // Seleção pequena demais (clique acidental): apaga.
                    if let Some(s) = sel.filter(|s| !usable(s)) {
                        ov.erase(s)?;
                        ov.help()?;
                        conn.flush()?;
                        sel = None;
                    }
                }
                _ => {}
            }
        }
    }

    pub fn warp_pointer(&self, x: i16, y: i16) -> Result<()> {
        self.conn.warp_pointer(NONE, self.root, 0, 0, 0, 0, x, y)?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn pointer(&self) -> Result<(i16, i16)> {
        let r = self.conn.query_pointer(self.root)?.reply()?;
        Ok((r.root_x, r.root_y))
    }

    /// Envia `clicks` cliques de roda (`WHEEL_UP` ou `WHEEL_DOWN`) na posição atual do mouse.
    pub fn scroll(&self, button: u8, clicks: u32) -> Result<()> {
        for _ in 0..clicks {
            for kind in [BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT] {
                self.conn
                    .xtest_fake_input(kind, button, CURRENT_TIME, self.root, 0, 0, 0)?;
            }
            self.conn.flush()?;
            sleep(Duration::from_millis(8));
        }
        Ok(())
    }
}
