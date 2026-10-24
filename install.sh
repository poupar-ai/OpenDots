#!/usr/bin/env bash
# open-poupar — instalador one-liner.
#
#   curl -fsSL https://raw.githubusercontent.com/poupar-ai/OpenDots/main/install.sh | bash
#
# Baixa o tarball da release correspondente ao seu SO/arquitetura,
# confere o SHA-256, instala em ~/.local/share/open-poupar e registra o
# plugin no ~/.claude/settings.json. Não precisa de Node, Rust ou git.
#
# Variáveis de ambiente:
#   POUPAR_VERSION      tag a instalar (ex.: v0.2.0). Default: última release.
#   POUPAR_HOME         diretório de instalação. Default: ${XDG_DATA_HOME:-~/.local/share}/open-poupar
#   POUPAR_LINK_DIR      onde criar o symlink do CLI. Default: ~/.local/bin
#   POUPAR_REPO         owner/repo de origem. Default: poupar-ai/OpenDots
#   POUPAR_NO_REGISTER  =1 para instalar os arquivos sem tocar no settings.json
#   POUPAR_API_BASE     base da API de releases. Default: https://api.github.com
#   POUPAR_DL_BASE      base do download dos assets. Default: https://github.com
#
# As duas últimas existem pra mirror interno e pros testes do instalador.
#
# Rodar de novo atualiza a instalação no lugar: só os diretórios que o
# instalador criou são substituídos, o resto de POUPAR_HOME fica intacto.

set -euo pipefail

REPO="${POUPAR_REPO:-poupar-ai/OpenDots}"
VERSION="${POUPAR_VERSION:-}"
HOME_DIR="${POUPAR_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/open-poupar}"
BIN_DIR="${POUPAR_LINK_DIR:-$HOME/.local/bin}"
API_BASE="${POUPAR_API_BASE:-https://api.github.com}"
DL_BASE="${POUPAR_DL_BASE:-https://github.com}"

# Tudo que o instalador cria dentro de POUPAR_HOME. Só esses caminhos são
# removidos num upgrade — qualquer outra coisa que o usuário tenha
# colocado lá sobrevive.
OWNED=(.claude-plugin plugin ui VERSION TARGET LICENSE LICENSE-INTERNAL-USE COMMERCIAL-LICENSE.md TRADEMARKS.md README.md)

say()  { printf '%s\n' "$*"; }
info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m✓\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "precisa do comando '$1' no PATH"; }

# ─── plataforma ──────────────────────────────────────────────────────
#
# Linux sai em musl (estático, roda em qualquer distro sem depender da
# glibc do host); macOS sai nos dois arcos nativos.

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$arch" in
    x86_64|amd64)  arch=x86_64 ;;
    arm64|aarch64) arch=aarch64 ;;
    *) die "arquitetura não suportada: $arch (suportadas: x86_64, aarch64)" ;;
  esac

  case "$os" in
    Linux)  printf '%s-unknown-linux-musl\n' "$arch" ;;
    Darwin) printf '%s-apple-darwin\n' "$arch" ;;
    *) die "sistema não suportado: $os (suportados: Linux, macOS)" ;;
  esac
}

# ─── release ─────────────────────────────────────────────────────────

