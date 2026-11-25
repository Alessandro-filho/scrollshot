mod annot;
mod app;
mod editor;
mod gallery;
mod selection;
mod stitch;
mod x11;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};

use stitch::{Frame, Push, Stitcher};
use x11::{Rect, X};

/// Altura máxima da imagem final, para não rolar para sempre em feeds infinitos.
const MAX_HEIGHT: usize = 40_000;
/// Quantos frames seguidos sem mudança encerram a captura.
const IDLE_LIMIT: u32 = 2;
/// Quanto o mouse pode se mexer (em px) sem interromper a captura.
const POINTER_TOLERANCE: i32 = 40;
/// Tentativas seguidas de voltar e rolar menos quando a emenda falha.
const MAX_RETRIES: u32 = 3;

const HELP: &str = "\
scrollshot: captura com rolagem para X11

Uso:
  scrollshot [--no-edit] [ARQUIVO.png]   captura com rolagem e abre o editor
  scrollshot --edit IMAGEM.png           só abre o editor numa imagem existente
  scrollshot --gallery                   abre a galeria das capturas salvas

Arraste para selecionar a área; ajuste pelas alças ou arrastando por dentro.
Espaço (ou duplo clique): print com rolagem; o programa rola sozinho e emenda
os trechos. Mexa o mouse para parar antes do fim.
Enter: print normal, só da área visível. Esc cancela.
A captura é salva antes do editor abrir; no editor, Salvar grava as anotações
por cima do mesmo arquivo. Sem ARQUIVO, salva em ~/Imagens/scrollshot/ (a pasta
que a galeria mostra), com o nome scrollshot-<data>.png. Para usar outra pasta,
defina a variável SCROLLSHOT_DIR.";

fn main() {
    if let Err(e) = run() {
        eprintln!("erro: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut no_edit = false;
    let mut gallery = false;
    let mut edit: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "--no-edit" => no_edit = true,
            "-g" | "--gallery" => gallery = true,
            "--edit" => {
                edit = Some(
                    args.next()
                        .context("--edit precisa do caminho da imagem")?
                        .into(),
                )
            }
            _ => out = Some(a.into()),
        }
    }

    let dir = gallery_dir()?;
    if gallery {
        return app::run(app::Start::Gallery, dir);
    }
    if let Some(path) = edit {
        let frame = load_png(&path)?;
        return app::run(app::Start::Editor(frame, out.unwrap_or(path)), dir);
    }

    let out = out.unwrap_or_else(|| default_path(&dir));
    let Some(frame) = capture()? else {
        return Ok(());
    };
    save_png(&frame, &out)?;
    println!("{}", out.display());
    if !no_edit {
        app::run(app::Start::Editor(frame, out), dir)?;
    }
    Ok(())
}

