import * as api from './api.js';
import * as icons from './icons.js';

const THEMES = [
  { id: 'midnight', label: 'Midnight', sub: 'OLED padrão', preview: ['#090b0e', '#111619', '#60a5fa', '#4fd1c5', '#e07a52'] },
  { id: 'pure-black', label: 'Pure Black', sub: 'AMOLED #000', preview: ['#000000', '#0a0d12', '#38bdf8', '#2dd4bf', '#fb923c'] },
  { id: 'amber', label: 'Amber', sub: 'Cockpit noturno', preview: ['#0c0805', '#1a120b', '#f59e0b', '#fb923c', '#84cc16'] },
  { id: 'nord', label: 'Nord Frost', sub: 'Ártico suave', preview: ['#1b212c', '#252d3b', '#88c0d0', '#a3be8c', '#b48ead'] },
  { id: 'daylight', label: 'Daylight', sub: 'Claro limpo', preview: ['#eef2f7', '#ffffff', '#2563eb', '#0d9488', '#c2410c'] },
];

const WIDGET_META = [
  { key: 'ai', name: 'IA & Agentes', desc: 'Tokens 5h/semanal e sessões ativas' },
  { key: 'weather', name: 'Clima', desc: 'Temperatura atual e máxima/mínima' },
  { key: 'agenda', name: 'A seguir (Agenda)', desc: 'Próximo compromisso do dia' },
  { key: 'media', name: 'Mídia', desc: 'Player atual com play/pause rápido' },
  { key: 'github', name: 'GitHub Heatmap', desc: 'Mapa de contribuições (ou PC se sem token)' },
  { key: 'pc', name: 'Telemetria do PC', desc: 'Faixa CPU, GPU, RAM e Rede' },
];

const SHORTCUT_ICONS = [
  'mic', 'mic-off', 'volume-x', 'volume-1', 'volume-2',
  'play', 'next', 'prev', 'screenshot', 'terminal',
  'folder', 'lock', 'globe', 'code', 'zap', 'sliders',
];

const ACTION_TYPES = [
  { type: 'toggle_mic', label: 'Mutar / Desmutar Mic' },
  { type: 'toggle_mute', label: 'Mutar / Desmutar Som' },
  { type: 'vol_up', label: 'Volume +' },
  { type: 'vol_down', label: 'Volume -' },
  { type: 'media_play_pause', label: 'Play / Pause Mídia' },
  { type: 'media_next', label: 'Próxima faixa' },
  { type: 'media_prev', label: 'Faixa anterior' },
  { type: 'lock_pc', label: 'Bloquear PC (Win+L)' },
  { type: 'hotkey', label: 'Atalho de Teclado (Hotkey)' },
  { type: 'app', label: 'Abrir Aplicativo' },
  { type: 'url', label: 'Abrir URL no Navegador' },
  { type: 'folder', label: 'Abrir Pasta' },
];

const SECTION_META = {
  general: ['Geral & Aparência', 'Tema, comportamento USB, tela inicial e modo descanso OLED.'],
  weather: ['Clima', 'Configure cidade, coordenadas opcionais e unidade de temperatura.'],
  github: ['GitHub', 'Configure seu usuário e Personal Access Token (GraphQL).'],
  agenda: ['Agenda', 'Gerencie o feed iCal (.ics) e acompanhe os eventos sincronizados.'],
  ai: ['IA & Orçamentos', 'Defina limites opcionais de tokens para a janela de 5h e semanal.'],
  shortcuts: ['Atalhos (Stream Deck)', 'Crie, edite, reordene e teste até 16 botões interativos.'],
  raw: ['JSON Bruto', 'Edite o config.json completo diretamente com validação segura.'],
};

let configData = null;
let liveState = null;
let draft = null;
let githubTokenClear = false;
let dirty = false;
let streamHandle = null;

const $ = (id) => document.getElementById(id);

function showToast(msg, kind = 'ok') {
  const stack = $('toastStack');
  if (!stack) return;
  const el = document.createElement('div');
  el.className = `toast ${kind}`;
  el.textContent = msg;
  stack.appendChild(el);
  setTimeout(() => {
    el.remove();
  }, 3600);
}

function setDirty(flag) {
  dirty = Boolean(flag);
  const pill = $('dirtyPill');
  if (pill) pill.classList.toggle('hidden', !dirty);
}

function applyThemePreview(themeId) {
  const valid = THEMES.some((t) => t.id === themeId) ? themeId : 'midnight';
  document.documentElement.dataset.theme = valid;
  const theme = THEMES.find((t) => t.id === valid);
  const meta = document.querySelector('meta[name="theme-color"]');
  if (theme && meta) meta.setAttribute('content', theme.preview[0]);
}