# Imprime a última tag publicada em stdout. O motivo de falhar vai no
# código de saída, e não numa variável: a função é chamada dentro de
# `$(...)`, que roda em subshell — variável setada aqui não chega ao
# chamador.
#
#   0  tag em stdout
#   3  repositório sem nenhuma release (404)
#   4  não consegui falar com a API (rede, proxy, rate limit)
#   5  API respondeu, mas sem `tag_name`
latest_tag() {
  local body http tag
  body="$(curl -sSL -w '\n%{http_code}' "$API_BASE/repos/$REPO/releases/latest" 2>/dev/null)" || return 4
  http="${body##*$'\n'}"
  body="${body%$'\n'*}"

  [ "$http" = "404" ] && return 3
  [ "$http" = "200" ] || return 4

  tag="$(printf '%s' "$body" | sed -n 's/.*"tag_name" *: *"\([^"]*\)".*/\1/p' | head -1)"
  [ -n "$tag" ] || return 5
  printf '%s' "$tag"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# ─── install ─────────────────────────────────────────────────────────

main() {
  need curl
  need tar

  local target tag asset url tmp tarball expected actual extracted
  target="$(detect_target)"

  tag="$VERSION"
  if [ -z "$tag" ]; then
    info "procurando a última release de $REPO"
    local rc=0
    tag="$(latest_tag)" || rc=$?
    if [ "$rc" != "0" ]; then
      case "$rc" in
        3) warn "o repositório $REPO ainda não publicou nenhuma release" ;;
        4) warn "não consegui falar com $API_BASE (rede, proxy ou rate limit)" ;;
        *) warn "a API respondeu sem 'tag_name'" ;;
      esac
      say
      say "  Se você é quem mantém o projeto: publique a primeira release com"
      say "      git tag v0.1.0 && git push origin v0.1.0"
      say "  O workflow de release compila os binários e publica os tarballs."
      say
      say "  Se você quer instalar uma versão específica que já existe:"
      say "      curl -fsSL <url>/install.sh | POUPAR_VERSION=v0.1.0 bash"
      say
      say "  Enquanto não há release, dá pra instalar do código-fonte:"
      say "      git clone https://github.com/$REPO.git && cd ${REPO##*/}"
      say "      make build && make install"
      die "instalação abortada — nada a baixar"
    fi
  fi

  # A tag é `vX.Y.Z`; o asset usa a versão sem o `v`.
  asset="open-poupar-${tag#v}-${target}.tar.gz"
  url="$DL_BASE/$REPO/releases/download/$tag/$asset"

  info "open-poupar $tag ($target)"

  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064  # $tmp precisa ser expandido agora, não no trap
  trap "rm -rf '$tmp'" EXIT

  tarball="$tmp/$asset"
  info "baixando $asset"
  curl -fsSL --retry 3 -o "$tarball" "$url" \
    || die "download falhou: $url"

  info "conferindo SHA-256"
  curl -fsSL --retry 3 -o "$tarball.sha256" "$url.sha256" \
    || die "não achei o checksum: $url.sha256"
  expected="$(cut -d' ' -f1 < "$tarball.sha256")"
  actual="$(sha256_of "$tarball")"
  [ "$expected" = "$actual" ] \
    || die "checksum não bate (esperado $expected, obtido $actual). Não instalei nada."

  tar -xzf "$tarball" -C "$tmp"
  extracted="$tmp/open-poupar-${tag#v}-${target}"
  [ -d "$extracted" ] || die "tarball com layout inesperado: falta $extracted"

  # Um clone git em POUPAR_HOME é instalação de fonte — sobrescrever ali
  # apagaria trabalho do usuário. `make install` é o caminho de lá.
  if [ -d "$HOME_DIR/.git" ]; then
    die "$HOME_DIR é um clone git. Use 'make install' nele, ou aponte POUPAR_HOME pra outro lugar."
  fi

  info "instalando em $HOME_DIR"
  mkdir -p "$HOME_DIR"
  for entry in "${OWNED[@]}"; do
    rm -rf "${HOME_DIR:?}/$entry"
    if [ -e "$extracted/$entry" ]; then
      mv "$extracted/$entry" "$HOME_DIR/$entry"
    fi
  done

  info "symlink do CLI em $BIN_DIR/poupar"
  mkdir -p "$BIN_DIR"
  ln -sf "$HOME_DIR/plugin/bin/poupar" "$BIN_DIR/poupar"

  if [ "${POUPAR_NO_REGISTER:-}" = "1" ]; then
    warn "POUPAR_NO_REGISTER=1 — settings.json não foi tocado"
  else
    info "registrando o plugin no Claude Code"
    "$HOME_DIR/plugin/bin/poupar" install "$HOME_DIR"
  fi

  say
  ok "open-poupar $tag instalado"
  say "  Diretório: $HOME_DIR"
  say "  CLI:       $BIN_DIR/poupar"
  say

  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR não está no PATH. Adicione ao seu shell rc:"
       say "    export PATH=\"$BIN_DIR:\$PATH\"" ;;
  esac

  say "Reinicie o Claude Code para ativar o plugin."
  say "Depois, dentro de um projeto, rode /mem-setup."
}

main "$@"
