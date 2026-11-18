# DeskPulse MVP — Especificação de Design

## 1. Intenção e Por Quê
Criar um painel pessoal leve e auditável (inspirado na ideia do `ersinkoc/phonebox`, porém refeito do zero em **Rust**) para acompanhar a saúde do PC, controlar a reprodução de mídia e receber/responder notificações do **Antigravity** direto no navegador do celular via cabo USB.

**Por que refazer em Rust (e não usar o PhoneBox original):**
- Remover recursos que geram atrito em ambiente corporativo (execução arbitrária de PowerShell, atalhos livres de sistema, matar processos, auto-updater não assinado, exposição de portas na rede local).
- Ter um binário único leve (~5–15 MB de RAM), 100% inspecionável, com controle remoto estritamente tipado e fechado.

---

## 2. Objetivos e Não-Objetivos (MVP v1)

### Objetivos (Entra na v1)
1. **Saúde do PC (Somente Leitura):** CPU %, RAM (usada/total), GPU (uso %, temperatura, VRAM via `nvidia-smi` quando disponível e fallback gracioso quando ausente), discos e tráfego de rede (down/up), além de uptime.
2. **Controle de Mídia do Windows:** exibir o que está reproduzindo (título, artista, app de origem, estado Playing/Paused e thumbnail quando disponível) + comandos de **Play/Pause, Avançar, Retroceder, Mutar/Desmutar e Volume**.
3. **Notificações e Aprovações do Antigravity (`~/.gemini/config/hooks.json`):**
   - **`Stop`:** recebe `terminationReason` (`NO_TOOL_CALL`, `ERROR`, `USER_CANCELED`, etc.), `executionNum` e `error`; envia notificação no celular e responde `{}` no `stdout` (permitindo encerrar normalmente).
   - **`PreToolUse` (seletivo e síncrono):** o runner de hooks do Antigravity executa o comando de forma síncrona *antes* da ferramenta rodar.
     - Ferramentas interceptadas para aprovação (`matcher`: `run_command|write_file|edit_file|call_mcp_tool`):
       - Lê `toolCall.id`, `toolCall.name` e `toolCall.args` (`CommandLine`, `Path`/trecho, `ServerName`/`ToolName`, `cwd`, `conversationId`).
       - Se houver painel web ativo conectado recentemente (últimos `8s`), envia card de aprovação formatado por tipo de ferramenta com 3 botões: **Aprovar (`{"decision": "allow"}`)**, **Negar (`{"decision": "deny", "reason": "..."}`)** e **Responder no PC (`{"decision": "ask"}`)**.
       - Se não houver painel conectado, se expirar o tempo limite (`45s` padrão) ou se a conexão cair, devolve **`{"decision": "ask"}`** imediatamente para seguir o fluxo normal do chat (respeitando inclusive o cache de *Always Allow*).
       - *Limitação real do protocolo de hooks do Antigravity:* como `PreToolUse` roda de forma síncrona bloqueando o loop antes do prompt da UI, o card no celular e o prompt no chat **não aparecem simultaneamente ao mesmo tempo** — primeiro aparece no celular (enquanto conectado) e, ao vencer o timeout ou clicar em *Responder no PC*, passa para o chat normalmente.
     - `ask_question`: envia notificação não bloqueante *"Antigravity está aguardando resposta no chat"* e devolve **`{"decision": "ask"}`** imediatamente.
     - Ferramentas de leitura/busca (`read_file`, `search_web`, `list_resources`, etc.) **não** entram no `matcher` (nunca são interceptadas).
     - Nota de contrato: no `hooks.json`, `"timeout"` é em **segundos** (usaremos `"timeout": 50` para acomodar espera de `45s` + margem local). O binário incluirá subcomandos `deskpulse install-hooks` e `deskpulse uninstall-hooks` para gerenciar `~/.gemini/config/hooks.json` com backup automático *(decisão do design)*.