function hydrateIcons() {
  document.querySelectorAll('[data-icon]').forEach((el) => {
    const name = el.dataset.icon;
    const size = el.classList.contains('btn-ico') ? 15 : 17;
    el.innerHTML = icons.icon(name, size);
  });
}

function setSection(sectionId) {
  document.querySelectorAll('.admin-nav-btn[data-section]').forEach((btn) => {
    const active = btn.dataset.section === sectionId;
    btn.classList.toggle('active', active);
    btn.setAttribute('aria-selected', active ? 'true' : 'false');
  });
  document.querySelectorAll('.panel[data-panel]').forEach((panel) => {
    panel.classList.toggle('active', panel.dataset.panel === sectionId);
  });
  const [title, sub] = SECTION_META[sectionId] || SECTION_META.general;
  if ($('sectionTitle')) $('sectionTitle').textContent = title;
  if ($('sectionSubtitle')) $('sectionSubtitle').textContent = sub;
}

function renderThemeGrid() {
  const grid = $('adminThemeGrid');
  if (!grid || !draft) return;
  grid.innerHTML = THEMES.map((t) => {
    const active = draft.theme === t.id;
    const [bg, surf, c1, c2, c3] = t.preview;
    return `
      <button type="button" class="theme-card ${active ? 'is-active' : ''}" data-theme-pick="${t.id}">
        <div class="theme-swatch" style="background:${bg}">
          <div class="theme-swatch-dots">
            <span style="background:${c1}"></span>
            <span style="background:${c2}"></span>
            <span style="background:${c3}"></span>
          </div>
          <div class="theme-swatch-card" style="background:${surf}">
            <span style="width:45%;height:4px;border-radius:99px;background:${c1}"></span>
            <span style="width:28%;height:4px;border-radius:99px;background:${c2}"></span>
          </div>
        </div>
        <div class="theme-card-name">${api.escapeHtml(t.label)}</div>
        <div class="theme-card-sub">${api.escapeHtml(t.sub)}</div>
      </button>`;
  }).join('');
}

function renderHomeWidgetsList() {
  const container = $('homeWidgetsList');
  if (!container || !draft) return;
  const activeList = Array.isArray(draft.home_widgets) ? draft.home_widgets : [];
  const ordered = [
    ...activeList.filter((k) => WIDGET_META.some((m) => m.key === k)),
    ...WIDGET_META.map((m) => m.key).filter((k) => !activeList.includes(k)),
  ];

  container.innerHTML = ordered.map((key) => {
    const meta = WIDGET_META.find((m) => m.key === key);
    const enabled = activeList.includes(key);
    const idx = activeList.indexOf(key);
    return `
      <div class="widget-item" data-widget-key="${key}">
        <div class="widget-item-left">
          <input type="checkbox" class="switch" data-widget-toggle="${key}" ${enabled ? 'checked' : ''}>
          <div>
            <div class="widget-item-name">${api.escapeHtml(meta.name)}</div>
            <div class="widget-item-desc">${api.escapeHtml(meta.desc)}</div>
          </div>
        </div>
        ${enabled ? `
          <div class="inline-actions">
            <button type="button" class="btn btn-secondary btn-sm" data-widget-move="${key}" data-dir="-1" ${idx <= 0 ? 'disabled' : ''} title="Subir">↑</button>
            <button type="button" class="btn btn-secondary btn-sm" data-widget-move="${key}" data-dir="1" ${idx >= activeList.length - 1 ? 'disabled' : ''} title="Descer">↓</button>
          </div>` : ''}
      </div>`;
  }).join('');
}

function defaultActionForType(type) {
  switch (type) {
    case 'hotkey': return { type: 'hotkey', keys: 'Ctrl+Shift+M' };
    case 'app': return { type: 'app', program: 'notepad.exe', args: [] };
    case 'url': return { type: 'url', url: 'https://github.com' };
    case 'folder': return { type: 'folder', path: '%USERPROFILE%\\Downloads' };
    default: return { type };
  }
}

