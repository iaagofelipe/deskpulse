# DeskPulse v2

**Hub de mesa OLED em Rust** projetado para transformar um smartphone Android conectado via **USB (`adb reverse`)** na horizontal em um *appliance* dedicado de telemetria, mídia, agenda, agentes de IA e atalhos rápidos — sem depender de nuvem ou Wi-Fi externo.

## Recursos (Appliance de 9 Abas)

- **Conexão local segura via USB (`adb reverse`)**: escuta exclusivamente em `127.0.0.1:8765`, cria o túnel reverso via ADB automaticamente quando o celular é plugado e autentica todas as chamadas HTTP/SSE via token salvo em `%APPDATA%\DeskPulse\config.json`.
- **Bandeja do Sistema (Windows System Tray) + Auto-Start**: roda discretamente na bandeja do Windows com menu nativo em PT-BR (*Abrir no Celular (ADB)*, *Abrir no Navegador*, *Editar Config*, *Iniciar com o Windows* e *Sair* com encerramento gracioso), além de *hot-reload* automático do `config.json`.
- **Modo Noturno e Proteção OLED**: escurecimento automático em horário configurado e quando o PC é bloqueado (`dim_when_pc_locked`), com protetor de tela de relógio anti-burn-in.

### As 9 Telas do Painel

1. **Início**: grade Bento configurável (`home_widgets`) com relógio, status de conexão e cartões de resumo (IA, Clima, Agenda, Mídia e GitHub).
2. **PC e Internet**: CPU, RAM, discos, GPU NVIDIA (`nvidia-smi`), histórico de 60 segundos, download/upload em tempo real, consumo de dados diário/mensal e latência (ping).
3. **IA e Agentes**: consumo agregado de tokens (janelas de 5h e semanal com orçamentos opcionais em `config.json`) e monitor *Master–Detail* de sessões Antigravity (`~/.gemini/antigravity`), checklist (`task.md`), eventos e aprovação remota via hooks `PreToolUse`/`Stop`.
4. **Agenda e Tarefas**: eventos de Hoje e dos próximos 7 dias via feed `.ics` (Google Agenda / iCalendar) e gaveta lateral de To-Dos sincronizados com a CLI.
5. **Clima**: condições atuais, sensação, umidade, vento e previsão via Open-Meteo (sem necessidade de chave de API).
6. **Mídia**: player com capa, linha do tempo com *seek* interativo, controles WinRT (play/pause, faixa anterior/próxima) e volume/mudo de sistema e microfone via WASAPI.
7. **GitHub**: contribuições, sequência (*streak*), PRs e atividade via GitHub GraphQL API (lê `github.token` ou variável de ambiente como `GITHUB_TOKEN`).
8. **Atalhos**: grade estilo Stream Deck configurável no `config.json` (mudo de mic/som, volume, mídia, captura `Win+Shift+S`, abrir apps, URLs, pastas e travar PC).
9. **Timer**: Pomodoro e temporizador local com badge de contagem ativa na barra lateral.

## Requisitos