/// Seleção, rolagem e emenda. Retorna `None` se o usuário cancelar.
fn capture() -> Result<Option<Frame>> {
    let x = X::connect()?;
    eprintln!(
        "Selecione a área: Espaço faz print com rolagem, Enter faz print normal, Esc cancela."
    );
    let Some((r, mode)) = x.select_region()? else {
        eprintln!("Cancelado.");
        return Ok(None);
    };
    if mode == x11::Mode::Single {
        // Dá tempo do compositor redesenhar a tela sem o overlay.
        sleep(Duration::from_millis(300));
        return Ok(Some(x.capture(r)?));
    }
    if r.h < 120 {
        eprintln!("aviso: região baixa ({} px), a emenda pode falhar", r.h);
    }

    // Dá tempo do compositor redesenhar a tela sem o overlay.
    sleep(Duration::from_millis(300));
    x.warp_pointer(r.x + r.w as i16 / 2, r.y + r.h as i16 / 2)?;
    // Usa a posição real como referência; mexidas pequenas não interrompem.
    let anchor = x.pointer()?;
    let moved = |p: (i16, i16)| {
        (p.0 as i32 - anchor.0 as i32).abs() + (p.1 as i32 - anchor.1 as i32).abs()
            > POINTER_TOLERANCE
    };

    let debug = std::env::var_os("SCROLLSHOT_DEBUG").is_some();
    let mut st = Stitcher::new(settled_capture(&x, r)?);
    let mut clicks: u32 = 1;
    // Teto de cliques por passo; cai quando uma quantidade já falhou na emenda.
    let mut max_clicks: u32 = 30;
    let mut idle = 0;
    let mut retries = 0;
    eprintln!("Capturando... mexa o mouse para interromper.");

    let reason = loop {
        if st.height() >= MAX_HEIGHT {
            break "limite de altura atingido";
        }
        if moved(x.pointer()?) {
            break "interrompido pelo mouse";
        }
        x.scroll(x11::WHEEL_DOWN, clicks)?;
        let result = st.push(settled_capture(&x, r)?);
        if debug {
            eprintln!(
                "[debug] cliques={clicks} resultado={result:?} altura={}",
                st.height()
            );
        }
        match result {
            Push::Appended(dy) => {
                idle = 0;
                retries = 0;
                // Calibra os cliques para rolar ~40% da região, no máximo
                // dobrando por passo (a medição pode ter pego a animação no meio).
                let per_click = (dy as f64 / clicks as f64).max(1.0);
                let target = ((r.h as f64 * 0.4) / per_click).floor().max(1.0) as u32;
                clicks = target.min(clicks * 2).min(max_clicks);
                if !debug {
                    eprint!("\r  {} px", st.height());
                }
            }
            Push::NoChange => {
                idle += 1;
                if idle >= IDLE_LIMIT {
                    break "fim da rolagem";
                }
            }
            Push::NoMatch => {
                retries += 1;
                if retries > MAX_RETRIES {
                    break "não consegui emendar o último trecho, o conteúdo mudou ou rolou demais";
                }
                // Volta para onde estava e tenta com um passo menor.
                x.scroll(x11::WHEEL_UP, clicks)?;
                settled_capture(&x, r)?;
                max_clicks = (clicks - 1).max(1);
                clicks = (clicks / 2).max(1);
            }
        }
    };
    eprintln!("\r  {} px: {reason}", st.height());
    if let Some((x0, x1)) = st.crop() {
        eprintln!(
            "  laterais cortadas (parte parada ou barra de rolagem): mantive as colunas {x0} a {x1} da seleção ({} px de largura)",
            x1 - x0
        );
    }
    Ok(Some(st.finish()))
}

/// Captura e espera o conteúdo parar de se mexer (rolagem suave, animações):
/// só aceita depois de 3 capturas idênticas seguidas.
fn settled_capture(x: &X, r: Rect) -> Result<Frame> {
    sleep(Duration::from_millis(150));
    let mut prev = x.capture(r)?;
    let mut same = 0;
    for _ in 0..40 {
        sleep(Duration::from_millis(40));
        let f = x.capture(r)?;
        if f == prev {
            same += 1;
            if same >= 2 {
                return Ok(f);
            }
        } else {
            same = 0;
        }
        prev = f;
    }
    Ok(prev)
}

/// Pasta das capturas (a que a galeria mostra): ~/Imagens/scrollshot, ou a
/// definida em SCROLLSHOT_DIR.
fn gallery_dir() -> Result<PathBuf> {
    let dir = match std::env::var_os("SCROLLSHOT_DIR") {
        Some(d) => PathBuf::from(d),
        None => dirs::picture_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("scrollshot"),
    };
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("não consegui criar {}", dir.display()))?;
    Ok(dir)
}

fn default_path(dir: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    dir.join(format!("scrollshot-{stamp}.png"))
}

fn to_rgb_image(f: &Frame) -> Result<image::RgbImage> {
    let mut buf = Vec::with_capacity(f.pixels.len() * 3);
    for &p in &f.pixels {
        buf.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    image::RgbImage::from_raw(f.width as u32, f.height as u32, buf)
        .context("buffer de imagem inválido")
}

pub fn save_png(f: &Frame, path: &Path) -> Result<()> {
    to_rgb_image(f)?
        .save(path)
        .with_context(|| format!("falha ao salvar {}", path.display()))
}

pub fn encode_png(f: &Frame, out: &mut impl Write) -> Result<()> {
    let mut cur = std::io::Cursor::new(Vec::new());
    to_rgb_image(f)?.write_to(&mut cur, image::ImageFormat::Png)?;
    out.write_all(cur.get_ref())?;
    Ok(())
}

pub fn load_png(path: &Path) -> Result<Frame> {
    let img = image::open(path)
        .with_context(|| format!("não consegui abrir {}", path.display()))?
        .to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let pixels = img
        .pixels()
        .map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
        .collect();
    Ok(Frame::new(w, h, pixels))
}
