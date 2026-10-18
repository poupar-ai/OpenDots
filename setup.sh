#!/usr/bin/env bash
# open-poupar — setup interativo.
#
#   bash setup.sh                       # de dentro do clone
#   curl -fsSL https://raw.githubusercontent.com/poupar-ai/open-poupar.ai/main/setup.sh | bash
#
# Pergunta como instalar (release pronta ou código-fonte), onde, e se
# pode registrar no ~/.claude/settings.json — mostrando o que vai mudar e
# fazendo backup antes. Não reimplementa nada: o modo release delega pro
# `install.sh` e o modo fonte pro `make build` + `poupar install`.
#
# Lê as respostas de /dev/tty, então funciona mesmo canalizado do curl.
# Sem terminal (CI, Docker sem -t), aborta e aponta pro `install.sh`,
# que é o caminho não-interativo.
#
# As mesmas variáveis do install.sh viram os defaults das perguntas:
# POUPAR_VERSION, POUPAR_HOME, POUPAR_LINK_DIR, POUPAR_REPO.

set -euo pipefail

REPO="${POUPAR_REPO:-poupar-ai/open-poupar.ai}"
RAW_BASE="${POUPAR_RAW_BASE:-https://raw.githubusercontent.com/$REPO/main}"
DEFAULT_HOME="${POUPAR_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/open-poupar}"
DEFAULT_LINK_DIR="${POUPAR_LINK_DIR:-$HOME/.local/bin}"
SETTINGS="$HOME/.claude/settings.json"

say()  { printf '%s\n' "$*"; }
info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m✓\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }
dim()  { printf '\033[2m%s\033[0m\n' "$*"; }

has() { command -v "$1" >/dev/null 2>&1; }

# ─── perguntas ───────────────────────────────────────────────────────
#
# Tudo vai pra /dev/tty: stdin pode ser o próprio script (curl | bash),
# e stdout pode estar redirecionado pra log.

TTY=/dev/tty

# ask <variável> <pergunta> [default]
ask() {
  local __var="$1" prompt="$2" def="${3:-}" reply
  if [ -n "$def" ]; then
    printf '\033[1m?\033[0m %s \033[2m[%s]\033[0m ' "$prompt" "$def" >"$TTY"
  else
    printf '\033[1m?\033[0m %s ' "$prompt" >"$TTY"
  fi
  IFS= read -r reply <"$TTY" || reply=""
  printf -v "$__var" '%s' "${reply:-$def}"
}

# confirm <pergunta> <s|n>  → 0 se sim
confirm() {
  local prompt="$1" def="${2:-s}" hint reply
  if [ "$def" = "s" ]; then hint="S/n"; else hint="s/N"; fi
  while true; do
    printf '\033[1m?\033[0m %s \033[2m[%s]\033[0m ' "$prompt" "$hint" >"$TTY"
    IFS= read -r reply <"$TTY" || reply=""
    reply="$(printf '%s' "${reply:-$def}" | tr '[:upper:]' '[:lower:]')"
    case "$reply" in
      s|sim|y|yes) return 0 ;;
      n|nao|não|no) return 1 ;;
    esac
  done
}

# choose <variável> <pergunta> <default> <opção>...  → número escolhido
choose() {
  local __var="$1" prompt="$2" def="$3" reply i
  shift 3
  printf '\033[1m?\033[0m %s\n' "$prompt" >"$TTY"
  i=1
  for opt in "$@"; do
    printf '    %d) %s\n' "$i" "$opt" >"$TTY"
    i=$((i + 1))
  done
  while true; do
    printf '  escolha \033[2m[%s]\033[0m ' "$def" >"$TTY"
    IFS= read -r reply <"$TTY" || reply=""
    reply="${reply:-$def}"
    if [[ "$reply" =~ ^[0-9]+$ ]] && [ "$reply" -ge 1 ] && [ "$reply" -le $# ]; then
      printf -v "$__var" '%s' "$reply"
      return
    fi
  done
}

# Expande ~ no começo de um caminho digitado — `read` não faz isso.
expand_path() {
  case "$1" in
    "~")   printf '%s' "$HOME" ;;
    "~/"*) printf '%s/%s' "$HOME" "${1#\~/}" ;;
    *)     printf '%s' "$1" ;;
  esac
}

