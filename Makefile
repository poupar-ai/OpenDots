SHELL := /bin/bash
INSTALL_DIR := $(shell pwd)

# Resolve Node.js — prefers nvm, falls back to system node
NODE := $(shell ls $(HOME)/.nvm/versions/node/*/bin/node 2>/dev/null | sort -V | tail -1)
ifeq ($(NODE),)
NODE := node
endif

.PHONY: install uninstall status help ui ui-dev build package smoke

help:
	@echo ""
	@echo "  project-man — comandos disponíveis"
	@echo ""
	@echo "  make install    Registra o plugin no Claude Code"
	@echo "  make uninstall  Remove o plugin do Claude Code"
	@echo "  make status     Verifica se o plugin está instalado"
	@echo "  make ui         Instala deps e builda a UI React (ui-next/dist)"
	@echo "  make ui-dev     Roda Vite dev server (porta 5173, proxy /api → 37778)"
	@echo "  make build      Compila os binários Rust em release"
	@echo "  make package    Monta o tarball de release em dist/"
	@echo "  make smoke      Verifica que o tarball de dist/ roda fora do clone"
	@echo ""

# `cd v2` para que v2/rust-toolchain.toml valha — mesmo pin do package.sh.
build:
	@cd v2 && cargo build --release --workspace

package:
	@bash scripts/package.sh

smoke:
	@bash scripts/smoke-package.sh

ui:
	@cd ui-next && npm install --silent && npm run build

ui-dev:
	@cd ui-next && npm run dev

install:
	@echo ""
	@echo "Instalando project-man..."
	@echo "Diretório: $(INSTALL_DIR)"
	@echo ""
	@$(NODE) scripts/install.cjs "$(INSTALL_DIR)"
	@echo ""
	@echo "Reinicie o Claude Code para ativar o plugin."
	@echo ""

uninstall:
	@echo ""
	@$(NODE) scripts/uninstall.cjs
	@echo ""
	@echo "Reinicie o Claude Code para aplicar a remoção."
	@echo ""

status:
	@echo ""
	@$(NODE) scripts/status.cjs
	@echo ""