function renderActionFields(sc, idx) {
  const action = sc.action || { type: 'toggle_mic' };
  const t = action.type || 'toggle_mic';
  if (t === 'hotkey') {
    return `
      <label class="field">
        <span>Combinação de teclas</span>
        <input type="text" data-sc-idx="${idx}" data-sc-action-field="keys" value="${api.escapeHtml(action.keys || '')}" placeholder="Ex: Win+Shift+S ou Ctrl+Shift+M">
      </label>`;
  }
  if (t === 'app') {
    const argsStr = Array.isArray(action.args) ? action.args.join(' ') : '';
    return `
      <div class="field-grid-2">
        <label class="field">
          <span>Executável</span>
          <input type="text" data-sc-idx="${idx}" data-sc-action-field="program" value="${api.escapeHtml(action.program || '')}" placeholder="Ex: wt.exe ou code">
        </label>
        <label class="field">
          <span>Argumentos (separados por espaço)</span>
          <input type="text" data-sc-idx="${idx}" data-sc-action-field="args" value="${api.escapeHtml(argsStr)}" placeholder="Opcional">
        </label>
      </div>`;
  }
  if (t === 'url') {
    return `
      <label class="field">
        <span>URL (http:// ou https://)</span>
        <input type="url" data-sc-idx="${idx}" data-sc-action-field="url" value="${api.escapeHtml(action.url || '')}" placeholder="https://...">
      </label>`;
  }
  if (t === 'folder') {
    return `
      <label class="field">
        <span>Caminho da pasta</span>
        <input type="text" data-sc-idx="${idx}" data-sc-action-field="path" value="${api.escapeHtml(action.path || '')}" placeholder="%USERPROFILE%\\Downloads">
      </label>`;
  }
  return '';
}

function renderShortcutsEditor() {
  const el = $('shortcutsEditor');
  if (!el || !draft) return;
  const list = Array.isArray(draft.shortcuts) ? draft.shortcuts : [];
  if ($('addShortcutBtn')) {
    $('addShortcutBtn').disabled = list.length >= 16;
  }

  if (!list.length) {
    el.innerHTML = '<div class="live-box">Nenhum atalho configurado. Clique em <strong>Novo atalho</strong> para criar o primeiro botão.</div>';
    return;
  }

  el.innerHTML = list.map((sc, idx) => {
    const color = /^#[0-9a-f]{6}$/i.test(sc.color || '') ? sc.color : '#60a5fa';
    const actionType = sc.action?.type || 'toggle_mic';
    return `
      <div class="shortcut-edit-card" data-sc-card="${idx}">
        <div class="shortcut-edit-head">
          <div class="shortcut-preview-pill">
            <span class="shortcut-color-dot" style="background:${color}26;color:${color}">${icons.icon(sc.icon || 'zap', 15)}</span>
            <span>#${idx + 1} · ${api.escapeHtml(sc.label || 'Atalho')}</span>
          </div>
          <div class="inline-actions">
            <button type="button" class="btn btn-secondary btn-sm" data-sc-test="${api.escapeHtml(sc.id)}" title="Testar no PC">Testar</button>
            <button type="button" class="btn btn-secondary btn-sm" data-sc-move="${idx}" data-dir="-1" ${idx === 0 ? 'disabled' : ''}>↑</button>
            <button type="button" class="btn btn-secondary btn-sm" data-sc-move="${idx}" data-dir="1" ${idx === list.length - 1 ? 'disabled' : ''}>↓</button>
            <button type="button" class="btn btn-danger-soft btn-sm" data-sc-del="${idx}" title="Remover">✕</button>
          </div>
        </div>

        <div class="shortcut-grid-3">
          <label class="field">
            <span>Rótulo</span>
            <input type="text" data-sc-idx="${idx}" data-sc-field="label" value="${api.escapeHtml(sc.label || '')}" placeholder="Nome do botão">
          </label>
          <label class="field">
            <span>Ícone</span>
            <select data-sc-idx="${idx}" data-sc-field="icon">
              ${SHORTCUT_ICONS.map((ic) => `<option value="${ic}" ${sc.icon === ic ? 'selected' : ''}>${ic}</option>`).join('')}
            </select>
          </label>
          <label class="field">
            <span>Cor</span>
            <input type="color" data-sc-idx="${idx}" data-sc-field="color" value="${color}">
          </label>
        </div>

        <label class="field">
          <span>Ação</span>
          <select data-sc-idx="${idx}" data-sc-action-type="1">
            ${ACTION_TYPES.map((a) => `<option value="${a.type}" ${actionType === a.type ? 'selected' : ''}>${api.escapeHtml(a.label)}</option>`).join('')}
          </select>
        </label>

        ${renderActionFields(sc, idx)}
      </div>`;
  }).join('');
}

