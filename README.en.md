# scrollshot

[Português](README.md) · **English** · [Español](README.es.md)

Area screenshots for Linux with **automatic scrolling**: select the part of the screen that scrolls (a web page, a list, a dashboard) and scrollshot scrolls it for you, stitching everything into one long image. It also includes an annotation editor and a gallery of your captures.

> For now it works only on **Linux with X11** (for example Cinnamon, MATE, Xfce, or GNOME/KDE in an X11 session). Wayland is not supported yet.

## Features

- **Scrolling screenshot**: scrolls the content under the selection and stitches the pieces without repeating or cutting lines.
  - Fixed headers and footers appear only once.
  - Static side menus and the scrollbar are cropped automatically.
  - Calibrates how far to scroll on each step and stops at the end of the content.
- **Regular screenshot** of the selected area, without scrolling.
- **Adjustable selection** before capturing: handles on the corners and sides, and you can move it by dragging inside.
- **Annotation editor**: arrow, rectangle, highlighter, pen, text (with or without a box), numbered markers and pixelate (to hide sensitive data). Annotations can be selected, moved, resized, edited and deleted, with undo and redo.
- **Gallery** of saved captures, with thumbnails, to open, edit, copy or move to the trash.
- **Copy** the image to the clipboard.

## Installation

### Dependencies

- [Rust](https://rustup.rs) 1.88 or newer.
- An **X11** session with the XTest extension (enabled by default on X servers).
- Programs used at runtime:

```sh
sudo apt install xclip libglib2.0-bin xdg-utils libgl1 libxkbcommon-x11-0
```

(`xclip` copies the image, `gio` moves files to the trash, `xdg-open` opens the folder.)

### Build and install

```sh
git clone https://github.com/Alessandro-filho/scrollshot.git
cd scrollshot
./install.sh
```

The script builds the project, installs the binary to `~/.local/bin/scrollshot` and adds **Scrollshot** and **Scrollshot Galeria** to the application menu. It does not use `sudo`.

- `./install.sh --cinnamon-shortcut` also creates the **Super+Shift+S** shortcut on Cinnamon.
- On other desktops, create a keyboard shortcut that runs `~/.local/bin/scrollshot`.
- `./install.sh --uninstall` removes everything.
- `PREFIX=/some/where ./install.sh` installs to another prefix.

## Usage

```sh
scrollshot                    # select an area and capture
scrollshot --gallery          # open the gallery
scrollshot --edit photo.png   # open the editor on an existing image
scrollshot --no-edit          # capture without opening the editor
scrollshot file.png           # save with this name
```

### Selection

| Action | Result |
|---|---|
| Drag | creates the selection |
| Drag a handle | resizes |
| Drag inside | moves the selection |
| **Space** or double click | **scrolling** screenshot |
| **Enter** | **regular** screenshot, visible area only |
| Esc | cancels |

Do not move the mouse while it scrolls: a large movement stops the capture and saves what has been stitched so far.

Select only the area that scrolls. If the selection includes a static side menu or the scrollbar, they are cropped from the image.

### Editor

| Key | Tool |
|---|---|
| V | Select (move, resize with handles, Delete removes, double click edits text) |
| A | Arrow |
| R | Rectangle |
| M | Highlighter |
| P | Pen |
| T | Text (the "caixa" option toggles the background box) |
| N | Number |
| B | Pixelate |

Pressing the key of the active tool again switches back to Select. Ctrl+Z undoes, Ctrl+Shift+Z or Ctrl+Y redoes, Ctrl+S saves, Ctrl+C copies and Ctrl+mouse wheel zooms.

The interface is currently in Portuguese.

### Where captures are saved

In `~/Pictures/scrollshot/` (your user's pictures folder, localized; `~/Imagens` in Portuguese). The capture is saved as soon as it finishes; in the editor, **Salvar** (Save) writes the annotations over the same file.

### Environment variables

| Variable | Effect |
|---|---|
| `SCROLLSHOT_DIR` | changes the captures and gallery folder |
| `SCROLLSHOT_DEBUG=1` | prints each scrolling step to the terminal (useful for bug reports) |

## Contributing

Contributions are welcome: fixes, support for other environments (Wayland, for example), new editor tools and translations.

### Setting up

```sh
git clone https://github.com/Alessandro-filho/scrollshot.git
cd scrollshot
cargo build
cargo run            # capture
cargo run -- --gallery
```

### Before opening a pull request

```sh
cargo fmt
cargo clippy --release   # no warnings
cargo test               # all passing
```

- Changes to `stitch.rs`, `annot.rs` or `selection.rs` should come with tests. These parts do not depend on the screen, so everything can be tested with synthetic images (see the existing tests).
- When fixing a stitching case, first write a test that reproduces the problem and fails, then fix it.
- Code comments explain **why**, not the obvious.
- Short, clear commit messages describing what changed and why.

### Code layout

| File | Responsibility |
|---|---|
| `src/main.rs` | command line, scrolling loop and calibration |
| `src/x11.rs` | X11: capture, selection overlay and scrolling via XTest |
| `src/selection.rs` | selection geometry (handles, move, resize) |
| `src/stitch.rs` | frame stitching, side cropping and scrollbar detection |
| `src/annot.rs` | annotations and PNG rendering |
| `src/editor.rs` | editor screen |
| `src/gallery.rs` | gallery screen |
| `src/app.rs` | window switching between gallery and editor |

To port to another system or to Wayland, start with `x11.rs`: capture, selection and scrolling are the only parts tied to X11.

### Reporting a problem

Open an [issue](https://github.com/Alessandro-filho/scrollshot/issues) with:

1. Distribution, desktop environment and whether the session is X11.
2. What you captured (browser, file manager...) and what you expected.
3. The output of `SCROLLSHOT_DEBUG=1 scrollshot`.
4. If possible, the resulting image.

## License

[MIT](LICENSE). The DejaVu fonts in `assets/` have their own license ([Bitstream Vera](assets/LICENSE-DejaVu.txt)).
