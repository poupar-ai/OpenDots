SHELL := /bin/bash
INSTALL_DIR := $(shell pwd)

# O CLI é o binário compilado. `make build` gera; sem ele, install/
# uninstall/status avisam em vez de falhar com "command not found".
CLI := v2/target/release/poupar

.PHONY: install uninstall status help ui ui-dev build package smoke _need-cli

help:
	@echo ""
	@echo "  open-poupar — comandos disponíveis"
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
	@echo "Instalando open-poupar..."
	@echo "Diretório: $(INSTALL_DIR)"
	@echo ""
	@$(MAKE) --no-print-directory _need-cli
	@$(CLI) install "$(INSTALL_DIR)"
	@echo ""
	@echo "Reinicie o Claude Code para ativar o plugin."
	@echo ""

uninstall:
	@echo ""
	@$(MAKE) --no-print-directory _need-cli
	@$(CLI) uninstall
	@echo ""
	@echo "Reinicie o Claude Code para aplicar a remoção."
	@echo ""

status:
	@echo ""
	@$(MAKE) --no-print-directory _need-cli
	@$(CLI) status
	@echo ""

# Guarda: sem binário compilado não há CLI. Mensagem clara em vez de
# "No such file or directory".
_need-cli:
	@test -x "$(CLI)" || { \
	  echo "$(CLI) não existe — rode 'make build' primeiro."; \
	  exit 1; \
	}
