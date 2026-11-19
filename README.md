# Codenotch for Linux

![Platform](https://img.shields.io/badge/platform-Linux%20x86__64-black?logo=linux)
![Stack](https://img.shields.io/badge/Rust%20%2B%20Tauri%202-GTK3%20%2F%20WebKit2GTK-orange?logo=rust)
![License](https://img.shields.io/badge/license-MIT-green)

**Um aplicativo nativo para Linux (X11 / XWayland) que fixa um *notch* discreto na borda da tela, mostrando em tempo real o consumo de cotas dos seus assistentes de código com IA, o estado de atividade das sessões locais (*working*, *waiting*, *done*) e modelos locais carregados no Ollama e LM Studio.**

---

## Principais Recursos

- **Notch flutuante nativo para Linux (X11 / XWayland):**
  - Pílula com cantos côncavos (*fillets*), anéis coloridos por faixa de consumo, *hover card* detalhado com janelas de reset e arraste suave entre qualquer borda do monitor (`left`, `right`, `top`, `bottom`).
  - **Click-through cirúrgico via Cairo (`gdk_window.input_shape_combine_region`):** cliques fora da pílula ou do card passam direto para a janela de trás, sem polling global de ponteiro.
  - **Detecção real de tela cheia via EWMH (`_NET_WM_STATE_FULLSCREEN`):** recolhe automaticamente o notch durante vídeos, apresentações ou jogos em tela cheia.
  - **Foco direto no terminal da sessão (`/proc` + `_NET_ACTIVE_WINDOW`):** clique na sessão ativa para trazer a janela do terminal correspondente para frente; olhar para o terminal limpa o estado *done* automaticamente (*seen-clears-it*).
- **14 Providers suportados no Linux:**
  1. **Claude Code** — Multi-conta (`~/.claude` e `~/.claude-<slug>`), renovação preventiva via `claude -p`, hooks determinísticos via `codenotch-hook` e fallback via *Transcript Watcher* (`notify` em `~/.claude/projects/**/*.jsonl`).
  2. **OpenAI Codex** — Sessão local (`~/.codex/auth.json`), fallback via `codex` app-server (`account/rateLimits/read`) e SQLite WAL (`~/.codex/state_5.sqlite` e `thread_history_1.sqlite`).
  3. **Cursor** — Leitura read-only WAL-aware de `~/.config/Cursor/User/globalStorage/state.vscdb` (cotas *included*, *API*, *on-demand* + estado real de *composer* em execução).
  4. **Google Antigravity** — Suporte completo ao CLI oficial (`~/.local/bin/agy`, `~/.gemini/bin/agy` ou `PATH` com parser interativo e TSV), bridge local HTTP/HTTPS do `language_server` (`x-codeium-csrf-token`) e contagem de passos em `~/.gemini/antigravity*/brain/*/.system_generated/logs/transcript.jsonl`.
  5. **GitHub Copilot** — `GH_TOKEN`/`GITHUB_TOKEN`, `~/.config/gh/hosts.yml` ou `gh auth token` → `/copilot_internal/user`.
  6. **Grok** — `~/.grok/auth.json` → `cli-chat-proxy.grok.com/v1/billing?format=credits`.
  7. **OpenCode / OpenCode Go** — `~/.local/share/opencode/auth.json` e `opencode.db` (`session_v2` WAL).
  8. **GLM (Z.ai / ZCode)** — Chave em Claude/OpenCode ou cofre ZCode descriptografado localmente com AES-256-GCM + SHA-256.
  9. **DeepSeek Harness** — Transcripts `.jsonl.zstd` em `~/.dsh` (`ruzstd`) + API de saldo da plataforma.
  10. **Command Code** — `~/.commandcode/auth.json` → endpoints GOAT `/alpha`.
  11. **Qoder** — Sidecar oficial `@qoder-ai/qoder-agent-sdk` (`qoder-usage.cjs`) reutilizando login `qodercli`.
  12. **Mistral AI / Vibe** — Orçamento mensal via `/v1/billing/budget`.
  13. **Ollama (Local)** — Modelos carregados em `http://127.0.0.1:11434/api/ps` (VRAM/RAM, quantização, tamanho de contexto e tempo de expiração).
  14. **LM Studio (Local)** — Instâncias LLM carregadas em `http://127.0.0.1:1234/api/v1/models` (quantização, contexto e tamanho em disco).
- **Alertas de renovação de cota & Autostart XDG:**
  - Card flutuante de renovação + notificação nativa do desktop Linux (`notify-send`).
  - Inicialização automática por usuário em `~/.config/autostart/codenotch.desktop`.

---

## Estrutura do Repositório (100% Linux-Only)

```text
codenotch-linux/
├── Cargo.toml                  # Workspace Rust (codenotch + codenotch-hook)
├── crates/
│   ├── codenotch/              # Aplicativo principal Tauri 2 + GTK3 + X11
│   │   ├── Cargo.toml
│   │   ├── tauri.conf.json
│   │   ├── qoder-usage.cjs
│   │   ├── glyphs/             # Ícones SVG monocromáticos embarcados
│   │   ├── icons/              # Ícones da aplicação
│   │   ├── ui/                 # Frontend (notch.html, settings.html, carry.html, reset-alert.html)
│   │   └── src/                # Módulos Rust (providers, X11 focus, Cairo input shape, SQLite WAL, servidor local :48666)
│   └── codenotch-hook/         # Cliente CLI zero-dependency para hooks do Claude Code
├── scripts/
│   ├── run-linux.sh            # Launcher X11/XWayland com limpeza de ambiente snap
│   ├── install-linux.sh        # Instalador user-space (~/.local/bin + ~/.local/share/applications)
│   ├── check-ui-scripts.mjs    # Verificador estático de sintaxe JS/HTML da UI
│   └── tests/                  # Suíte de testes unitários da UI (node --test)
└── .github/workflows/
    └── ci-linux.yml            # Pipeline CI/CD Ubuntu 24.04 (.deb, .AppImage, .tar.gz)
```

---

## Pré-requisitos e Compilação no Ubuntu / Debian

### 1. Instalar dependências de sistema do GTK3 / WebKit2GTK (Tauri 2)

```bash
sudo apt update
sudo apt install -y \
  pkg-config \
  build-essential \
  libssl-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
```

### 2. Rodar testes e compilar os binários

```bash
# Validar sintaxe e testes unitários da UI (Node.js)
node scripts/check-ui-scripts.mjs
for f in scripts/tests/test-*.cjs; do node --test "$f"; done

# Rodar suíte de testes Rust
cargo test --workspace

# Compilar binários otimizados de release
cargo build --release --workspace
```

### 3. Instalar no seu usuário (`~/.local/bin/codenotch` + menu de aplicativos)

```bash
./scripts/install-linux.sh
codenotch
```

Ou executar diretamente do repositório:

```bash
./scripts/run-linux.sh
```

---

## Licença

Distribuído sob a licença [MIT](LICENSE).