# ─── ambiente ────────────────────────────────────────────────────────

# Diretório deste script quando rodado de um arquivo; vazio quando veio
# canalizado do curl (aí BASH_SOURCE não aponta pra nada no disco).
script_dir() {
  local src="${BASH_SOURCE[0]:-}"
  if [ -n "$src" ] && [ -f "$src" ]; then
    (cd "$(dirname "$src")" && pwd)
  fi
}

# É um clone do open-poupar se tem o workspace Rust e o marketplace.
is_clone() {
  [ -n "$1" ] && [ -f "$1/v2/Cargo.toml" ] && [ -f "$1/.claude-plugin/marketplace.json" ]
}

check_env() {
  info "verificando o ambiente"
  local os arch
  os="$(uname -s)"; arch="$(uname -m)"
  case "$os" in
    Linux|Darwin) ok "sistema: $os $arch" ;;
    *) die "sistema não suportado: $os (suportados: Linux, macOS)" ;;
  esac

  if has claude; then
    ok "Claude Code: $(claude --version 2>/dev/null | head -1 || echo encontrado)"
  else
    warn "comando 'claude' não está no PATH — o plugin instala, mas só roda dentro do Claude Code"
    dim "  https://claude.ai/code"
  fi

  has curl || die "precisa do comando 'curl' no PATH"
  has tar  || die "precisa do comando 'tar' no PATH"
}

# ─── settings.json ───────────────────────────────────────────────────

backup_settings() {
  [ -f "$SETTINGS" ] || return 0
  local bak
  bak="$SETTINGS.bak-$(date +%Y%m%d-%H%M%S)"
  cp "$SETTINGS" "$bak"
  ok "backup do settings.json: $bak"
}

# Mostra o que o `poupar install` vai mexer. O merge é feito pelo CLI;
# aqui é só pra pessoa saber antes de dizer sim.
explain_register() {
  local dir="$1" mcp="$2"
  say
  say "  O registro adiciona 3 chaves em $SETTINGS"
  say "  (merge — o resto do arquivo fica como está):"
  dim "    extraKnownMarketplaces.open-poupar → $dir"
  dim "    enabledPlugins.open-poupar@open-poupar → true"
  dim "    mcpServers.open-poupar → $mcp"
  say
}

# ─── modo release ────────────────────────────────────────────────────

run_release() {
  local src_dir="$1" version home link register installer

  ask version "Versão (tag, ex.: v0.1.0; vazio = última release):" "${POUPAR_VERSION:-}"
  ask home "Diretório de instalação:" "$DEFAULT_HOME"
  home="$(expand_path "$home")"
  ask link "Onde criar o atalho do CLI 'poupar':" "$DEFAULT_LINK_DIR"
  link="$(expand_path "$link")"

  explain_register "$home" "$home/plugin/bin/poupar-mcp"
  register=1
  confirm "Registrar o plugin no Claude Code agora?" s || register=0

  say
  say "  Resumo"
  say "    modo:      release pronta"
  say "    versão:    ${version:-última}"
  say "    destino:   $home"
  say "    atalho:    $link/poupar"
  say "    registrar: $([ "$register" = 1 ] && echo sim || echo não)"
  say
  confirm "Continuar?" s || die "cancelado — nada foi alterado"

  # Prefere o install.sh do lado deste script (clone ou cópia baixada);
  # senão baixa o da main. Em nenhum dos dois casos é `curl | bash`
  # aninhado: vai pro disco primeiro, e falha alto se o download falhar.
  if [ -n "$src_dir" ] && [ -f "$src_dir/install.sh" ]; then
    installer="$src_dir/install.sh"
  else
    installer="$(mktemp)"
    # shellcheck disable=SC2064  # $installer precisa ser expandido agora
    trap "rm -f '$installer'" EXIT
    curl -fsSL "$RAW_BASE/install.sh" -o "$installer" \
      || die "não consegui baixar $RAW_BASE/install.sh"
  fi

  [ "$register" = 1 ] && backup_settings

  POUPAR_VERSION="$version" \
  POUPAR_HOME="$home" \
  POUPAR_LINK_DIR="$link" \
  POUPAR_REPO="$REPO" \
  POUPAR_NO_REGISTER="$([ "$register" = 1 ] && echo "" || echo 1)" \
    bash "$installer" </dev/null

  [ "$register" = 1 ] && "$home/plugin/bin/poupar" status || true
}

