# scrollshot

[Português](README.md) · [English](README.en.md) · **Español**

Capturas de área para Linux con **desplazamiento automático**: seleccionas la parte de la pantalla que se desplaza (una página web, una lista, un panel) y scrollshot la desplaza por ti y une todo en una sola imagen larga. Incluye un editor de anotaciones y una galería de tus capturas.

> Por ahora funciona solo en **Linux con X11** (por ejemplo Cinnamon, MATE, Xfce, o GNOME/KDE en sesión X11). Wayland todavía no es compatible.

## Funciones

- **Captura con desplazamiento**: desplaza el contenido bajo la selección y une los fragmentos sin repetir ni cortar líneas.
  - Los encabezados y pies fijos aparecen una sola vez.
  - Los menús laterales fijos y la barra de desplazamiento se recortan automáticamente.
  - Calibra cuánto desplazar en cada paso y se detiene al final del contenido.
- **Captura normal** del área seleccionada, sin desplazar.
- **Selección ajustable** antes de capturar: controladores en las esquinas y los lados, y se puede mover arrastrando por dentro.
- **Editor de anotaciones**: flecha, rectángulo, resaltador, lápiz, texto (con o sin caja), numeración y pixelado (para ocultar datos sensibles). Las anotaciones se pueden seleccionar, mover, redimensionar, editar y borrar, con deshacer y rehacer.
- **Galería** de las capturas guardadas, con miniaturas, para abrir, editar, copiar o enviar a la papelera.
- **Copiar** la imagen al portapapeles.

## Instalación

### Dependencias

- [Rust](https://rustup.rs) 1.88 o más reciente.
- Una sesión **X11** con la extensión XTest (activada por defecto en los servidores X).
- Programas usados en tiempo de ejecución:

```sh
sudo apt install xclip libglib2.0-bin xdg-utils libgl1 libxkbcommon-x11-0
```

(`xclip` copia la imagen, `gio` envía a la papelera, `xdg-open` abre la carpeta.)

### Compilar e instalar

```sh
git clone https://github.com/Alessandro-filho/scrollshot.git
cd scrollshot
./install.sh
```

El script compila el proyecto, instala el binario en `~/.local/bin/scrollshot` y agrega **Scrollshot** y **Scrollshot Galeria** al menú de aplicaciones. No usa `sudo`.

- `./install.sh --cinnamon-shortcut` también crea el atajo **Super+Shift+S** en Cinnamon.
- En otros escritorios, crea un atajo de teclado que ejecute `~/.local/bin/scrollshot`.
- `./install.sh --uninstall` lo elimina todo.
- `PREFIX=/otro/lugar ./install.sh` instala en otro prefijo.

## Uso

```sh
scrollshot                    # selecciona el área y captura
scrollshot --gallery          # abre la galería
scrollshot --edit foto.png    # abre el editor con una imagen existente
scrollshot --no-edit          # captura sin abrir el editor
scrollshot archivo.png        # guarda con este nombre
```

### Selección

| Acción | Resultado |
|---|---|
| Arrastrar | crea la selección |
| Arrastrar un controlador | redimensiona |
| Arrastrar por dentro | mueve la selección |
| **Espacio** o doble clic | captura **con desplazamiento** |
| **Enter** | captura **normal**, solo el área visible |
| Esc | cancela |

No muevas el ratón mientras se desplaza: un movimiento grande detiene la captura y guarda lo que ya se unió.

Selecciona solo el área que se desplaza. Si la selección incluye un menú lateral fijo o la barra de desplazamiento, se recortan de la imagen.

### Editor

| Tecla | Herramienta |
|---|---|
| V | Seleccionar (mover, redimensionar con los controladores, Supr borra, doble clic edita el texto) |
| A | Flecha |
| R | Rectángulo |
| M | Resaltador |
| P | Lápiz |
| T | Texto (la opción "caixa" activa o desactiva el fondo) |
| N | Número |
| B | Pixelar |

Pulsar de nuevo la tecla de la herramienta activa vuelve a Seleccionar. Ctrl+Z deshace, Ctrl+Shift+Z o Ctrl+Y rehace, Ctrl+S guarda, Ctrl+C copia y Ctrl+rueda del ratón hace zoom.

Por ahora la interfaz está en portugués.

### Dónde se guardan las capturas

En `~/Imágenes/scrollshot/` (la carpeta de imágenes de tu usuario, según el idioma del sistema). La captura se guarda en cuanto termina; en el editor, **Salvar** (Guardar) escribe las anotaciones sobre el mismo archivo.

### Variables de entorno

| Variable | Efecto |
|---|---|
| `SCROLLSHOT_DIR` | cambia la carpeta de las capturas y de la galería |
| `SCROLLSHOT_DEBUG=1` | muestra en la terminal cada paso del desplazamiento (útil para reportar problemas) |

## Contribuir

Las contribuciones son bienvenidas: correcciones, compatibilidad con otros entornos (Wayland, por ejemplo), nuevas herramientas en el editor y traducciones.

### Preparar el entorno

```sh
git clone https://github.com/Alessandro-filho/scrollshot.git
cd scrollshot
cargo build
cargo run            # captura
cargo run -- --gallery
```

### Antes de abrir un pull request

```sh
cargo fmt
cargo clippy --release   # sin advertencias
cargo test               # todas las pruebas pasando
```

- Los cambios en `stitch.rs`, `annot.rs` o `selection.rs` deben incluir pruebas. Estas partes no dependen de la pantalla, así que todo se puede probar con imágenes sintéticas (mira las pruebas existentes).
- Al corregir un caso de unión, primero escribe una prueba que reproduzca el problema y falle, y después corrígelo.
- Los comentarios en el código explican el **porqué**, no lo obvio.
- Mensajes de commit cortos y claros, que describan qué cambió y por qué.

### Organización del código

| Archivo | Responsabilidad |
|---|---|
| `src/main.rs` | línea de comandos, bucle de desplazamiento y calibración |
| `src/x11.rs` | X11: captura, superposición de selección y desplazamiento vía XTest |
| `src/selection.rs` | geometría de la selección (controladores, mover, redimensionar) |
| `src/stitch.rs` | unión de los fotogramas, recortes laterales y detección de la barra de desplazamiento |
| `src/annot.rs` | anotaciones y renderizado en PNG |
| `src/editor.rs` | pantalla del editor |
| `src/gallery.rs` | pantalla de la galería |
| `src/app.rs` | ventana que alterna entre galería y editor |

Para portar a otro sistema o a Wayland, el punto de partida es `x11.rs`: captura, selección y desplazamiento son las únicas partes ligadas a X11.

### Reportar un problema

Abre un [issue](https://github.com/Alessandro-filho/scrollshot/issues) con:

1. Distribución, entorno de escritorio y si la sesión es X11.
2. Qué capturaste (navegador, gestor de archivos...) y qué esperabas.
3. La salida de `SCROLLSHOT_DEBUG=1 scrollshot`.
4. Si es posible, la imagen generada.

## Licencia

[MIT](LICENSE). Las fuentes DejaVu en `assets/` tienen su propia licencia ([Bitstream Vera](assets/LICENSE-DejaVu.txt)).
