#!/usr/bin/env bash
# Instala o scrollshot para o usuário atual (sem sudo).
#
#   ./install.sh                     compila, instala o binário e as entradas do menu
#   ./install.sh --cinnamon-shortcut também cria o atalho Super+Shift+S no Cinnamon
#   ./install.sh --uninstall         remove o que foi instalado
#
# Variáveis: PREFIX (padrão ~/.local).
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/scrollshot"
APPS="$PREFIX/share/applications"
SHORTCUT_ID="scrollshot"
SHORTCUT_PATH="/org/cinnamon/desktop/keybindings/custom-keybindings/$SHORTCUT_ID/"

cd "$(dirname "$0")"

uninstall() {
    rm -f "$BIN" "$APPS/scrollshot.desktop" "$APPS/scrollshot-galeria.desktop"
    if command -v gsettings >/dev/null && command -v dconf >/dev/null \
        && gsettings get org.cinnamon.desktop.keybindings custom-list >/dev/null 2>&1 \
        && [[ "$(dconf read "${SHORTCUT_PATH}command")" == "'$BIN'" ]]; then
        # Só remove o atalho se ele aponta para este binário (outro PREFIX não mexe nele).
        dconf reset -f "$SHORTCUT_PATH"
        python3 - "$SHORTCUT_ID" <<'EOF'
import ast, subprocess, sys
key = ["org.cinnamon.desktop.keybindings", "custom-list"]
cur = ast.literal_eval(subprocess.check_output(["gsettings", "get", *key]).decode().strip())
if sys.argv[1] in cur:
    cur.remove(sys.argv[1])
    subprocess.check_call(["gsettings", "set", *key, str(cur)])
EOF
    fi
    echo "scrollshot removido."
}

desktop_entry() { # nome, comentário, argumentos, arquivo
    cat >"$APPS/$4" <<EOF
[Desktop Entry]
Type=Application
Name=$1
Comment=$2
Exec=$BIN$3
Icon=org.gnome.Screenshot
Terminal=false
Categories=Graphics;
Keywords=print;screenshot;captura;rolagem;scroll;
EOF
}

cinnamon_shortcut() {
    if ! gsettings get org.cinnamon.desktop.keybindings custom-list >/dev/null 2>&1; then
        echo "Cinnamon não encontrado; crie o atalho manualmente apontando para: $BIN"
        return
    fi
    dconf write "${SHORTCUT_PATH}name" "'Scrollshot'"
    dconf write "${SHORTCUT_PATH}command" "'$BIN'"
    dconf write "${SHORTCUT_PATH}binding" "['<Shift><Super>s']"
    python3 - "$SHORTCUT_ID" <<'EOF'
import ast, subprocess, sys
key = ["org.cinnamon.desktop.keybindings", "custom-list"]
cur = ast.literal_eval(subprocess.check_output(["gsettings", "get", *key]).decode().strip())
if sys.argv[1] not in cur:
    # O Cinnamon só recarrega a lista se a entrada nova vier antes de "__dummy__".
    i = cur.index("__dummy__") if "__dummy__" in cur else len(cur)
    cur.insert(i, sys.argv[1])
    subprocess.check_call(["gsettings", "set", *key, str(cur)])
EOF
    echo "Atalho Super+Shift+S criado."
}

case "${1:-}" in
--uninstall)
    uninstall
    exit 0
    ;;
esac

for dep in xclip gio xdg-open; do
    command -v "$dep" >/dev/null || echo "aviso: '$dep' não encontrado (veja as dependências no README)"
done

cargo build --release
install -Dm755 target/release/scrollshot "$BIN"
mkdir -p "$APPS"
desktop_entry "Scrollshot" "Print de área, com ou sem rolagem, e editor de anotações" "" scrollshot.desktop
desktop_entry "Scrollshot Galeria" "Ver e editar as capturas salvas" " --gallery" scrollshot-galeria.desktop
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" 2>/dev/null || true

if [[ "${1:-}" == "--cinnamon-shortcut" ]]; then
    cinnamon_shortcut
fi

echo "Instalado em $BIN"
case ":$PATH:" in
*":$PREFIX/bin:"*) ;;
*) echo "aviso: $PREFIX/bin não está no PATH" ;;
esac
