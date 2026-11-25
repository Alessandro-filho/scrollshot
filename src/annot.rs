//! Anotações sobre a captura e sua renderização no PNG final.
//!
//! Formas vetoriais (seta, retângulo, marca-texto, caneta) são desenhadas com
//! tiny-skia no arquivo e com o painter do egui na tela. Texto, numeração e
//! pixelação viram "sprites" (imagens pequenas) gerados pelo mesmo código para
//! a tela e para o arquivo, então o que aparece no editor é o que é salvo.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform,
};

use crate::stitch::Frame;

pub static FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");

/// Opacidade do marca-texto.
pub const HIGHLIGHT_ALPHA: u8 = 0x66;
/// Tamanho do bloco da pixelação, em pixels.
pub const PIXEL_BLOCK: usize = 10;

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Arrow {
        from: Pos2,
        to: Pos2,
    },
    Rect {
        rect: Rect,
    },
    Highlight {
        rect: Rect,
    },
    Pen {
        points: Vec<Pos2>,
    },
    Text {
        pos: Pos2,
        text: String,
        size: f32,
        boxed: bool,
    },
    Number {
        center: Pos2,
        n: u32,
        size: f32,
    },
    Pixelate {
        rect: Rect,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annot {
    pub shape: Shape,
    pub color: [u8; 3],
    pub width: f32,
}

pub struct ArrowGeom {
    pub shaft: Option<(Pos2, Pos2)>,
    pub head: [Pos2; 3],
}

pub fn arrow_geom(from: Pos2, to: Pos2, width: f32) -> ArrowGeom {
    let d = to - from;
    let len = d.length();
    let dir = if len > 0.0 { d / len } else { vec2(1.0, 0.0) };
    let perp = vec2(-dir.y, dir.x);
    let head_len = head_len(width);
    let base = to - dir * head_len;
    let half = head_len * 0.55;
    ArrowGeom {
        // A haste termina dentro da ponta para não sobrar fresta.
        shaft: (len > head_len).then(|| (from, base + dir * head_len * 0.3)),
        head: [to, base + perp * half, base - perp * half],
    }
}

/// Cor do texto que contrasta com o fundo `c`.
pub fn contrast(c: [u8; 3]) -> [u8; 3] {
    let lum = 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
    if lum > 150.0 {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    }
}

pub struct Sprite {
    pub x: i32,
    pub y: i32,
    pub pixmap: Pixmap,
}

impl Sprite {
    pub fn rect(&self) -> Rect {
        Rect::from_min_size(
            pos2(self.x as f32, self.y as f32),
            vec2(self.pixmap.width() as f32, self.pixmap.height() as f32),
        )
    }
}

/// Gera o sprite das anotações que não são vetoriais.
pub fn sprite(a: &Annot, base: &Frame) -> Option<Sprite> {
    match &a.shape {
        Shape::Text {
            pos,
            text,
            size,
            boxed,
        } => text_sprite(*pos, text, *size, *boxed, a.color),
        Shape::Number { center, n, size } => number_sprite(*center, *n, *size, a.color),
        Shape::Pixelate { rect } => pixelate_sprite(*rect, base),
        _ => None,
    }
}

fn font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT_BYTES).expect("fonte embutida inválida")
}

