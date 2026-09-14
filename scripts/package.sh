#!/usr/bin/env bash
# package.sh — monta o tarball de release consumido pelo `install.sh`.
#
# Produz, em `dist/`:
#   project-mem-<versão>-<target>.tar.gz
#   project-mem-<versão>-<target>.tar.gz.sha256
#
# O tarball tem uma raiz única (`project-mem-<versão>-<target>/`) para
# que extrair num diretório qualquer nunca espalhe arquivo solto:
#
#   <raiz>/
#   ├── .claude-plugin/marketplace.json   ← faz o Claude Code reconhecer o plugin
#   ├── plugin/
#   │   ├── bin/                          ← binários compilados
#   │   ├── hooks/ skills/ seeds/ ...     ← assets do plugin
#   │   └── scripts/ mcp/                 ← implementação .cjs (skills que ainda usam Node)
#   ├── ui/                               ← build do Vite (ui-next/dist)
#   └── VERSION
#
# O layout é o que `core_paths::install` procura a partir do próprio
# executável: `plugin/bin/pmem-http` sobe dois níveis e acha a raiz pelo
# marcador `.claude-plugin/marketplace.json`.
#
# O cargo roda com cwd em `v2/` para que `v2/rust-toolchain.toml` valha —
# release sai sempre no toolchain pinado, não no que estiver default.
#
# uso:
#   scripts/package.sh                    # target nativo, build completo
#   scripts/package.sh --target <triple>  # cross-compile
#   scripts/package.sh --skip-ui          # reaproveita ui-next/dist (CI builda a UI uma vez só)
#   scripts/package.sh --skip-cargo       # reaproveita os binários já compilados
#   scripts/package.sh --skip-build       # os dois acima

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

TARGET=""
SKIP_CARGO=0
SKIP_UI=0
while [ $# -gt 0 ]; do
  case "$1" in
    --target)     TARGET="${2:?--target requer um triple}"; shift 2 ;;
    --skip-cargo) SKIP_CARGO=1; shift ;;
    --skip-ui)    SKIP_UI=1; shift ;;
    --skip-build) SKIP_CARGO=1; SKIP_UI=1; shift ;;
    -h|--help)    sed -n '2,36p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *)            echo "package.sh: argumento desconhecido: $1" >&2; exit 2 ;;
  esac
done

# Binários que compõem a release. Precisa bater com os `[[bin]]` dos
# crates `api-*` — o build falha alto se algum sumir.
BINARIES=(
  pmem
  pmem-http
  pmem-mcp
  pmem-hook-startup
  pmem-hook-observe
  pmem-hook-session-init
  pmem-hook-session-end
)

VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' v2/Cargo.toml | head -1)"
[ -n "$VERSION" ] || { echo "package.sh: não achei version em v2/Cargo.toml" >&2; exit 1; }

# Sem --target explícito, deriva o triple do próprio rustc para nomear o
# artefato — o nome é o que o install.sh usa pra escolher o download.
if [ -z "$TARGET" ]; then
  TARGET="$(cd v2 && rustc -vV | sed -n 's/^host: //p')"
  CARGO_TARGET_ARGS=()
  BIN_DIR="v2/target/release"
else
  CARGO_TARGET_ARGS=(--target "$TARGET")
  BIN_DIR="v2/target/$TARGET/release"
fi

NAME="project-mem-${VERSION}-${TARGET}"
STAGE="dist/$NAME"

echo "==> project-mem $VERSION → $TARGET"

if [ "$SKIP_CARGO" = "0" ]; then
  echo "==> cargo build --release"
  ( cd v2 && cargo build --release \
      "${CARGO_TARGET_ARGS[@]}" \
      --bin pmem --bin pmem-http --bin pmem-mcp \
      --bin pmem-hook-startup --bin pmem-hook-observe \
      --bin pmem-hook-session-init --bin pmem-hook-session-end )
fi

if [ "$SKIP_UI" = "0" ]; then
  echo "==> build da UI (ui-next)"
  ( cd ui-next && npm ci --no-audit --no-fund && npm run build )
fi

[ -f ui-next/dist/index.html ] || {
  echo "package.sh: ui-next/dist/index.html não existe — rode sem --skip-ui" >&2
  exit 1
}

echo "==> montando $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE/plugin/bin" "$STAGE/.claude-plugin"

for bin in "${BINARIES[@]}"; do
  src="$BIN_DIR/$bin"
  [ -f "$src" ] || { echo "package.sh: binário ausente: $src" >&2; exit 1; }
  install -m 0755 "$src" "$STAGE/plugin/bin/$bin"
done

# Assets do plugin. `plugin/bin/` já foi preenchido acima e não existe no
# repo, então não há colisão. `scripts/` e `mcp/` (.cjs) entram porque
# alguns skills — /mem-agents, /mem-index — ainda fazem `require()` neles;
# quem não tem Node perde só esses skills, não o plugin.
cp -R plugin/. "$STAGE/plugin/"
cp .claude-plugin/marketplace.json "$STAGE/.claude-plugin/"
cp LICENSE README.md "$STAGE/"

# UI buildada. Vai pra `ui/` (e não `ui-next/dist/`) porque no tarball não
# existe fonte nem node_modules — só o resultado.
mkdir -p "$STAGE/ui"
cp -R ui-next/dist/. "$STAGE/ui/"

printf '%s\n' "$VERSION" > "$STAGE/VERSION"
printf '%s\n' "$TARGET"  > "$STAGE/TARGET"

# Bit de execução dos scripts shell sobrevive ao cp, mas garantimos —
# um `chmod` esquecido vira hook que não roda na máquina do usuário.
chmod +x "$STAGE/plugin/bin-path.sh" "$STAGE/plugin/hooks/"*.sh "$STAGE/plugin/agent-bin/"*

echo "==> tar.gz"
TARBALL="dist/$NAME.tar.gz"
rm -f "$TARBALL" "$TARBALL.sha256"
tar -czf "$TARBALL" -C dist "$NAME"

# `shasum` no macOS, `sha256sum` no Linux. Formato idêntico nos dois, que
# é o que o install.sh sabe verificar.
if command -v sha256sum >/dev/null 2>&1; then
  ( cd dist && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256" )
else
  ( cd dist && shasum -a 256 "$NAME.tar.gz" > "$NAME.tar.gz.sha256" )
fi

rm -rf "$STAGE"

echo
echo "✓ $TARBALL ($(du -h "$TARBALL" | cut -f1))"
echo "✓ $TARBALL.sha256"
