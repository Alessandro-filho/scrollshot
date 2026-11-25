# scrollshot

**Português** · [English](README.en.md) · [Español](README.es.md)

Print de área para Linux com **rolagem automática**: você seleciona a parte da tela que rola (uma página, uma lista, um painel) e o scrollshot rola sozinho e emenda tudo numa imagem comprida. Inclui um editor de anotações e uma galeria das capturas.

> Por enquanto funciona só em **Linux com X11** (por exemplo Cinnamon, MATE, Xfce ou GNOME/KDE em sessão X11). Wayland ainda não é suportado.

## Recursos

- **Print com rolagem**: rola o conteúdo sob a seleção e emenda os trechos sem repetir nem cortar linhas.
  - Cabeçalhos e rodapés fixos aparecem uma vez só.
  - Menus laterais parados e a barra de rolagem são cortados automaticamente.
  - Calibra sozinho quanto rolar a cada passo e para no fim do conteúdo.
- **Print normal** da área selecionada, sem rolar.
- **Seleção ajustável** antes de capturar: alças nos cantos e nos lados, e dá para mover arrastando por dentro.
- **Editor de anotações**: seta, retângulo, marca-texto, caneta, texto (com ou sem caixa), numeração e pixelar (para esconder dados sensíveis). As anotações podem ser selecionadas, movidas, redimensionadas, editadas e apagadas, com desfazer e refazer.
- **Galeria** das capturas salvas, com miniaturas, para abrir, editar, copiar ou mandar para a lixeira.
- **Copiar** a imagem para a área de transferência.

## Instalação

### Dependências

- [Rust](https://rustup.rs) 1.88 ou mais recente.
- Sessão **X11** com a extensão XTest (padrão nos servidores X).
- Programas usados em tempo de execução:

```sh
sudo apt install xclip libglib2.0-bin xdg-utils libgl1 libxkbcommon-x11-0
```

(`xclip` copia a imagem, `gio` manda para a lixeira, `xdg-open` abre a pasta.)

### Compilar e instalar

```sh
git clone https://github.com/Alessandro-filho/scrollshot.git
cd scrollshot
./install.sh
```

O script compila, instala o binário em `~/.local/bin/scrollshot` e cria as entradas **Scrollshot** e **Scrollshot Galeria** no menu de aplicativos. Ele não usa `sudo`.

- `./install.sh --cinnamon-shortcut` também cria o atalho **Super+Shift+S** no Cinnamon.
- Em outros ambientes, crie um atalho de teclado que execute `~/.local/bin/scrollshot`.
- `./install.sh --uninstall` remove tudo.
- `PREFIX=/outro/lugar ./install.sh` instala em outro prefixo.

## Uso

```sh
scrollshot                    # seleciona a área e captura
scrollshot --gallery          # abre a galeria
scrollshot --edit foto.png    # abre o editor numa imagem existente
scrollshot --no-edit          # captura sem abrir o editor
scrollshot arquivo.png        # salva com este nome
```

### Seleção

| Ação | Efeito |
|---|---|
| Arrastar | cria a seleção |
| Arrastar uma alça | redimensiona |
| Arrastar por dentro | move a seleção |
| **Espaço** ou duplo clique | print **com rolagem** |
| **Enter** | print **normal**, só a área visível |
| Esc | cancela |

Durante a rolagem não mexa no mouse: um movimento grande interrompe a captura e salva o que já foi emendado.

Selecione só a área que rola. Se a seleção pegar um menu lateral parado ou a barra de rolagem, eles são cortados da imagem.

### Editor

| Tecla | Ferramenta |
|---|---|
| V | Selecionar (mover, redimensionar pelas alças, Delete apaga, duplo clique edita texto) |
| A | Seta |
| R | Retângulo |
| M | Marca-texto |
| P | Caneta |
| T | Texto (a opção "caixa" liga ou desliga o fundo) |
| N | Número |
| B | Pixelar |

Apertar a tecla da ferramenta ativa de novo volta para Selecionar. Ctrl+Z desfaz, Ctrl+Shift+Z ou Ctrl+Y refaz, Ctrl+S salva, Ctrl+C copia e Ctrl+roda do mouse dá zoom.

### Onde as capturas ficam

Em `~/Imagens/scrollshot/` (a pasta de imagens do seu usuário). A captura é salva assim que termina; no editor, **Salvar** grava as anotações por cima do mesmo arquivo.

### Variáveis de ambiente

| Variável | Efeito |
|---|---|
| `SCROLLSHOT_DIR` | troca a pasta das capturas e da galeria |
| `SCROLLSHOT_DEBUG=1` | mostra no terminal cada passo da rolagem (útil para relatar problemas) |

## Contribuindo

Contribuições são bem-vindas: correções, suporte a outros ambientes (Wayland, por exemplo), novas ferramentas no editor e traduções.

### Preparar o ambiente

```sh
git clone https://github.com/Alessandro-filho/scrollshot.git
cd scrollshot
cargo build
cargo run            # captura
cargo run -- --gallery
```

### Antes de abrir um pull request

```sh
cargo fmt
cargo clippy --release   # sem avisos
cargo test               # todos passando
```

- Mudanças em `stitch.rs`, `annot.rs` ou `selection.rs` devem vir com testes. Essas partes não dependem da tela, então dá para testar tudo com imagens sintéticas (veja os testes existentes).
- Ao corrigir um caso de emenda, primeiro escreva um teste que reproduz o problema e falha, depois corrija.
- Comentários no código explicam o **porquê**, não o óbvio.
- Mensagens de commit curtas e claras, descrevendo o que mudou e por quê.

### Organização do código

| Arquivo | Responsabilidade |
|---|---|
| `src/main.rs` | linha de comando, loop de rolagem e calibração |
| `src/x11.rs` | X11: captura, overlay de seleção e rolagem via XTest |
| `src/selection.rs` | geometria da seleção (alças, mover, redimensionar) |
| `src/stitch.rs` | emenda dos frames, cortes laterais e detecção da barra de rolagem |
| `src/annot.rs` | anotações e renderização no PNG |
| `src/editor.rs` | tela do editor |
| `src/gallery.rs` | tela da galeria |
| `src/app.rs` | janela que alterna entre galeria e editor |

Para portar para outro sistema ou para Wayland, o ponto de partida é o `x11.rs`: captura, seleção e rolagem são as únicas partes ligadas ao X11.

### Relatar um problema

Abra uma [issue](https://github.com/Alessandro-filho/scrollshot/issues) com:

1. Distribuição, ambiente gráfico e se a sessão é X11.
2. O que você capturou (navegador, gerenciador de arquivos...) e o que esperava.
3. A saída de `SCROLLSHOT_DEBUG=1 scrollshot`.
4. Se possível, a imagem gerada.

## Licença

[MIT](LICENSE). As fontes DejaVu em `assets/` têm licença própria ([Bitstream Vera](assets/LICENSE-DejaVu.txt)).