function hydrateFormFromConfig(cfg) {
  configData = cfg;
  githubTokenClear = false;
  draft = {
    theme: cfg.theme || 'midnight',
    auto_open_on_usb: Boolean(cfg.auto_open_on_usb),
    remote_approval_enabled: Boolean(cfg.remote_approval_enabled),
    approval_timeout_secs: Number(cfg.approval_timeout_secs || 45),
    calendar_ics_url: cfg.calendar_ics_url || '',
    home_widgets: Array.isArray(cfg.home_widgets) ? cfg.home_widgets.slice() : ['ai', 'weather', 'agenda', 'media', 'github'],
    night_mode: {
      enabled: Boolean(cfg.night_mode?.enabled),
      start_hour: Number(cfg.night_mode?.start_hour ?? 23),
      end_hour: Number(cfg.night_mode?.end_hour ?? 7),
      dim_when_pc_locked: Boolean(cfg.night_mode?.dim_when_pc_locked),
    },
    weather: {
      city: cfg.weather?.city || '',
      latitude: cfg.weather?.latitude ?? '',
      longitude: cfg.weather?.longitude ?? '',
      unit: cfg.weather?.unit || 'c',
    },
    github: {
      username: cfg.github?.username || '',
      token_env: cfg.github?.token_env || 'GITHUB_TOKEN',
    },
    ai: {
      provider: cfg.ai?.provider || 'antigravity',
      budget_5h_tokens: cfg.ai?.budget_5h_tokens ?? '',
      budget_weekly_tokens: cfg.ai?.budget_weekly_tokens ?? '',
    },
    shortcuts: Array.isArray(cfg.shortcuts) ? JSON.parse(JSON.stringify(cfg.shortcuts)) : [],
  };

  applyThemePreview(draft.theme);
  renderThemeGrid();
  renderHomeWidgetsList();
  renderShortcutsEditor();

  $('autoOpenUsbInput').checked = draft.auto_open_on_usb;
  $('startupEnabledInput').checked = Boolean(cfg.startup_enabled);
  $('remoteApprovalInput').checked = draft.remote_approval_enabled;
  $('approvalTimeoutInput').value = draft.approval_timeout_secs;
  $('nightEnabledInput').checked = draft.night_mode.enabled;
  $('nightDimLockedInput').checked = draft.night_mode.dim_when_pc_locked;
  $('nightStartInput').value = draft.night_mode.start_hour;
  $('nightEndInput').value = draft.night_mode.end_hour;

  $('weatherCityInput').value = draft.weather.city;
  $('weatherLatInput').value = draft.weather.latitude;
  $('weatherLonInput').value = draft.weather.longitude;
  $('weatherUnitInput').value = draft.weather.unit;

  $('githubUserInput').value = draft.github.username;
  $('githubTokenInput').value = '';
  $('githubTokenEnvInput').value = draft.github.token_env;

  const tokBar = $('githubTokenStatus');
  if (tokBar) {
    if (cfg.github?.token_set) {
      tokBar.innerHTML = `Token salvo no <code>config.json</code>: <strong>${api.escapeHtml(cfg.github.token_preview || 'Configurado')}</strong>`;
    } else {
      tokBar.innerHTML = `Nenhum token inline salvo (usará variável <code>${api.escapeHtml(draft.github.token_env)}</code> se definida).`;
    }
  }

  $('agendaIcsInput').value = draft.calendar_ics_url;
  $('aiProviderInput').value = draft.ai.provider;
  $('aiBudget5hInput').value = draft.ai.budget_5h_tokens;
  $('aiBudgetWeeklyInput').value = draft.ai.budget_weekly_tokens;

  if ($('configPathText')) {
    $('configPathText').textContent = cfg.config_path || '';
    $('configPathText').title = cfg.config_path || '';
  }
  if ($('rawJsonArea') && typeof cfg.raw_json === 'string') {
    $('rawJsonArea').value = cfg.raw_json;
  }
  if ($('rawJsonError')) {
    $('rawJsonError').classList.add('hidden');
  }

  const tok = api.loadToken();
  if ($('previewLink') && tok) {
    $('previewLink').href = `/#token=${encodeURIComponent(tok)}`;
  }

  setDirty(false);
}

