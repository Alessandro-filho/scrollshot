//! Emenda de frames capturados durante a rolagem.
//!
//! Cada frame é comparado com o anterior linha a linha (hash por linha).
//! O deslocamento vertical é o que faz o maior número de linhas do frame novo
//! coincidir com linhas do frame anterior. Linhas que não mudaram de posição
//! (cabeçalho e rodapé fixos) e linhas de cor única (ambíguas) ficam fora da
//! comparação.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Imagem em memória, um `u32` por pixel no formato 0x00RRGGBB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Frame {
    pub fn new(width: usize, height: usize, pixels: Vec<u32>) -> Self {
        assert_eq!(pixels.len(), width * height, "tamanho do buffer inválido");
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn row(&self, y: usize) -> &[u32] {
        &self.pixels[y * self.width..(y + 1) * self.width]
    }
}

#[derive(Clone, Copy, Debug)]
struct RowInfo {
    hash: u64,
    uniform: bool,
}

fn row_infos(f: &Frame) -> Vec<RowInfo> {
    (0..f.height)
        .map(|y| {
            let row = f.row(y);
            let mut h = DefaultHasher::new();
            row.hash(&mut h);
            RowInfo {
                hash: h.finish(),
                uniform: row.iter().all(|&p| p == row[0]),
            }
        })
        .collect()
}

/// Sobreposição mínima (em linhas) para aceitar um deslocamento.
const MIN_OVERLAP: usize = 8;
/// Mínimo de linhas coincidentes para aceitar um deslocamento.
const MIN_MATCHES: usize = 3;
/// Fração mínima das linhas comparáveis que precisa coincidir.
/// Fica baixa de propósito: o cursor da barra de rolagem estraga algumas linhas.
const MIN_RATIO: f64 = 0.35;

#[derive(Debug, PartialEq, Eq)]
pub enum Push {
    /// O conteúdo rolou `dy` linhas e a parte nova foi anexada.
    Appended(usize),
    /// O frame é praticamente igual ao anterior (fim da página ou nada rolou).
    NoChange,
    /// Não foi possível encontrar a sobreposição.
    NoMatch,
}

fn find_offset(prev: &[RowInfo], new: &[RowInfo], is_static: &[bool]) -> Option<usize> {
    let h = new.len();
    if h <= MIN_OVERLAP {
        return None;
    }
    let mut best: Option<(usize, usize, usize)> = None; // (dy, coincidências, comparáveis)
    for dy in 1..=h - MIN_OVERLAP {
        let mut matches = 0;
        let mut eligible = 0;
        for r in 0..h - dy {
            if new[r].uniform || is_static[r] {
                continue;
            }
            eligible += 1;
            if prev[r + dy].hash == new[r].hash {
                matches += 1;
            }
        }
        if matches > best.map_or(0, |b| b.1) {
            best = Some((dy, matches, eligible));
        }
    }
    best.filter(|&(_, m, e)| m >= MIN_MATCHES && m as f64 >= MIN_RATIO * e as f64)
        .map(|b| b.0)
}

pub struct Stitcher {
    width: usize,
    canvas: Vec<u32>,
    rows: usize,
    last: Frame,
    last_info: Vec<RowInfo>,
    /// Largura dos frames capturados (antes de qualquer corte).
    input_width: usize,
    /// Faixa de colunas que rola, quando há uma lateral parada cortada.
    crop: Option<(usize, usize)>,
    /// O corte lateral já foi decidido (no primeiro passo em que algo mudou).
    crop_decided: bool,
}

/// Largura mínima de uma faixa lateral parada para ser cortada.
fn min_side_run(width: usize) -> usize {
    (width / 20).max(40)
}

/// Máximo de mudanças de cor na vertical para uma coluna parecer barra de
/// rolagem (trilho e cursor têm poucas cores; conteúdo tem muitas).
const SCROLLBAR_MAX_TRANSITIONS: usize = 12;
/// Distância máxima da borda até a barra de rolagem (margem parada).
const SCROLLBAR_MAX_GAP: usize = 12;
/// Largura aceitável de uma barra de rolagem.
const SCROLLBAR_WIDTH: std::ops::RangeInclusive<usize> = 3..=24;

/// Mudanças de cor na vertical da coluna `x`, contando só as linhas que rolaram
/// (cabeçalhos parados, como a barra do navegador, não entram na conta).
fn transitions(f: &Frame, x: usize, moving: &[bool]) -> usize {
    (1..f.height)
        .filter(|&y| moving[y] && moving[y - 1])
        .filter(|&y| f.pixels[y * f.width + x] != f.pixels[(y - 1) * f.width + x])
        .count()
}

