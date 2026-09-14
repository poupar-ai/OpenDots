# open-poupar v2

Port em Rust do plugin `open-poupar` (Node.js CJS em `../plugin/`).

> **Status:** skeleton (Fase 0). Ainda não executável.
> **Plano completo:** [`../docs/migrato_to_rust.md`](../docs/migrato_to_rust.md).

## Regra de ouro

O `.cjs` em `../plugin/` e `../scripts/` **continua sendo a versão oficial** até a paridade estar 100% e o cutover ser feito. Nada aqui deve impactar a versão em produção.

## Estrutura (será preenchida nas próximas fases)

```
v2/
├── crates/          # crates do workspace (Fase 1 em diante)
├── tests/           # paridade .cjs ↔ Rust
├── Cargo.toml       # workspace root
├── rust-toolchain.toml
└── settings.toml    # branding/URLs (Fase 0.2)
```

## Build (quando houver crates)

```bash
cargo build --workspace --release
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```