function renderLivePanels() {
  if (!liveState) return;

  // Sidebar status
  const adb = liveState.adb || {};
  const usbDot = $('usbDot');
  const usbText = $('usbStatusText');
  if (usbDot && usbText) {
    if (adb.reverse_active && Array.isArray(adb.devices) && adb.devices.length) {
      usbDot.className = 'status-dot ok';
      usbText.textContent = `USB ativo (${adb.devices[0]})`;
    } else if (Array.isArray(adb.devices) && adb.devices.length) {
      usbDot.className = 'status-dot warn';
      usbText.textContent = `USB detectado (${adb.devices[0]})`;
    } else {
      usbDot.className = 'status-dot';
      usbText.textContent = 'Nenhum celular USB';
    }
  }

  // Weather live box
  const w = liveState.weather || {};
  const wBox = $('weatherLiveBox');
  if (wBox) {
    if (w.error) {
      wBox.innerHTML = `<div class="raw-error">${api.escapeHtml(w.error)}</div>`;
    } else if (w.configured && w.temp != null) {
      const u = (w.unit || 'c').toLowerCase().startsWith('f') ? '°F' : '°C';
      wBox.innerHTML = `
        <div class="live-kpi">${Math.round(w.temp)}${u} · ${api.escapeHtml(w.city || '')}</div>
        <div class="live-row"><span>Condição</span><strong>${api.escapeHtml(w.condition || '—')}</strong></div>
        <div class="live-row"><span>Sensação</span><strong>${w.feels_like != null ? `${Math.round(w.feels_like)}${u}` : '—'}</strong></div>
        <div class="live-row"><span>Máx / Mín</span><strong>${w.today_high != null ? `${Math.round(w.today_high)}° / ${Math.round(w.today_low)}°` : '—'}</strong></div>
        <div class="live-row"><span>Umidade / Vento</span><strong>${w.humidity_pct ?? '—'}% · ${w.wind_speed != null ? `${Math.round(w.wind_speed)} km/h` : '—'}</strong></div>`;
    } else {
      wBox.textContent = 'Clima ainda não configurado. Informe sua cidade e clique em Salvar.';
    }
  }

  // GitHub live box
  const gh = liveState.github || {};
  const ghBox = $('githubLiveBox');
  if (ghBox) {
    if (gh.error) {
      ghBox.innerHTML = `<div class="raw-error">${api.escapeHtml(gh.error)}</div>`;
    } else if (gh.configured) {
      ghBox.innerHTML = `
        <div class="live-kpi">@${api.escapeHtml(gh.username || 'github')}</div>
        <div class="live-row"><span>Contribuições (ano)</span><strong>${api.formatCompactNumber(gh.total_contributions || 0)}</strong></div>
        <div class="live-row"><span>Streak atual / recorde</span><strong>${gh.current_streak || 0}d / ${gh.longest_streak || 0}d</strong></div>
        <div class="live-row"><span>Semana atual / anterior</span><strong>${gh.this_week_total || 0} / ${gh.last_week_total || 0}</strong></div>`;
    } else {
      ghBox.textContent = 'GitHub não configurado. Cole seu Personal Access Token (escopo read:user) e clique em Salvar.';
    }
  }

  // Agenda live box
  const ag = liveState.agenda || {};
  const agBox = $('agendaLiveBox');
  if (agBox) {
    const evs = Array.isArray(ag.events) ? ag.events : [];
    if (ag.error) {
      agBox.innerHTML = `<div class="raw-error">${api.escapeHtml(ag.error)}</div>`;
    } else if (!evs.length) {
      agBox.textContent = 'Nenhum evento sincronizado nos próximos 7 dias.';
    } else {
      agBox.innerHTML = evs.slice(0, 6).map((ev) => `
        <div class="live-row">
          <span><strong>${api.escapeHtml(ev.day_key || '')}</strong> · ${api.escapeHtml(ev.title || '')}</span>
          <span>${api.escapeHtml(ev.source || '')}</span>
        </div>`).join('') + (evs.length > 6 ? `<div style="font-size:11.5px;color:var(--muted)">+ ${evs.length - 6} eventos nos próximos dias</div>` : '');
    }
  }

  // AI live box
  const ai = liveState.ai || {};
  const aiBox = $('aiLiveBox');
  if (aiBox) {
    aiBox.innerHTML = `
      <div class="live-kpi">${api.escapeHtml(ai.provider_label || 'Antigravity')}</div>
      <div class="live-row"><span>Janela 5h</span><strong>${api.formatCompactNumber(ai.window_5h?.tokens || 0)} tokens</strong></div>
      <div class="live-row"><span>Hoje</span><strong>${api.formatCompactNumber(ai.today?.total_tokens || 0)} tokens</strong></div>
      <div class="live-row"><span>Semanal (7d)</span><strong>${api.formatCompactNumber(ai.weekly?.tokens || 0)} tokens</strong></div>
      <div class="live-row"><span>Sessões ativas</span><strong>${(ai.sessions || []).filter((s) => s.status === 'running' || s.status === 'waiting').length}</strong></div>`;
  }
}