- **Windows 10/11** (mídia WinRT/WASAPI, detecção de bloqueio de sessão, bandeja do sistema e auto-start no Registro)
- [Rust toolchain](https://rustup.rs/)
- `adb` (Android Platform Tools) no `PATH` ou no Android SDK padrão e **Depuração USB** ativa no celular

## Build e testes

```powershell
cargo check
cargo test
cargo build --release
```

## Uso rápido e CLI

```powershell
# Inicia o servidor local, túnel ADB reverse e ícone na bandeja do Windows
deskpulse serve

# Inicia em modo headless (sem ícone na bandeja do sistema)
deskpulse serve --no-tray

# Abre o DeskPulse automaticamente no navegador do celular via ADB (autenticado com #token)
deskpulse open

# Exibe o caminho completo do config.json ativo
deskpulse config-path

# Gerencia a inicialização automática com o Windows (HKCU\...\Run)
deskpulse startup install
deskpulse startup status
deskpulse startup uninstall

# Gerencia To-Dos pelo terminal
deskpulse todo list
deskpulse todo add "Revisar PR"
deskpulse todo done 1
deskpulse todo rm 1

# Instala ou remove hooks do Antigravity (~/.gemini/config/hooks.json)
deskpulse install-hooks
deskpulse uninstall-hooks
```

## Configuração (`config.json` v2)

O arquivo é criado automaticamente na primeira execução (use `deskpulse config-path` para localizar, normalmente `%APPDATA%\DeskPulse\config.json`). Alterações salvas em disco são recarregadas automaticamente (*hot-reload*) sem precisar reiniciar o servidor.

```json
{
  "port": 8765,
  "token": "gerado-automaticamente-64-hex",
  "approval_timeout_secs": 45,
  "calendar_ics_url": "https://calendar.google.com/calendar/ical/.../basic.ics",
  "remote_approval_enabled": false,
  "weather": {
    "city": "São Paulo",
    "latitude": -23.5505,
    "longitude": -46.6333,
    "unit": "c"
  },
  "github": {
    "username": "seu-usuario",
    "token": null,
    "token_env": "GITHUB_TOKEN"
  },
  "ai": {
    "provider": "antigravity",
    "budget_5h_tokens": 500000,
    "budget_weekly_tokens": 5000000
  },
  "home_widgets": ["ai", "weather", "agenda", "media", "github"],
  "night_mode": {
    "enabled": true,
    "start_hour": 23,
    "end_hour": 7,
    "dim_when_pc_locked": true
  },
  "shortcuts": [
    {
      "id": "mute_mic",
      "label": "Mutar Mic",
      "icon": "mic",
      "color": "#ef4444",
      "action": { "type": "toggle_mic" }
    },
    {
      "id": "screenshot",
      "label": "Captura",
      "icon": "camera",
      "color": "#8b5cf6",
      "action": { "type": "hotkey", "keys": "Win+Shift+S" }
    },
    {
      "id": "terminal",
      "label": "Terminal",
      "icon": "terminal",
      "color": "#10b981",
      "action": { "type": "app", "program": "wt.exe", "args": [] }
    },
    {
      "id": "docs",
      "label": "Docs",
      "icon": "globe",
      "color": "#06b6d4",
      "action": { "type": "url", "url": "https://github.com" }
    },
    {
      "id": "downloads",
      "label": "Downloads",
      "icon": "folder",
      "color": "#06b6d4",
      "action": { "type": "folder", "path": "%USERPROFILE%\\Downloads" }
    },
    {
      "id": "lock_pc",
      "label": "Bloquear PC",
      "icon": "lock",
      "color": "#64748b",
      "action": { "type": "lock_pc" }
    }
  ]
}
```

> **Compatibilidade:** arquivos `config.json` da v1 continuam funcionando sem migração manual — os novos campos recebem padrões seguros automaticamente (incluindo 8 atalhos padrão e os 5 widgets da Home).

## Bandeja do Sistema (Windows)

Ao rodar `deskpulse serve`, um ícone do DeskPulse aparece na área de notificação do Windows:

- **Duplo clique**: abre o painel no navegador padrão (`http://127.0.0.1:<porta>/#token=<token>`).
- **Clique direito**:
  - **Abrir no Celular (ADB)** — executa `deskpulse open` sem janela de console
  - **Abrir no Navegador** — abre o painel autenticado no PC
  - **Editar Config (config.json)** — abre o `config.json` no Bloco de Notas
  - **Iniciar com o Windows** — marca/desmarca o auto-start no Registro do usuário
  - **Sair** — encerra o servidor HTTP, os monitores em segundo plano e remove o ícone da bandeja

## Estrutura do projeto

- `src/main.rs` — CLI (`serve`, `open`, `config-path`, `startup`, `todo`, `hook`, `install-hooks`, `uninstall-hooks`), ciclo de vida e *graceful shutdown*
- `src/config.rs` — esquema `config.json` v2, *hot-reload* (`ConfigWatcher`) e helpers de auto-start no Windows
- `src/tray.rs` — bandeja nativa do Windows (`Shell_NotifyIconW`, `TrackPopupMenu`, `TrayHandle` RAII) com stub no-op fora do Windows
- `src/server.rs` & `src/security.rs` — API HTTP/SSE (`axum`) protegida por `X-DeskPulse-Token` / `Bearer`
- `src/monitors/` — coletores de sistema/GPU (`system.rs`), rede (`network.rs`), mídia (`media.rs`), ADB (`adb.rs`), IA/Antigravity (`ai.rs`, `antigravity.rs`), clima (`weather.rs`) e GitHub (`github.rs`)
- `src/shortcuts.rs` — executor seguro de atalhos estilo Stream Deck
- `src/agenda.rs` & `src/todos.rs` — sincronização `.ics` (7 dias) e persistência de tarefas
- `src/hooks.rs`, `src/approvals.rs` & `src/events.rs` — hooks `PreToolUse`/`Stop` e barramento SSE
- `web/` — SPA OLED de 9 abas embutida no binário (`index.html`, CSS modular, ES modules, PWA manifest e service worker)

## Licença

Este projeto está sob a licença [MIT](LICENSE).