fn text_width(font: &FontRef, scale: PxScale, text: &str) -> f32 {
    let sf = font.as_scaled(scale);
    let mut w = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

/// Mistura `color` com cobertura `cov` no pixel (x, y), em RGBA pré-multiplicado.
fn blend(pm: &mut Pixmap, x: i32, y: i32, color: [u8; 3], cov: f32) {
    if x < 0 || y < 0 || x >= pm.width() as i32 || y >= pm.height() as i32 {
        return;
    }
    let a = cov.clamp(0.0, 1.0);
    let i = (y as usize * pm.width() as usize + x as usize) * 4;
    let data = pm.data_mut();
    for k in 0..3 {
        data[i + k] = (color[k] as f32 * a + data[i + k] as f32 * (1.0 - a)).round() as u8;
    }
    data[i + 3] = (255.0 * a + data[i + 3] as f32 * (1.0 - a)).round() as u8;
}

fn draw_text(
    pm: &mut Pixmap,
    font: &FontRef,
    scale: PxScale,
    text: &str,
    x0: f32,
    baseline: f32,
    color: [u8; 3],
) {
    let sf = font.as_scaled(scale);
    let mut x = x0;
    let mut prev = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev {
            x += sf.kern(p, id);
        }
        let glyph = id.with_scale_and_position(scale, point(x, baseline));
        if let Some(og) = font.outline_glyph(glyph) {
            let b = og.px_bounds();
            og.draw(|gx, gy, cov| {
                blend(
                    pm,
                    b.min.x as i32 + gx as i32,
                    b.min.y as i32 + gy as i32,
                    color,
                    cov,
                )
            });
        }
        x += sf.h_advance(id);
        prev = Some(id);
    }
}

fn solid(color: [u8; 3], alpha: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(color[0], color[1], color[2], alpha);
    p
}

fn text_sprite(pos: Pos2, text: &str, size: f32, boxed: bool, color: [u8; 3]) -> Option<Sprite> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return None;
    }
    let font = font();
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let line_h = sf.height() + sf.line_gap();
    let max_w = lines
        .iter()
        .map(|l| text_width(&font, scale, l))
        .fold(0.0, f32::max);
    // Com caixa: fundo na cor escolhida. Sem caixa: contorno na cor de contraste
    // para o texto ficar legível sobre qualquer fundo.
    let pad = if boxed { (size * 0.35).round() } else { 3.0 };
    let w = (max_w + 2.0 * pad).ceil() as u32;
    let h = (line_h * lines.len() as f32 + 2.0 * pad).ceil() as u32;
    let mut pm = Pixmap::new(w.max(1), h.max(1))?;

    let fg = if boxed {
        let r = tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32)?;
        pm.fill_rect(r, &solid(color, 255), Transform::identity(), None);
        contrast(color)
    } else {
        color
    };
    for (i, line) in lines.iter().enumerate() {
        let baseline = pad + sf.ascent() + i as f32 * line_h;
        if !boxed {
            let halo = contrast(color);
            for (dx, dy) in [
                (-2.0, 0.0),
                (2.0, 0.0),
                (0.0, -2.0),
                (0.0, 2.0),
                (-1.5, -1.5),
                (1.5, -1.5),
                (-1.5, 1.5),
                (1.5, 1.5),
            ] {
                draw_text(&mut pm, &font, scale, line, pad + dx, baseline + dy, halo);
            }
        }
        draw_text(&mut pm, &font, scale, line, pad, baseline, fg);
    }
    Some(Sprite {
        x: pos.x.round() as i32,
        y: pos.y.round() as i32,
        pixmap: pm,
    })
}

