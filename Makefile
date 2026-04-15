SHELL := /bin/bash
INSTALL_DIR := $(shell pwd)

# Resolve Node.js — prefers nvm, falls back to system node
NODE := $(shell ls $(HOME)/.nvm/versions/node/*/bin/node 2>/dev/null | sort -V | tail -1)
ifeq ($(NODE),)
NODE := node
endif

.PHONY: install uninstall status help

help:
	@echo ""
	@echo "  project-man — comandos disponíveis"
	@echo ""
	@echo "  make install    Registra o plugin no Claude Code"
	@echo "  make uninstall  Remove o plugin do Claude Code"
	@echo "  make status     Verifica se o plugin está instalado"
	@echo ""

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