4. **Interface Web embarcada (PWA leve):** servida diretamente pelo executável Rust, com atualizações em tempo real via **SSE** (`/api/stream`), **Wake Lock** para manter a tela do celular acesa na mesa, **som (Web Audio) + vibração** ao receber alertas e contagem regressiva nas aprovações.
5. **Transporte USB-only:** escuta apenas em `127.0.0.1:8765`, com gerenciador automático de `adb reverse tcp:8765 tcp:8765` usando `adb.exe` local.

### Não-Objetivos / Anti-Objetivos (Fica fora da v1)
- ❌ Sem porta aberta na rede local / Wi-Fi / Tailscale (0.0.0.0) na v1.
- ❌ Sem execução livre de comandos/shell, sem atalhos para abrir programas, sem matar processos, sem digitação remota/SendKeys.
- ❌ Sem auto-updater automático em segundo plano.
- ❌ Sem integrações de terceiros (Discord, OBS, Home Assistant, Docker, Spotify OAuth, RSS, câmeras RTSP).
- ❌ Sem app Android nativo na v1 (a API HTTP/SSE já nasce compatível para um futuro cliente Flutter na v2).

---

## 3. Restrições e Ambiente
- **Sistema:** Windows 10/11 x64 (`x86_64-pc-windows-msvc`).
- **Pré-requisitos a instalar:** `rustup` + Visual Studio C++ Build Tools (MSVC linker/Windows SDK para a crate `windows`).
- **Ferramentas já presentes:** `adb.exe` (`D:\Android\Sdk\platform-tools\adb.exe`), `py.exe` (Python 3.13.7).
- **Local do projeto:** `D:\Projetos\deskpulse`.

---

## 4. Arquitetura Proposta

```text
+-------------------------------------------------------------------+
| Windows PC                                                        |
|                                                                   |
|  [Antigravity] --(hooks.json)--> [deskpulse hook (stdin/stdout)]  |
|                                          | POST /api/hooks/...    |
|                                          v                        |
|  [Hardware / WinRT / CoreAudio] ---> [deskpulse.exe (Rust/Axum)]  |
|                                       127.0.0.1:8765 (Loopback)   |
|                                          ^                        |
|                                          | adb reverse tcp:8765   |
+------------------------------------------|------------------------+
                                           | Cabo USB
+------------------------------------------v------------------------+
| Celular Android (Navegador / PWA)                                 |
|  http://127.0.0.1:8765/#token=...                                 |
|  - SSE /api/stream (PC + Mídia + Eventos/Aprovações)              |
|  - Controles tipados (POST /api/media/*, POST /api/approvals/*)   |
+-------------------------------------------------------------------+
```

### 4.1 Agente Rust (`deskpulse`)
1. **Binário único com subcomandos:**
   - `deskpulse serve` (padrão): inicia o servidor HTTP/SSE em `127.0.0.1:8765`, monitores e loop `adb reverse`.
   - `deskpulse hook pre-tool` / `deskpulse hook stop`: lê o JSON do `stdin` emitido pelo Antigravity (`toolCall.id`, `toolCall.name`, `toolCall.args`, `cwd`, `conversationId` no `PreToolUse`; `terminationReason`, `error`, `fullyIdle` no `Stop`), envia via HTTP loopback para `127.0.0.1:8765` com o token local, e imprime o JSON de decisão (`{"decision": "..."}` ou `{}`) no `stdout`. Se o agente não estiver rodando ou falhar a conexão, devolve `{"decision": "ask"}` (ou `{}` no `Stop`) sem nunca travar o Antigravity.
   - `deskpulse install-hooks` / `deskpulse uninstall-hooks`: adiciona/remove com segurança apenas as entradas do DeskPulse em `C:\Users\iagofelipe\.gemini\config\hooks.json` (preservando demais hooks e criando backup `.bak`).