function collectPatchFromForm() {
  const latRaw = $('weatherLatInput').value.trim();
  const lonRaw = $('weatherLonInput').value.trim();
  const b5Raw = $('aiBudget5hInput').value.trim();
  const bwRaw = $('aiBudgetWeeklyInput').value.trim();
  const newTok = $('githubTokenInput').value.trim();

  const patch = {
    theme: draft.theme,
    auto_open_on_usb: $('autoOpenUsbInput').checked,
    remote_approval_enabled: $('remoteApprovalInput').checked,
    approval_timeout_secs: Math.max(5, Math.min(300, Number($('approvalTimeoutInput').value) || 45)),
    calendar_ics_url: $('agendaIcsInput').value.trim() || null,
    home_widgets: draft.home_widgets.length ? draft.home_widgets : ['ai', 'weather', 'agenda', 'media', 'github'],
    night_mode: {
      enabled: $('nightEnabledInput').checked,
      start_hour: Math.max(0, Math.min(23, Number($('nightStartInput').value) || 0)),
      end_hour: Math.max(0, Math.min(23, Number($('nightEndInput').value) || 0)),
      dim_when_pc_locked: $('nightDimLockedInput').checked,
    },
    weather: {
      city: $('weatherCityInput').value.trim(),
      latitude: latRaw !== '' && Number.isFinite(Number(latRaw)) ? Number(latRaw) : null,
      longitude: lonRaw !== '' && Number.isFinite(Number(lonRaw)) ? Number(lonRaw) : null,
      unit: $('weatherUnitInput').value === 'f' ? 'f' : 'c',
    },
    github: {
      username: $('githubUserInput').value.trim() || null,
      token_env: $('githubTokenEnvInput').value.trim() || 'GITHUB_TOKEN',
      token_action: githubTokenClear ? 'clear' : (newTok ? 'set' : 'keep'),
      new_token: newTok || null,
    },
    ai: {
      provider: $('aiProviderInput').value || 'antigravity',
      budget_5h_tokens: b5Raw !== '' && Number(b5Raw) > 0 ? Math.round(Number(b5Raw)) : null,
      budget_weekly_tokens: bwRaw !== '' && Number(bwRaw) > 0 ? Math.round(Number(bwRaw)) : null,
    },
    shortcuts: draft.shortcuts.slice(0, 16),
  };
  return patch;
}

async function loadConfig() {
  const cfg = await api.apiRequest('GET', '/api/config');
  hydrateFormFromConfig(cfg);
}