fn number_sprite(center: Pos2, n: u32, size: f32, color: [u8; 3]) -> Option<Sprite> {
    let r = (size * 0.8).round().max(8.0);
    let d = (2.0 * r + 2.0) as u32;
    let mut pm = Pixmap::new(d, d)?;
    let c = d as f32 / 2.0;
    let circle = PathBuilder::from_circle(c, c, r)?;
    pm.fill_path(
        &circle,
        &solid(color, 255),
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    let font = font();
    let label = n.to_string();
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let tw = text_width(&font, scale, &label);
    let baseline = c + (sf.ascent() + sf.descent()) / 2.0;
    draw_text(
        &mut pm,
        &font,
        scale,
        &label,
        c - tw / 2.0,
        baseline,
        contrast(color),
    );
    Some(Sprite {
        x: (center.x - c).round() as i32,
        y: (center.y - c).round() as i32,
        pixmap: pm,
    })
}

fn pixelate_sprite(rect: Rect, base: &Frame) -> Option<Sprite> {
    let x0 = rect.min.x.max(0.0).round() as usize;
    let y0 = rect.min.y.max(0.0).round() as usize;
    let x1 = (rect.max.x.round().max(0.0) as usize).min(base.width);
    let y1 = (rect.max.y.round().max(0.0) as usize).min(base.height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut pm = Pixmap::new(w as u32, h as u32)?;
    let data = pm.data_mut();
    for by in (0..h).step_by(PIXEL_BLOCK) {
        for bx in (0..w).step_by(PIXEL_BLOCK) {
            let (bw, bh) = (PIXEL_BLOCK.min(w - bx), PIXEL_BLOCK.min(h - by));
            let mut sum = [0u64; 3];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let p = base.pixels[(y0 + y) * base.width + x0 + x];
                    sum[0] += (p >> 16 & 0xff) as u64;
                    sum[1] += (p >> 8 & 0xff) as u64;
                    sum[2] += (p & 0xff) as u64;
                }
            }
            let n = (bw * bh) as u64;
            let avg = [
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
                255,
            ];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    let i = (y * w + x) * 4;
                    data[i..i + 4].copy_from_slice(&avg);
                }
            }
        }
    }
    Some(Sprite {
        x: x0 as i32,
        y: y0 as i32,
        pixmap: pm,
    })
}

fn to_sk_rect(r: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_ltrb(r.min.x, r.min.y, r.max.x, r.max.y)
}

fn draw_vector(pm: &mut Pixmap, a: &Annot) {
    let paint = solid(a.color, 255);
    let mut stroke = Stroke {
        width: a.width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    };
    let id = Transform::identity();
    match &a.shape {
        Shape::Arrow { from, to } => {
            let g = arrow_geom(*from, *to, a.width);
            if let Some((p, q)) = g.shaft {
                let mut pb = PathBuilder::new();
                pb.move_to(p.x, p.y);
                pb.line_to(q.x, q.y);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint, &stroke, id, None);
                }
            }
            let mut pb = PathBuilder::new();
            pb.move_to(g.head[0].x, g.head[0].y);
            pb.line_to(g.head[1].x, g.head[1].y);
            pb.line_to(g.head[2].x, g.head[2].y);
            pb.close();
            if let Some(path) = pb.finish() {
                pm.fill_path(&path, &paint, FillRule::Winding, id, None);
            }
        }
        Shape::Rect { rect } => {
            stroke.line_join = LineJoin::Miter;
            if let Some(path) = to_sk_rect(*rect).map(PathBuilder::from_rect) {
                pm.stroke_path(&path, &paint, &stroke, id, None);
            }
        }
        Shape::Highlight { rect } => {
            if let Some(r) = to_sk_rect(*rect) {
                pm.fill_rect(r, &solid(a.color, HIGHLIGHT_ALPHA), id, None);
            }
        }
        Shape::Pen { points } => {
            let mut pb = PathBuilder::new();
            if let Some(first) = points.first() {
                pb.move_to(first.x, first.y);
                // Um ponto só vira um traço mínimo, que a ponta redonda transforma em bolinha.
                let rest: &[Pos2] = if points.len() > 1 {
                    &points[1..]
                } else {
                    &[*first]
                };
                for p in rest {
                    pb.line_to(p.x + if points.len() == 1 { 0.01 } else { 0.0 }, p.y);
                }
            }
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &paint, &stroke, id, None);
            }
        }
        _ => {}
    }
}