2. **Coleta de Métricas do PC (`monitors/system.rs`):**
   - Loop a cada `1s` usando `sysinfo` (CPU %, RAM, discos a cada 15s, bytes de rede down/up, uptime).
   - GPU via `nvidia-smi` (quando NVIDIA presente) com fallback gracioso (`null` se ausente).
3. **Controle e Leitura de Mídia (`monitors/media.rs`):**
   - **WinRT GSMTC** (`Windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager`): lê sessão ativa (título, artista, status, app, thumbnail) e executa `TogglePlayPause`, `SkipNext`, `SkipPrevious`.
   - **Core Audio** (`IAudioEndpointVolume`): lê nível de volume (0–100) e estado `muted`, e aplica `mute`, `unmute`, `toggle-mute`, `set-volume`.
4. **Motor de Eventos e Aprovações (`approvals.rs`, `events.rs`):**
   - Rastreia `last_client_seen_ms` quando há cliente SSE/Web ativo conectado nos últimos `8s`.
   - Ao receber `POST /api/hooks/pre-tool` (autenticado com `X-DeskPulse-Token`):
     - Se `tool == "ask_question"`: emite evento `notify` ("Antigravity aguarda resposta no chat") e responde imediatamente `{"decision": "ask"}`.
     - Se `tool` for `run_command`, `write_file`, `edit_file` ou `call_mcp_tool`: se não houver cliente ativo, responde imediatamente `{"decision": "ask"}`; se houver, extrai um resumo seguro da ação (comando, caminho do arquivo/trecho, ou servidor/ferramenta MCP), enfileira a aprovação com ID aleatório, emite evento `approval` no SSE e aguarda até `45s` (`tokio::select!` com timeout e cancelamento caso o cliente HTTP da hook desconecte, ex. se o usuário cancelar o turno no chat). Aceita `allow`, `deny` ou `ask` (botão *Responder no PC*); timeout ou desconexão devolve `{"decision": "ask"}`.
   - Ao receber `POST /api/hooks/stop`: emite notificação com resumo (`NO_TOOL_CALL`, `ERROR`, etc.) e responde `{}`.
5. **Transporte USB (`adb_bridge.rs`):**
   - A cada `4s`, localiza `adb.exe` (`PATH` ou `D:\Android\Sdk\platform-tools\adb.exe` / `%LOCALAPPDATA%\Android\Sdk\platform-tools\adb.exe`) e verifica `adb devices`.
   - Para dispositivo conectado em estado `device`, garante `adb -s <serial> reverse tcp:8765 tcp:8765` e expõe no terminal/UI a URL pronta com `#token=...`.
6. **Segurança:**
   - Bind estrito em `127.0.0.1:8765` (nunca `0.0.0.0`).
   - Token aleatório de 256 bits persistido em `%APPDATA%\DeskPulse\config.json` (criado na primeira execução).
   - Middleware de segurança:
     - Valida `Host` (`127.0.0.1:8765` ou `localhost:8765`) contra DNS rebinding.
     - Valida `Sec-Fetch-Site` (`same-origin` ou `none`) e `Origin` contra CSRF em navegadores.
     - Exige `Authorization: Bearer <token>` ou `X-DeskPulse-Token` em todos os endpoints `/api/*` (nunca em query string). A URL raiz `/` entrega apenas a casca HTML estática sem segredos, e o cliente lê o token do fragmento `#token=` ou de campo manual em `localStorage`.

---

## 5. Decisões Deixadas ao Construtor (Builder)
- Estrutura exata de módulos/crates dentro do workspace Cargo e escolha de bibliotecas auxiliares (`sysinfo`, `axum`, `tokio`, `tokio-stream`, `serde`, `windows`, `rand`).
- Layout visual CSS/HTML exato dos cartões do painel web (tema escuro responsivo mobile-first).
- Detalhes de timeout do `hooks.json` (`"timeout": 50` para acomodar espera de `45s` + margem local).
- Como o usuário inicializa o token no celular (suporte a `#token=` na URL + campo manual caso abra `http://127.0.0.1:8765` direto).