/// Procura uma barra de rolagem perto de uma borda: uma faixa estreita de
/// colunas com poucas cores que mudou entre os frames (o cursor andou),
/// começando a até `SCROLLBAR_MAX_GAP` colunas da borda. As colunas antes dela
/// (borda da janela, margem) podem ter qualquer coisa, até um pixel solto que
/// mudou. Retorna quantas colunas cortar a partir da borda (margem mais barra).
fn edge_scrollbar(
    prev: &Frame,
    new: &Frame,
    changed: &[bool],
    moving: &[bool],
    cols: impl Iterator<Item = usize>,
) -> usize {
    let cols: Vec<usize> = cols.collect();
    let is_bar = |x: usize| {
        changed[x]
            && transitions(prev, x, moving) <= SCROLLBAR_MAX_TRANSITIONS
            && transitions(new, x, moving) <= SCROLLBAR_MAX_TRANSITIONS
    };
    for gap in 0..=SCROLLBAR_MAX_GAP.min(cols.len()) {
        let bar = cols[gap..].iter().take_while(|&&x| is_bar(x)).count();
        if SCROLLBAR_WIDTH.contains(&bar) {
            return gap + bar;
        }
    }
    0
}

/// Compara dois frames coluna a coluna. Se nada mudou, retorna `None` (ainda
/// não dá para decidir). Senão, retorna a faixa de colunas a manter, cortando
/// das laterais: faixas largas paradas (ex.: menu fixo de um painel) e a barra
/// de rolagem, cujo cursor muda de lugar e atrapalharia a emenda.
fn side_crop(prev: &Frame, new: &Frame) -> Option<Option<(usize, usize)>> {
    let w = new.width;
    let changed: Vec<bool> = (0..w)
        .map(|x| (0..new.height).any(|y| prev.pixels[y * w + x] != new.pixels[y * w + x]))
        .collect();
    let first = changed.iter().position(|&c| c)?;
    let last = changed.iter().rposition(|&c| c)?;
    let min_run = min_side_run(w);
    let mut x0 = if first >= min_run { first } else { 0 };
    let mut x1 = if w - 1 - last >= min_run { last + 1 } else { w };
    let moving: Vec<bool> = (0..new.height).map(|y| prev.row(y) != new.row(y)).collect();
    x1 -= edge_scrollbar(prev, new, &changed, &moving, (x0..x1).rev());
    x0 += edge_scrollbar(prev, new, &changed, &moving, x0..x1);
    if x1 <= x0 {
        return Some(None);
    }
    Some(((x0, x1) != (0, w)).then_some((x0, x1)))
}

fn crop_frame(f: &Frame, x0: usize, x1: usize) -> Frame {
    let pixels = (0..f.height)
        .flat_map(|y| f.pixels[y * f.width + x0..y * f.width + x1].iter().copied())
        .collect();
    Frame::new(x1 - x0, f.height, pixels)
}

impl Stitcher {
    pub fn new(first: Frame) -> Self {
        let last_info = row_infos(&first);
        Self {
            width: first.width,
            canvas: first.pixels.clone(),
            rows: first.height,
            input_width: first.width,
            last: first,
            last_info,
            crop: None,
            crop_decided: false,
        }
    }

    /// Faixa de colunas mantida, se uma lateral parada foi cortada.
    pub fn crop(&self) -> Option<(usize, usize)> {
        self.crop
    }

    fn apply_crop(&mut self, x0: usize, x1: usize) {
        let full = Frame::new(self.width, self.rows, std::mem::take(&mut self.canvas));
        self.canvas = crop_frame(&full, x0, x1).pixels;
        self.width = x1 - x0;
        self.last = crop_frame(&self.last, x0, x1);
        self.last_info = row_infos(&self.last);
        self.crop = Some((x0, x1));
    }

    /// Altura atual da imagem emendada.
    pub fn height(&self) -> usize {
        self.rows
    }