/// Aplica as anotações sobre a captura e devolve a imagem final.
pub fn render(base: &Frame, annots: &[Annot]) -> Frame {
    let mut pm = Pixmap::new(base.width as u32, base.height as u32).expect("imagem vazia");
    for (px, &p) in pm.data_mut().chunks_exact_mut(4).zip(&base.pixels) {
        px.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]);
    }
    for a in annots {
        match sprite(a, base) {
            Some(s) => pm.draw_pixmap(
                s.x,
                s.y,
                s.pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            ),
            None => draw_vector(&mut pm, a),
        }
    }
    let pixels = pm
        .data()
        .chunks_exact(4)
        .map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
        .collect();
    Frame::new(base.width, base.height, pixels)
}

fn head_len(width: f32) -> f32 {
    (width * 4.0).max(14.0)
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    let t = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// A anotação está sob o ponto `p`? `sprite_rect` é o retângulo já calculado
/// para texto, número e pixelação; `tol` é a folga, em pixels da imagem.
pub fn hit(a: &Annot, sprite_rect: Option<Rect>, p: Pos2, tol: f32) -> bool {
    let half = a.width / 2.0 + tol;
    match &a.shape {
        Shape::Arrow { from, to } => {
            dist_to_segment(p, *from, *to) <= half
                || p.distance(*to) <= head_len(a.width) * 0.6 + tol
        }
        // Retângulo pega pela borda e por toda a área de dentro.
        Shape::Rect { rect } => rect.expand(half).contains(p),
        Shape::Highlight { rect } => rect.expand(tol).contains(p),
        Shape::Pen { points } => match points.as_slice() {
            [single] => p.distance(*single) <= half,
            pts => pts
                .windows(2)
                .any(|w| dist_to_segment(p, w[0], w[1]) <= half),
        },
        Shape::Text { .. } | Shape::Number { .. } | Shape::Pixelate { .. } => {
            sprite_rect.is_some_and(|r| r.expand(tol).contains(p))
        }
    }
}

/// Desloca a anotação inteira.
pub fn translate(a: &mut Annot, d: Vec2) {
    match &mut a.shape {
        Shape::Arrow { from, to } => {
            *from += d;
            *to += d;
        }
        Shape::Rect { rect } | Shape::Highlight { rect } | Shape::Pixelate { rect } => {
            *rect = rect.translate(d)
        }
        Shape::Pen { points } => points.iter_mut().for_each(|p| *p += d),
        Shape::Text { pos, .. } => *pos += d,
        Shape::Number { center, .. } => *center += d,
    }
}

/// Retângulo que envolve a anotação (para desenhar a seleção).
pub fn bounds(a: &Annot, sprite_rect: Option<Rect>) -> Rect {
    let pad = a.width / 2.0;
    match &a.shape {
        Shape::Arrow { from, to } => {
            Rect::from_two_pos(*from, *to).expand(pad.max(head_len(a.width) * 0.55))
        }
        Shape::Rect { rect } => rect.expand(pad),
        Shape::Highlight { rect } => *rect,
        Shape::Pen { points } => Rect::from_points(points).expand(pad),
        _ => sprite_rect.unwrap_or(Rect::NOTHING),
    }
}

/// Alças de redimensionamento da anotação, em coordenadas da imagem.
pub fn handles(a: &Annot, sprite_rect: Option<Rect>) -> Vec<Pos2> {
    match &a.shape {
        Shape::Arrow { from, to } => vec![*from, *to],
        // Cantos (0 a 3) e meios dos lados (4 a 7: cima, direita, baixo, esquerda).
        Shape::Rect { rect } | Shape::Highlight { rect } | Shape::Pixelate { rect } => vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
            rect.center_top(),
            rect.right_center(),
            rect.center_bottom(),
            rect.left_center(),
        ],
        Shape::Text { .. } | Shape::Number { .. } => sprite_rect
            .map(|r| vec![r.right_bottom()])
            .unwrap_or_default(),
        Shape::Pen { .. } => vec![],
    }
}