# ─── modo código-fonte ───────────────────────────────────────────────

run_source() {
  local src_dir="$1" repo_dir build_ui register cli

  if is_clone "$src_dir"; then
    repo_dir="$src_dir"
    ok "usando o clone em $repo_dir"
  else
    has git || die "precisa do 'git' pra clonar o código-fonte"
    ask repo_dir "Onde clonar o repositório:" "$HOME/open-poupar.ai"
    repo_dir="$(expand_path "$repo_dir")"
  fi

  has cargo || die "precisa do Rust (cargo) — instale com: curl https://sh.rustup.rs -sSf | sh"
  ok "Rust: $(cargo --version)"

  build_ui=0
  if has npm; then
    ok "Node: $(node --version 2>/dev/null)"
    confirm "Buildar a UI web também (ui-next, precisa de npm)?" s && build_ui=1
  else
    warn "npm não encontrado — a UI web fica sem build (hooks e MCP funcionam sem ela)"
  fi

  explain_register "$repo_dir" "$repo_dir/v2/target/release/poupar-mcp"
  register=1
  confirm "Registrar o plugin no Claude Code depois do build?" s || register=0

  say
  say "  Resumo"
  say "    modo:      código-fonte"
  say "    repo:      $repo_dir"
  say "    UI:        $([ "$build_ui" = 1 ] && echo buildar || echo pular)"
  say "    registrar: $([ "$register" = 1 ] && echo sim || echo não)"
  dim "    (o primeiro build do Rust leva alguns minutos)"
  say
  confirm "Continuar?" s || die "cancelado — nada foi alterado"

  if [ ! -d "$repo_dir/.git" ]; then
    info "clonando $REPO em $repo_dir"
    git clone "https://github.com/$REPO.git" "$repo_dir"
  fi

  info "compilando os binários (cargo build --release)"
  make -C "$repo_dir" --no-print-directory build

  if [ "$build_ui" = 1 ]; then
    info "buildando a UI"
    make -C "$repo_dir" --no-print-directory ui
  fi

  cli="$repo_dir/v2/target/release/poupar"
  [ -x "$cli" ] || die "build terminou mas $cli não existe"

  if [ "$register" = 1 ]; then
    backup_settings
    info "registrando o plugin no Claude Code"
    "$cli" install "$repo_dir"
    "$cli" status
  else
    warn "registro pulado — quando quiser: make -C $repo_dir install"
  fi
}

# ─── main ────────────────────────────────────────────────────────────

main() {
  if ! { : >"$TTY"; } 2>/dev/null; then
    die "sem terminal interativo. Pra instalar sem perguntas use o install.sh:
    curl -fsSL $RAW_BASE/install.sh | bash"
  fi

  say
  printf '\033[1m  open-poupar — setup\033[0m\n'
  dim "  memória persistente e um time de agentes pro Claude Code"
  say

  check_env
  say

  local src_dir mode def
  src_dir="$(script_dir)"

  # Dentro de um clone o natural é buildar dele; fora, baixar o binário.
  def=1
  is_clone "$src_dir" && def=2

  choose mode "Como você quer instalar?" "$def" \
    "Release pronta — baixa o binário, não precisa de Rust nem Node" \
    "Código-fonte — compila com cargo (precisa de Rust 1.88+)"
  say

  case "$mode" in
    1) run_release "$src_dir" ;;
    2) run_source "$src_dir" ;;
  esac

  say
  ok "pronto"
  say "  1. Reinicie o Claude Code."
  say "  2. Dentro de um projeto, rode /mem-setup."
  say
}

main "$@"