    pub fn push(&mut self, f: Frame) -> Push {
        assert_eq!((f.width, f.height), (self.input_width, self.last.height));
        if !self.crop_decided
            && let Some(decision) = side_crop(&self.last, &f)
        {
            self.crop_decided = true;
            if let Some((x0, x1)) = decision {
                self.apply_crop(x0, x1);
            }
        }
        let f = match self.crop {
            Some((x0, x1)) => crop_frame(&f, x0, x1),
            None => f,
        };
        let h = f.height;
        let w = self.width;
        let info = row_infos(&f);
        let is_static: Vec<bool> = (0..h)
            .map(|i| info[i].hash == self.last_info[i].hash)
            .collect();

        let changed = is_static.iter().filter(|&&s| !s).count();
        if changed <= (h / 50).max(2) {
            return Push::NoChange;
        }

        let Some(dy) = find_offset(&self.last_info, &info, &is_static) else {
            return Push::NoMatch;
        };

        // Trecho do fim do canvas a ser refeito com o frame novo. O canvas
        // sempre termina com o fim do último frame, então removemos esse
        // trecho e reanexamos a partir do frame novo. Superestimar não causa
        // erro, só precisa caber no frame (footer + dy <= h). Duas fontes:
        // 1. rodapé fixo: linhas do fim que não mudaram de posição;
        let static_run = is_static.iter().rev().take_while(|&&s| s).count();
        // 2. sujeira na borda: linhas do fim do frame anterior que não batem
        //    com o frame novo na sobreposição (algo desenhado por cima do
        //    conteúdo, como o tracejado do Nemo).
        let dirty_run = (0..h - dy)
            .rev()
            .take_while(|&r| self.last_info[r + dy].hash != info[r].hash)
            .count();
        let footer = static_run.max(dirty_run).min(h - dy);

        self.rows -= footer;
        self.canvas.truncate(self.rows * w);
        self.canvas
            .extend_from_slice(&f.pixels[(h - dy - footer) * w..]);
        self.rows += dy + footer;

        self.last = f;
        self.last_info = info;
        Push::Appended(dy)
    }