/// Resultado de arrastar a alça `idx` até `p`, calculado a partir do estado
/// original (do início do arraste) para não acumular erro.
pub fn drag_handle(orig: &Annot, sprite_rect: Option<Rect>, idx: usize, p: Pos2) -> Annot {
    let mut a = orig.clone();
    match &mut a.shape {
        Shape::Arrow { from, to } => {
            if idx == 0 {
                *from = p
            } else {
                *to = p
            }
        }
        Shape::Rect { rect } | Shape::Highlight { rect } | Shape::Pixelate { rect } => {
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ];
            *rect = match idx {
                0..=3 => Rect::from_two_pos(corners[(idx + 2) % 4], p),
                _ => {
                    let (mut min, mut max) = (rect.min, rect.max);
                    match idx {
                        4 => min.y = p.y,
                        5 => max.x = p.x,
                        6 => max.y = p.y,
                        _ => min.x = p.x,
                    }
                    Rect::from_two_pos(min, max)
                }
            };
        }
        Shape::Text { size, .. } => {
            if let Some(r) = sprite_rect {
                let scale = (p.y - r.top()) / r.height().max(1.0);
                *size = (*size * scale).clamp(6.0, 300.0);
            }
        }
        Shape::Number { center, size, .. } => {
            if let Some(r) = sprite_rect {
                let scale = center.distance(p) / center.distance(r.right_bottom()).max(1.0);
                *size = (*size * scale).clamp(6.0, 300.0);
            }
        }
        Shape::Pen { .. } => {}
    }
    a
}

