#!/usr/bin/env bash
# project-mem — desinstalador da instalação feita pelo install.sh.
#
#   curl -fsSL https://raw.githubusercontent.com/poupar-ai/open-poupar.ai/main/uninstall.sh | bash
#
# Tira o registro do ~/.claude/settings.json, remove o symlink do CLI e
# apaga os diretórios que o instalador criou. **Não** toca em nenhum
# `.claude-project-memory/` — a memória dos seus projetos fica onde está.
#
# Variáveis de ambiente:
#   PMEM_HOME     diretório de instalação. Default: ${XDG_DATA_HOME:-~/.local/share}/project-mem
#   PMEM_BIN_DIR  onde está o symlink do CLI. Default: ~/.local/bin

set -euo pipefail

HOME_DIR="${PMEM_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/project-mem}"
BIN_DIR="${PMEM_BIN_DIR:-$HOME/.local/bin}"

# Mesma lista do install.sh — remover só o que foi instalado deixa
# intacto qualquer coisa que o usuário tenha colocado em PMEM_HOME.
OWNED=(.claude-plugin plugin ui VERSION TARGET LICENSE README.md)

say()  { printf '%s\n' "$*"; }
info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m✓\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

if [ -d "$HOME_DIR/.git" ]; then
  die "$HOME_DIR é um clone git — use 'make uninstall' nele."
fi

# Desregistrar antes de apagar: o `pmem uninstall` é o binário que está
# dentro da instalação que vamos remover.
if [ -x "$HOME_DIR/plugin/bin/pmem" ]; then
  info "removendo o registro do Claude Code"
  "$HOME_DIR/plugin/bin/pmem" uninstall
else
  warn "$HOME_DIR/plugin/bin/pmem não encontrado — pulando o desregistro"
  warn "se o plugin ainda aparecer no Claude Code, edite ~/.claude/settings.json à mão"
fi

# Só remove o symlink se ele aponta pra esta instalação — se o usuário
# tem outro `pmem` ali, não é nosso pra apagar.
link="$BIN_DIR/pmem"
if [ -L "$link" ] && [ "$(readlink "$link")" = "$HOME_DIR/plugin/bin/pmem" ]; then
  info "removendo $link"
  rm -f "$link"
fi

info "removendo arquivos de $HOME_DIR"
for entry in "${OWNED[@]}"; do
  rm -rf "${HOME_DIR:?}/$entry"
done
# `rmdir` só apaga se ficou vazio — se sobrou algo do usuário, fica.
rmdir "$HOME_DIR" 2>/dev/null || true

say
ok "project-mem removido"
say "  A memória dos projetos (.claude-project-memory/) não foi tocada."
say "  Reinicie o Claude Code para aplicar."