    pub fn finish(self) -> Frame {
        Frame::new(self.width, self.rows, self.canvas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 40;
    const H: usize = 100;

    /// Página sintética: cada linha tem um padrão próprio, com algumas linhas
    /// brancas espalhadas (como espaçamento entre parágrafos).
    fn page_row(y: usize) -> Vec<u32> {
        if y % 13 == 0 {
            return vec![0xffffff; W];
        }
        (0..W)
            .map(|x| {
                let mut h = DefaultHasher::new();
                (x, y).hash(&mut h);
                (h.finish() as u32) & 0x00ff_ffff
            })
            .collect()
    }

    fn rows_to_frame(rows: Vec<Vec<u32>>) -> Frame {
        let h = rows.len();
        Frame::new(W, h, rows.into_iter().flatten().collect())
    }

    fn page(range: std::ops::Range<usize>) -> Vec<Vec<u32>> {
        range.map(page_row).collect()
    }

    #[test]
    fn emenda_rolagem_simples() {
        let offsets = [0, 40, 95, 150, 200];
        let mut st = Stitcher::new(rows_to_frame(page(0..H)));
        for win in offsets.windows(2) {
            let r = st.push(rows_to_frame(page(win[1]..win[1] + H)));
            assert_eq!(r, Push::Appended(win[1] - win[0]));
        }
        assert_eq!(st.finish(), rows_to_frame(page(0..200 + H)));
    }

    #[test]
    fn cabecalho_e_rodape_fixos_aparecem_uma_vez() {
        let header: Vec<Vec<u32>> = (0..10).map(|i| vec![0x112233 + i; W]).collect();
        let footer: Vec<Vec<u32>> = (0..12)
            .map(|i| (0..W as u32).map(|x| 0x445566 + x * 7 + i).collect())
            .collect();
        let content_h = H - header.len() - footer.len();
        let frame_at = |o: usize| {
            let mut rows = header.clone();
            rows.extend(page(o..o + content_h));
            rows.extend(footer.clone());
            rows_to_frame(rows)
        };

        let offsets = [0, 30, 60, 100];
        let mut st = Stitcher::new(frame_at(0));
        for win in offsets.windows(2) {
            assert_eq!(st.push(frame_at(win[1])), Push::Appended(win[1] - win[0]));
        }

        let mut expected = header.clone();
        expected.extend(page(0..100 + content_h));
        expected.extend(footer.clone());
        assert_eq!(st.finish(), rows_to_frame(expected));
    }

    #[test]
    fn barra_de_rolagem_nao_atrapalha_o_deslocamento() {
        // As 6 colunas da direita são a barra: trilho cinza com um cursor de
        // 30 linhas que desce conforme a página rola.
        let frame_at = |o: usize| {
            let thumb_top = o / 4;
            let rows = page(o..o + H)
                .into_iter()
                .enumerate()
                .map(|(y, mut row)| {
                    let c = if (thumb_top..thumb_top + 30).contains(&y) {
                        0x555555
                    } else {
                        0xdddddd
                    };
                    row[W - 6..].fill(c);
                    row
                })
                .collect();
            rows_to_frame(rows)
        };

        let offsets = [0, 45, 90, 135];
        let mut st = Stitcher::new(frame_at(0));
        for win in offsets.windows(2) {
            assert_eq!(st.push(frame_at(win[1])), Push::Appended(win[1] - win[0]));
        }
        assert_eq!(st.height(), 135 + H);
    }

    #[test]
    fn sobreposicao_na_borda_de_baixo_nao_fica_no_meio() {
        // A última linha da área tem um tracejado fixo desenhado por cima do
        // conteúdo que rola (como o Nemo faz): metade dos pixels é fixa.
        let frame_at = |o: usize| {
            let mut rows = page(o..o + H);
            for x in (0..W).step_by(2) {
                rows[H - 1][x] = 0x777777;
            }
            rows_to_frame(rows)
        };

        let offsets = [0, 40, 80, 120];
        let mut st = Stitcher::new(frame_at(0));
        for win in offsets.windows(2) {
            assert_eq!(st.push(frame_at(win[1])), Push::Appended(win[1] - win[0]));
        }

        // Conteúdo limpo e contínuo; o tracejado só na última linha da imagem.
        let mut expected = page(0..120 + H - 1);
        expected.push(frame_at(120).row(H - 1).to_vec());
        assert_eq!(st.finish(), rows_to_frame(expected));
    }

    #[test]
    fn frame_igual_indica_fim() {
        let mut st = Stitcher::new(rows_to_frame(page(0..H)));
        assert_eq!(st.push(rows_to_frame(page(0..H))), Push::NoChange);
        assert_eq!(st.height(), H);
    }

    #[test]
    fn sem_sobreposicao_nao_emenda() {
        let mut st = Stitcher::new(rows_to_frame(page(0..H)));
        assert_eq!(st.push(rows_to_frame(page(500..500 + H))), Push::NoMatch);
        assert_eq!(st.height(), H);
    }

    #[test]
    fn menu_lateral_parado_e_cortado_e_o_conteudo_emendado() {
        // 50 colunas de menu fixo à esquerda (com texto, não é cor única) e o
        // conteúdo que rola à direita, como num painel web.
        const MENU: usize = 50;
        let menu_row = |y: usize| -> Vec<u32> {
            (0..MENU)
                .map(|x| 0x101010 + (x * 31 + y * 7) as u32 % 200)
                .collect()
        };
        let frame_at = |o: usize| {
            let rows: Vec<u32> = (0..H)
                .flat_map(|y| {
                    let mut row = menu_row(y);
                    row.extend(page_row(o + y));
                    row
                })
                .collect();
            Frame::new(MENU + W, H, rows)
        };

        let offsets = [0, 40, 80, 120];
        let mut st = Stitcher::new(frame_at(0));
        for win in offsets.windows(2) {
            assert_eq!(st.push(frame_at(win[1])), Push::Appended(win[1] - win[0]));
        }
        assert_eq!(st.crop(), Some((MENU, MENU + W)));
        assert_eq!(st.finish(), rows_to_frame(page(0..120 + H)));
    }

    #[test]
    fn barra_de_rolagem_grande_na_borda_e_removida() {
        // Página curta: o cursor da barra ocupa metade da altura e anda muito
        // a cada passo. A barra (6 colunas na borda direita, depois de 3 de
        // margem) deve sair da comparação e da imagem.
        const BAR: usize = 6;
        const MARGIN: usize = 3;
        // Como num navegador: um cabeçalho parado no topo (abas e endereço)
        // com várias cores em todas as colunas, inclusive na borda da janela.
        const TOP: usize = 16;
        let frame_at = |o: usize| {
            let thumb_top = o / 2;
            let rows: Vec<u32> = (0..H)
                .flat_map(|y| {
                    if y < TOP {
                        return (0..W + BAR + MARGIN)
                            .map(|x| ((x + y) % 2) as u32 * 0x303030 + y as u32)
                            .collect::<Vec<_>>();
                    }
                    let mut row = page_row(o + y);
                    let c = if (thumb_top..thumb_top + 50).contains(&y) {
                        0x555555
                    } else {
                        0xdddddd
                    };
                    row.extend(std::iter::repeat_n(c, BAR));
                    row.extend(std::iter::repeat_n(0x222222, MARGIN));
                    // Um pixel solto na borda que muda entre os frames (como a
                    // borda da janela do navegador no caso real).
                    if y == TOP + 5 {
                        *row.last_mut().unwrap() = 0x222222 + o as u32;
                    }
                    row
                })
                .collect();
            Frame::new(W + BAR + MARGIN, H, rows)
        };

        let offsets = [0, 45, 90];
        let mut st = Stitcher::new(frame_at(0));
        for win in offsets.windows(2) {
            assert_eq!(st.push(frame_at(win[1])), Push::Appended(win[1] - win[0]));
        }
        assert_eq!(st.crop(), Some((0, W)));
        let out = st.finish();
        assert_eq!(out.height, 90 + H);
        // Abaixo do cabeçalho, a página emendada sem a barra.
        let corpo = Frame::new(W, out.height - TOP, out.pixels[TOP * W..].to_vec());
        assert_eq!(corpo, rows_to_frame(page(TOP..90 + H)));
    }
}
