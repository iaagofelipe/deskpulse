# DeskPulse

Dashboard desktop em **Rust** projetado para rodar em um smartphone Android conectado via **USB (ADB reverse)** na horizontal — transformando o celular em uma segunda tela de comando e telemetria sem depender de Wi-Fi externo.

## Recursos

- **Conexão local via USB (`adb reverse`)**: o servidor sobe apenas em `127.0.0.1:8765`, faz o túnel automático via ADB quando o aparelho está plugado e protege a API com token gerado em disco (`%APPDATA%\DeskPulse\config.json` no Windows).
- **Visão Geral (Landscape)**: relógio, status USB/ADB, **Agenda Hoje** (ou mini-calendário mensal quando vazia), **To-Do**, controles de mídia/volume e resumo de CPU, RAM, disco e rede — com blocos recolhíveis ao toque e minimização automática de listas vazias.
- **Agentes IA (Antigravity)**: painel *Master–Detail* com sessões recentes (`~/.gemini/antigravity`), etapa e ferramenta atual, último prompt, ações recentes, checklist (`task.md`), log de eventos e suporte opcional a **Aprovação Remota** via hooks `PreToolUse`/`Stop`.
- **Agenda & To-Do**: sincronização periódica com feed `.ics` (iCalendar / Google Agenda), lista de tarefas persistida e timer Pomodoro local.
- **Monitor PC**: histórico de CPU, uso de RAM, discos locais, GPU NVIDIA (quando `nvidia-smi` está disponível), mídia via WinRT/WASAPI e ações rápidas (Mudo e Travar tela).

## Requisitos

- Windows (para controles de mídia via WinRT/WASAPI e bloqueio de sessão)
- [Rust toolchain](https://rustup.rs/)
- `adb` (Android Platform Tools) no `PATH` e Depuração USB ativa no aparelho

## Build e testes

```powershell
cargo test
cargo build --release
```

## Uso rápido

```powershell
# Inicia o servidor local e o túnel ADB reverse (porta 8765)
cargo run --release -- serve

# Abre o DeskPulse automaticamente no navegador do celular conectado (já autenticado via #token)
cargo run --release -- open
```

### Outros comandos da CLI

```powershell
# Gerenciar To-Dos pelo terminal
deskpulse todo list
deskpulse todo add "Revisar PR"
deskpulse todo done 1
deskpulse todo rm 1

# Instalar ou remover hooks do Antigravity (~/.gemini/config/hooks.json)
deskpulse install-hooks
deskpulse uninstall-hooks
```

## Estrutura do projeto

- `src/main.rs` — CLI (`serve`, `open`, `todo`, `hook`, `install-hooks`, `uninstall-hooks`) e ciclo de vida do daemon
- `src/server.rs` & `src/security.rs` — API HTTP/SSE (`axum`) com autenticação por header `X-DeskPulse-Token` / `Bearer`
- `src/monitors/` — coletores de sistema/GPU (`system.rs`), mídia (`media.rs`), ADB (`adb.rs`) e sessões Antigravity (`antigravity.rs`)
- `src/agenda.rs` & `src/todos.rs` — sincronização `.ics` e persistência de tarefas
- `src/hooks.rs`, `src/approvals.rs` & `src/events.rs` — hooks `PreToolUse`/`Stop` e barramento SSE
- `web/` — SPA HTML/CSS/JS embutida no binário (`index.html`, `manifest.webmanifest`, `sw.js`, ícone PWA)

## Licença

Este projeto está sob a licença [MIT](LICENSE).