/// Uma anotação tem tamanho suficiente para valer a pena guardar?
pub fn is_meaningful(a: &Annot) -> bool {
    let big = |r: &Rect| r.width() >= 3.0 && r.height() >= 3.0;
    match &a.shape {
        Shape::Arrow { from, to } => (*to - *from).length() >= 5.0,
        Shape::Rect { rect } | Shape::Highlight { rect } | Shape::Pixelate { rect } => big(rect),
        Shape::Pen { points } => !points.is_empty(),
        Shape::Text { text, .. } => !text.trim().is_empty(),
        Shape::Number { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: u32 = 0xffffff;

    fn blank(w: usize, h: usize) -> Frame {
        Frame::new(w, h, vec![WHITE; w * h])
    }

    fn annot(shape: Shape) -> Annot {
        Annot {
            shape,
            color: [255, 0, 0],
            width: 4.0,
        }
    }

    fn px(f: &Frame, x: usize, y: usize) -> u32 {
        f.pixels[y * f.width + x]
    }

    #[test]
    fn seta_pinta_a_haste_e_a_ponta() {
        let out = render(
            &blank(200, 100),
            &[annot(Shape::Arrow {
                from: pos2(10.0, 50.0),
                to: pos2(190.0, 50.0),
            })],
        );
        assert_eq!(px(&out, 60, 50), 0xff0000, "haste");
        assert_eq!(px(&out, 185, 50), 0xff0000, "ponta");
        assert_eq!(px(&out, 60, 10), WHITE, "fora da seta");
    }

    #[test]
    fn retangulo_pinta_so_a_borda() {
        let r = Rect::from_min_max(pos2(20.0, 20.0), pos2(80.0, 80.0));
        let out = render(&blank(100, 100), &[annot(Shape::Rect { rect: r })]);
        assert_eq!(px(&out, 20, 50), 0xff0000);
        assert_eq!(px(&out, 50, 50), WHITE);
    }

    #[test]
    fn marca_texto_e_translucido() {
        let r = Rect::from_min_max(pos2(0.0, 0.0), pos2(50.0, 50.0));
        let out = render(&blank(60, 60), &[annot(Shape::Highlight { rect: r })]);
        let p = px(&out, 25, 25);
        assert_ne!(p, WHITE);
        assert_ne!(p, 0xff0000);
        assert_eq!(
            p >> 16,
            0xff,
            "vermelho por cima do branco mantém o R no máximo"
        );
    }

    #[test]
    fn legenda_com_caixa_tem_fundo_na_cor_e_texto() {
        let a = annot(Shape::Text {
            pos: pos2(5.0, 5.0),
            text: "Olá".into(),
            size: 24.0,
            boxed: true,
        });
        let s = sprite(&a, &blank(10, 10)).unwrap();
        assert!(s.pixmap.width() > 30 && s.pixmap.height() > 20);
        let out = render(&blank(200, 100), &[a]);
        assert_eq!(px(&out, 6, 6), 0xff0000, "canto da caixa");
        let caixa = Rect::from_min_size(pos2(5.0, 5.0), s.rect().size());
        let texto = (0..out.height)
            .flat_map(|y| (0..out.width).map(move |x| (x, y)))
            .filter(|&(x, y)| caixa.contains(pos2(x as f32, y as f32)) && px(&out, x, y) == WHITE)
            .count();
        assert!(
            texto > 20,
            "o texto branco precisa aparecer dentro da caixa"
        );
    }

    #[test]
    fn pixelar_deixa_blocos_uniformes() {
        let w = 40;
        let base = Frame::new(
            w,
            w,
            (0..w * w)
                .map(|i| (i as u32 * 2654435761) & 0xffffff)
                .collect(),
        );
        let r = Rect::from_min_max(pos2(0.0, 0.0), pos2(20.0, 20.0));
        let out = render(&base, &[annot(Shape::Pixelate { rect: r })]);
        let first = px(&out, 0, 0);
        for y in 0..PIXEL_BLOCK {
            for x in 0..PIXEL_BLOCK {
                assert_eq!(px(&out, x, y), first);
            }
        }
        assert_eq!(px(&out, 30, 30), px(&base, 30, 30), "fora da área não muda");
    }

    #[test]
    fn numeracao_centralizada_no_clique() {
        let a = annot(Shape::Number {
            center: pos2(50.0, 50.0),
            n: 3,
            size: 20.0,
        });
        let s = sprite(&a, &blank(10, 10)).unwrap();
        let c = s.rect().center();
        assert!((c.x - 50.0).abs() <= 1.0 && (c.y - 50.0).abs() <= 1.0);
    }

    #[test]
    fn clique_acerta_seta_e_area_do_retangulo() {
        let seta = annot(Shape::Arrow {
            from: pos2(0.0, 0.0),
            to: pos2(100.0, 0.0),
        });
        assert!(hit(&seta, None, pos2(50.0, 3.0), 2.0));
        assert!(!hit(&seta, None, pos2(50.0, 30.0), 2.0));

        let r = annot(Shape::Rect {
            rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)),
        });
        assert!(hit(&r, None, pos2(1.0, 50.0), 2.0), "na borda");
        assert!(
            hit(&r, None, pos2(50.0, 50.0), 2.0),
            "o miolo também pega o retângulo"
        );
        assert!(!hit(&r, None, pos2(150.0, 50.0), 2.0), "fora não pega");
    }

    #[test]
    fn clique_em_texto_usa_o_retangulo_do_sprite() {
        let t = annot(Shape::Text {
            pos: pos2(10.0, 10.0),
            text: "oi".into(),
            size: 20.0,
            boxed: true,
        });
        let r = sprite(&t, &blank(10, 10)).unwrap().rect();
        assert!(hit(&t, Some(r), r.center(), 0.0));
        assert!(!hit(&t, Some(r), pos2(500.0, 500.0), 0.0));
    }

    #[test]
    fn mover_desloca_todos_os_pontos() {
        let mut a = annot(Shape::Pen {
            points: vec![pos2(0.0, 0.0), pos2(10.0, 5.0)],
        });
        translate(&mut a, vec2(3.0, 4.0));
        assert_eq!(
            a.shape,
            Shape::Pen {
                points: vec![pos2(3.0, 4.0), pos2(13.0, 9.0)]
            }
        );
        let mut t = annot(Shape::Text {
            pos: pos2(1.0, 1.0),
            text: "x".into(),
            size: 10.0,
            boxed: false,
        });
        translate(&mut t, vec2(-1.0, 2.0));
        assert!(matches!(t.shape, Shape::Text { pos, .. } if pos == pos2(0.0, 3.0)));
    }

    #[test]
    fn alca_do_canto_redimensiona_com_o_canto_oposto_fixo() {
        let a = annot(Shape::Rect {
            rect: Rect::from_min_max(pos2(10.0, 10.0), pos2(50.0, 50.0)),
        });
        let b = drag_handle(&a, None, 2, pos2(80.0, 90.0));
        assert_eq!(
            b.shape,
            Shape::Rect {
                rect: Rect::from_min_max(pos2(10.0, 10.0), pos2(80.0, 90.0))
            }
        );
        let c = drag_handle(&a, None, 0, pos2(0.0, 0.0));
        assert_eq!(
            c.shape,
            Shape::Rect {
                rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(50.0, 50.0))
            }
        );
    }

    #[test]
    fn alca_da_seta_move_so_a_ponta_escolhida() {
        let a = annot(Shape::Arrow {
            from: pos2(0.0, 0.0),
            to: pos2(10.0, 0.0),
        });
        assert_eq!(
            drag_handle(&a, None, 1, pos2(30.0, 5.0)).shape,
            Shape::Arrow {
                from: pos2(0.0, 0.0),
                to: pos2(30.0, 5.0)
            }
        );
        assert_eq!(
            drag_handle(&a, None, 0, pos2(-5.0, 0.0)).shape,
            Shape::Arrow {
                from: pos2(-5.0, 0.0),
                to: pos2(10.0, 0.0)
            }
        );
    }

    #[test]
    fn alca_do_texto_aumenta_a_fonte() {
        let t = annot(Shape::Text {
            pos: pos2(0.0, 0.0),
            text: "oi".into(),
            size: 20.0,
            boxed: true,
        });
        let r = sprite(&t, &blank(10, 10)).unwrap().rect();
        let maior = drag_handle(&t, Some(r), 0, pos2(r.right(), r.top() + r.height() * 2.0));
        assert!(matches!(maior.shape, Shape::Text { size, .. } if (size - 40.0).abs() < 0.01));
    }

    #[test]
    fn alcas_do_meio_mexem_so_um_lado() {
        let r0 = Rect::from_min_max(pos2(10.0, 10.0), pos2(50.0, 50.0));
        let a = annot(Shape::Rect { rect: r0 });
        let hs = handles(&a, None);
        assert_eq!(hs.len(), 8, "4 cantos e 4 meios");
        assert_eq!(hs[4], pos2(30.0, 10.0), "meio de cima");
        assert_eq!(hs[5], pos2(50.0, 30.0), "meio da direita");

        let topo = drag_handle(&a, None, 4, pos2(999.0, 0.0));
        assert_eq!(
            topo.shape,
            Shape::Rect {
                rect: Rect::from_min_max(pos2(10.0, 0.0), pos2(50.0, 50.0))
            }
        );
        let direita = drag_handle(&a, None, 5, pos2(70.0, 999.0));
        assert_eq!(
            direita.shape,
            Shape::Rect {
                rect: Rect::from_min_max(pos2(10.0, 10.0), pos2(70.0, 50.0))
            }
        );
        let baixo = drag_handle(&a, None, 6, pos2(0.0, 80.0));
        assert_eq!(
            baixo.shape,
            Shape::Rect {
                rect: Rect::from_min_max(pos2(10.0, 10.0), pos2(50.0, 80.0))
            }
        );
        // Passar do lado oposto inverte sem quebrar o retângulo.
        let esquerda = drag_handle(&a, None, 7, pos2(60.0, 0.0));
        assert_eq!(
            esquerda.shape,
            Shape::Rect {
                rect: Rect::from_min_max(pos2(50.0, 10.0), pos2(60.0, 50.0))
            }
        );
    }
}