async function saveVisualConfig() {
  const btn = $('saveConfigBtn');
  if (btn) btn.disabled = true;
  try {
    const patch = collectPatchFromForm();
    const updated = await api.apiRequest('PUT', '/api/config', patch);
    hydrateFormFromConfig(updated);
    showToast('Configurações salvas e aplicadas ao vivo!', 'ok');
  } catch (err) {
    showToast(err?.message || 'Erro ao salvar configurações.', 'err');
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function saveRawJson() {
  const errBox = $('rawJsonError');
  const raw = $('rawJsonArea').value;
  try {
    JSON.parse(raw);
    errBox.classList.add('hidden');
  } catch (e) {
    errBox.textContent = `JSON inválido: ${e.message}`;
    errBox.classList.remove('hidden');
    return;
  }

  const btn = $('saveRawJsonBtn');
  if (btn) btn.disabled = true;
  try {
    const updated = await api.apiRequest('PUT', '/api/config/raw', { raw_json: raw });
    hydrateFormFromConfig(updated);
    showToast('config.json aplicado com sucesso!', 'ok');
  } catch (err) {
    errBox.textContent = err?.message || 'Erro ao aplicar JSON.';
    errBox.classList.remove('hidden');
  } finally {
    if (btn) btn.disabled = false;
  }
}

function showTokenGate(errMsg = '') {
  $('tokenOverlay').classList.remove('hidden');
  const errEl = $('tokenError');
  if (errMsg) {
    errEl.textContent = errMsg;
    errEl.classList.remove('hidden');
  } else {
    errEl.classList.add('hidden');
  }
}

function hideTokenGate() {
  $('tokenOverlay').classList.add('hidden');
}

function connectLive(token) {
  if (streamHandle) {
    streamHandle.stop();
    streamHandle = null;
  }
  streamHandle = api.startStream({
    token,
    onState: (next) => {
      liveState = next;
      renderLivePanels();
    },
    onConnChange: (status) => {
      const dot = $('serverDot');
      const txt = $('serverStatusText');
      if (!dot || !txt) return;
      if (status === 'connected') {
        dot.className = 'status-dot ok';
        txt.textContent = 'Servidor conectado';
      } else if (status === 'connecting' || status === 'reconnecting') {
        dot.className = 'status-dot warn';
        txt.textContent = 'Reconectando…';
      } else {
        dot.className = 'status-dot err';
        txt.textContent = 'Desconectado';
      }
    },
    onUnauthorized: () => {
      showTokenGate('Token inválido ou expirado.');
    },
  });
}

function wireEvents() {
  document.querySelectorAll('.admin-nav-btn[data-section]').forEach((btn) => {
    btn.addEventListener('click', () => setSection(btn.dataset.section));
  });

  $('saveConfigBtn').addEventListener('click', saveVisualConfig);
  $('reloadConfigBtn').addEventListener('click', async () => {
    try {
      await loadConfig();
      showToast('Configurações recarregadas do disco.', 'ok');
    } catch (err) {
      showToast(err?.message || 'Falha ao recarregar.', 'err');
    }
  });

  // Dirty tracking on inputs
  $('adminContent').addEventListener('input', (ev) => {
    if (ev.target.id === 'rawJsonArea') return;
    if (ev.target.id === 'startupEnabledInput') return;

    const scIdx = ev.target.dataset.scIdx;
    if (scIdx !== undefined) {
      const i = Number(scIdx);
      const sc = draft.shortcuts[i];
      if (!sc) return;
      if (ev.target.dataset.scField) {
        sc[ev.target.dataset.scField] = ev.target.value;
        if (ev.target.dataset.scField === 'icon' || ev.target.dataset.scField === 'color') {
          renderShortcutsEditor();
        }
      } else if (ev.target.dataset.scActionField) {
        const f = ev.target.dataset.scActionField;
        if (!sc.action) sc.action = { type: 'hotkey' };
        if (f === 'args') {
          sc.action.args = ev.target.value.trim() ? ev.target.value.trim().split(/\s+/) : [];
        } else {
          sc.action[f] = ev.target.value;
        }
      }
    }
    setDirty(true);
  });

  $('adminContent').addEventListener('change', async (ev) => {
    if (ev.target.id === 'startupEnabledInput') {
      const enabled = ev.target.checked;
      try {
        const res = await api.apiRequest('POST', '/api/admin/startup', { enabled });
        ev.target.checked = Boolean(res.startup_enabled);
        showToast(res.startup_enabled ? 'Auto-start com o Windows ativado.' : 'Auto-start removido.', 'ok');
      } catch (err) {
        ev.target.checked = !enabled;
        showToast(err?.message || 'Erro ao alterar auto-start.', 'err');
      }
      return;
    }

    const wToggle = ev.target.dataset.widgetToggle;
    if (wToggle) {
      if (ev.target.checked) {
        if (!draft.home_widgets.includes(wToggle)) draft.home_widgets.push(wToggle);
      } else {
        draft.home_widgets = draft.home_widgets.filter((k) => k !== wToggle);
      }
      renderHomeWidgetsList();
      setDirty(true);
      return;
    }

    if (ev.target.dataset.scActionType) {
      const i = Number(ev.target.dataset.scIdx);
      if (draft.shortcuts[i]) {
        draft.shortcuts[i].action = defaultActionForType(ev.target.value);
        renderShortcutsEditor();
        setDirty(true);
      }
      return;
    }

    if (ev.target.id !== 'rawJsonArea') {
      setDirty(true);
    }
  });

  $('adminContent').addEventListener('click', async (ev) => {
    const themePick = ev.target.closest('[data-theme-pick]');
    if (themePick) {
      draft.theme = themePick.dataset.themePick;
      applyThemePreview(draft.theme);
      renderThemeGrid();
      setDirty(true);
      return;
    }

    const wMove = ev.target.closest('[data-widget-move]');
    if (wMove) {
      const key = wMove.dataset.widgetMove;
      const dir = Number(wMove.dataset.dir);
      const idx = draft.home_widgets.indexOf(key);
      const nextIdx = idx + dir;
      if (idx >= 0 && nextIdx >= 0 && nextIdx < draft.home_widgets.length) {
        const [item] = draft.home_widgets.splice(idx, 1);
        draft.home_widgets.splice(nextIdx, 0, item);
        renderHomeWidgetsList();
        setDirty(true);
      }
      return;
    }

    const scMove = ev.target.closest('[data-sc-move]');
    if (scMove) {
      const idx = Number(scMove.dataset.scMove);
      const dir = Number(scMove.dataset.dir);
      const nextIdx = idx + dir;
      if (idx >= 0 && nextIdx >= 0 && nextIdx < draft.shortcuts.length) {
        const [item] = draft.shortcuts.splice(idx, 1);
        draft.shortcuts.splice(nextIdx, 0, item);
        renderShortcutsEditor();
        setDirty(true);
      }
      return;
    }

    const scDel = ev.target.closest('[data-sc-del]');
    if (scDel) {
      const idx = Number(scDel.dataset.scDel);
      draft.shortcuts.splice(idx, 1);
      renderShortcutsEditor();
      setDirty(true);
      return;
    }

    const scTest = ev.target.closest('[data-sc-test]');
    if (scTest) {
      const id = scTest.dataset.scTest;
      try {
        await api.apiRequest('POST', `/api/shortcuts/${encodeURIComponent(id)}/trigger`);
        showToast(`Atalho "${id}" disparado no PC!`, 'ok');
      } catch (err) {
        showToast(err?.message || 'Salve antes de testar novos atalhos.', 'err');
      }
    }
  });

  $('addShortcutBtn').addEventListener('click', () => {
    if (!draft || draft.shortcuts.length >= 16) return;
    const id = `custom_${Date.now().toString(36)}`;
    draft.shortcuts.push({
      id,
      label: `Atalho ${draft.shortcuts.length + 1}`,
      icon: 'zap',
      color: '#60a5fa',
      action: { type: 'url', url: 'https://github.com' },
    });
    renderShortcutsEditor();
    setDirty(true);
  });

  $('clearGithubTokenBtn').addEventListener('click', () => {
    githubTokenClear = true;
    $('githubTokenInput').value = '';
    $('githubTokenStatus').innerHTML = '<strong>Token marcado para remoção</strong> — clique em Salvar alterações.';
    setDirty(true);
  });

  $('refreshAgendaBtn').addEventListener('click', async () => {
    try {
      await api.apiRequest('POST', '/api/agenda/refresh');
      showToast('Feed ICS atualizado!', 'ok');
    } catch (err) {
      showToast(err?.message || 'Nenhum feed ICS configurado ou erro ao buscar.', 'err');
    }
  });

  $('openPhoneBtn').addEventListener('click', async () => {
    const btn = $('openPhoneBtn');
    btn.disabled = true;
    try {
      const res = await api.apiRequest('POST', '/api/admin/open-phone');
      showToast(`DeskPulse aberto no celular (${res.device})!`, 'ok');
    } catch (err) {
      showToast(err?.message || 'Nenhum celular conectado via USB.', 'err');
    } finally {
      btn.disabled = false;
    }
  });

  $('restartAdbBtn').addEventListener('click', async () => {
    const btn = $('restartAdbBtn');
    btn.disabled = true;
    try {
      await api.apiRequest('POST', '/api/admin/restart-adb');
      showToast('Daemon ADB reiniciado com sucesso!', 'ok');
    } catch (err) {
      showToast(err?.message || 'Erro ao reiniciar ADB.', 'err');
    } finally {
      btn.disabled = false;
    }
  });

  $('formatJsonBtn').addEventListener('click', () => {
    const errBox = $('rawJsonError');
    try {
      const parsed = JSON.parse($('rawJsonArea').value);
      $('rawJsonArea').value = JSON.stringify(parsed, null, 2);
      errBox.classList.add('hidden');
    } catch (e) {
      errBox.textContent = `JSON inválido: ${e.message}`;
      errBox.classList.remove('hidden');
    }
  });

  $('saveRawJsonBtn').addEventListener('click', saveRawJson);

  $('tokenForm').addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const tok = $('tokenInput').value.trim();
    if (!tok) return;
    api.saveToken(tok);
    try {
      await loadConfig();
      hideTokenGate();
      connectLive(tok);
      showToast('Conectado ao DeskPulse!', 'ok');
    } catch (err) {
      showTokenGate(err?.message || 'Token recusado.');
    }
  });

  // Ctrl+S shortcut to save
  window.addEventListener('keydown', (ev) => {
    if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 's') {
      ev.preventDefault();
      const rawActive = document.querySelector('.panel[data-panel="raw"]')?.classList.contains('active');
      if (rawActive) {
        saveRawJson();
      } else {
        saveVisualConfig();
      }
    }
  });
}

async function init() {
  hydrateIcons();
  wireEvents();

  const token = api.loadToken();
  if (!token) {
    showTokenGate();
    return;
  }

  try {
    await loadConfig();
    connectLive(token);
  } catch (err) {
    if (err?.unauthorized) {
      showTokenGate('Token inválido ou expirado.');
    } else {
      showToast(err?.message || 'Erro ao carregar configurações.', 'err');
    }
  }
}

init();
