let activeSubTab = 'uso';
let selectedSessionId = null;
let mountedEl = null;
let lastToolbarSig = '';
let lastSegSig = '';
let lastPanelSig = '';
let approvalToggling = false;
let latestCtx = null;

function clamp(n, min, max) {
  return Math.max(min, Math.min(max, n));
}

function pctColor(pct) {
  if (pct >= 85) return 'var(--red)';
  if (pct >= 65) return 'var(--amber)';
  return 'var(--ai)';
}

function resolveSessions(state) {
  const agy = Array.isArray(state?.antigravity?.sessions)
    ? state.antigravity.sessions.filter(Boolean)
    : [];
  if (agy.length) return agy;
  const ai = Array.isArray(state?.ai?.sessions)
    ? state.ai.sessions.filter(Boolean)
    : [];
  return ai;
}

function formatTimeMs(ms, withSeconds = false) {
  const n = Number(ms);
  if (!Number.isFinite(n) || n <= 0) return '';
  const d = new Date(n);
  if (Number.isNaN(d.getTime())) return '';
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  if (!withSeconds) return `${hh}:${mm}`;
  const ss = String(d.getSeconds()).padStart(2, '0');
  return `${hh}:${mm}:${ss}`;
}

function sessionStatusMeta(status) {
  const s = String(status || '').toLowerCase();
  if (s === 'running') {
    return { label: 'Rodando', dot: 'status-ok', badge: 'badge-ok' };
  }
  if (s === 'waiting') {
    return { label: 'Aguardando', dot: 'status-warn', badge: 'badge-warn' };
  }
  return { label: 'Idle', dot: 'status-muted', badge: '' };
}

function notificationLevelBadge(level, kind) {
  const l = String(level || '').toLowerCase();
  const k = String(kind || '').toLowerCase();
  if (l === 'error' || l === 'err' || l === 'danger') return 'badge-err';
  if (l === 'warn' || l === 'warning') return 'badge-warn';
  if (l === 'ok' || l === 'success') return 'badge-ok';
  if (k === 'approval' || k === 'permission') return 'badge-ai';
  return 'badge-accent';
}

function formatEventKind(kind) {
  const k = String(kind || '').trim().toLowerCase();
  if (!k) return 'Evento';
  if (k === 'stop') return 'Conclusão';
  if (k === 'approval') return 'Aprovação';
  if (k === 'ask_question') return 'Pergunta';
  if (k === 'notify') return 'Alerta';
  if (k === 'error') return 'Erro';
  return kind;
}

function renderWindowCard(title, metric, emptyCaption, ctx) {
  const { escapeHtml, formatCompactNumber } = ctx;
  const tokens = Math.max(0, Number(metric?.tokens || 0));
  const rawPct = metric?.pct;
  const hasPct = rawPct !== null && rawPct !== undefined && Number.isFinite(Number(rawPct));
  const budget = metric?.budget_tokens !== null && metric?.budget_tokens !== undefined && Number(metric.budget_tokens) > 0
    ? Number(metric.budget_tokens)
    : null;
  const resetLabel = String(metric?.reset_label || '').trim();

  if (hasPct) {
    const pct = clamp(Number(rawPct), 0, 100);
    const col = pctColor(pct);
    const pctRounded = Math.round(pct);
    const subText = resetLabel || (budget
      ? `${formatCompactNumber(tokens)} / ${formatCompactNumber(budget)} tokens`
      : `${formatCompactNumber(tokens)} tokens`);

    return `
      <div class="card ai-window-card">
        <div class="card-head" style="margin-bottom:2px">
          <span class="card-label">${escapeHtml(title)}</span>
          <span class="ai-sub-muted num">${escapeHtml(formatCompactNumber(tokens))} tok</span>
        </div>
        <div class="ai-hero-num num" style="color:${col}">${pctRounded}%</div>
        <div class="ai-bar-track" aria-hidden="true">
          <div class="ai-bar-fill" style="width:${pct.toFixed(1)}%;background:${col}"></div>
        </div>
        <div class="ai-sub-muted num" title="${escapeHtml(subText)}">${escapeHtml(subText)}</div>
      </div>`;
  }

  return `
    <div class="card ai-window-card">
      <div class="card-head" style="margin-bottom:2px">
        <span class="card-label">${escapeHtml(title)}</span>
      </div>
      <div class="ai-hero-num num" style="color:var(--ai)">${escapeHtml(formatCompactNumber(tokens))}</div>
      <div style="display:flex;flex-direction:column;gap:2px">
        <div class="ai-sub-line">${escapeHtml(emptyCaption)}</div>
        <div class="ai-sub-muted">Sem teto configurado</div>
      </div>
    </div>`;
}

function buildUsageSubView(ctx) {
  const { state, charts, escapeHtml, formatCompactNumber } = ctx;
  const ai = state?.ai || null;

  const w5 = ai?.window_5h || {};
  const wk = ai?.weekly || {};
  const today = ai?.today || {};
  const last7 = Array.isArray(ai?.last_7_days) ? ai.last_7_days : [];
  const modelsToday = Array.isArray(ai?.models_today) ? ai.models_today : [];
  const models7d = Array.isArray(ai?.models_7d) ? ai.models_7d : [];

  const using7dModels = modelsToday.length === 0 && models7d.length > 0;
  const activeModels = using7dModels ? models7d : modelsToday;
  const modelsTitle = using7dModels ? 'MODELOS (7D)' : 'MODELOS HOJE';

  let peak = 0;
  const barItems = last7.map((b) => {
    const val = Math.max(0, Number(b?.total_tokens || 0));
    if (val > peak) peak = val;
    return {
      label: b?.label || '',
      value: val,
    };
  });

  const sig = JSON.stringify([
    'uso',
    w5.tokens || 0,
    w5.budget_tokens ?? null,
    w5.pct ?? null,
    w5.reset_label || '',
    wk.tokens || 0,
    wk.budget_tokens ?? null,
    wk.pct ?? null,
    wk.reset_label || '',
    today.total_tokens || 0,
    today.output_tokens || 0,
    today.cache_read_tokens || 0,
    today.messages || 0,
    today.last_hour_tokens || 0,
    barItems,
    using7dModels ? 1 : 0,
    activeModels.map((m) => [m.display_name || m.model, m.total_tokens || 0]),
  ]);

  const maxModelTokens = Math.max(1, ...activeModels.map((m) => Math.max(0, Number(m?.total_tokens || 0))));

  const modelsHtml = activeModels.length
    ? activeModels.map((m) => {
        const name = m?.display_name || m?.model || 'Desconhecido';
        const val = Math.max(0, Number(m?.total_tokens || 0));
        const pct = val > 0 ? Math.max(4, (val / maxModelTokens) * 100) : 0;
        return `
          <div class="ai-model-row">
            <div class="ai-model-head">
              <span class="ai-model-name" title="${escapeHtml(name)}">${escapeHtml(name)}</span>
              <span class="ai-model-val num">${escapeHtml(formatCompactNumber(val))}</span>
            </div>
            <div class="ai-model-bar" aria-hidden="true">
              <div class="ai-bar-fill" style="width:${pct.toFixed(1)}%"></div>
            </div>
          </div>`;
      }).join('')
    : '<div class="empty-state" style="padding:8px"><div class="empty-state-sub">Sem uso registrado</div></div>';

  const barSvg = charts.renderBarChart({
    items: barItems,
    color: 'var(--ai)',
    height: 108,
  });

  const html = `
    <div class="ai-top-3">
      ${renderWindowCard('JANELA 5H', w5, 'tokens nas últimas 5h', ctx)}
      ${renderWindowCard('SEMANAL', wk, 'tokens nos últimos 7 dias', ctx)}
      <div class="card">
        <div class="card-head" style="margin-bottom:4px">
          <span class="card-label">HOJE</span>
        </div>
        <div class="ai-kv-table">
          <div class="ai-kv-row">
            <span class="ai-kv-key">Tokens</span>
            <span class="ai-kv-val num">${escapeHtml(formatCompactNumber(today.total_tokens || 0))}</span>
          </div>
          <div class="ai-kv-row">
            <span class="ai-kv-key">Output</span>
            <span class="ai-kv-val num">${escapeHtml(formatCompactNumber(today.output_tokens || 0))}</span>
          </div>
          <div class="ai-kv-row">
            <span class="ai-kv-key">Cache</span>
            <span class="ai-kv-val num">${escapeHtml(formatCompactNumber(today.cache_read_tokens || 0))}</span>
          </div>
          <div class="ai-kv-row">
            <span class="ai-kv-key">Mensagens</span>
            <span class="ai-kv-val num">${escapeHtml(formatCompactNumber(today.messages || 0))}</span>
          </div>
          <div class="ai-kv-row">
            <span class="ai-kv-key">Última hora</span>
            <span class="ai-kv-val num">${escapeHtml(formatCompactNumber(today.last_hour_tokens || 0))}</span>
          </div>
        </div>
      </div>
    </div>
    <div class="ai-bottom-split">
      <div class="card chart-card">
        <div class="card-head">
          <span class="card-label">ÚLTIMOS 7 DIAS</span>
          <span class="badge num">pico ${escapeHtml(formatCompactNumber(peak))}</span>
        </div>
        <div class="chart-wrap">${barSvg}</div>
      </div>
      <div class="card" style="min-height:0">
        <div class="card-head">
          <span class="card-label">${escapeHtml(modelsTitle)}</span>
          ${activeModels.length ? `<span class="ai-sub-muted num">${activeModels.length} ${activeModels.length === 1 ? 'modelo' : 'modelos'}</span>` : ''}
        </div>
        <div class="ai-models-list scroll-y">${modelsHtml}</div>
      </div>
    </div>`;

  return { sig, html };
}

function buildSessionsSubView(ctx) {
  const { state, icons, escapeHtml, formatCompactNumber } = ctx;
  const sessions = resolveSessions(state);

  if (!sessions.length) {
    return {
      sig: 'sessoes:empty',
      html: `
        <div class="card empty-state" style="flex:1">
          <span style="color:var(--ai);opacity:0.8">${icons.icon('ai', 32)}</span>
          <div class="empty-state-title">Nenhuma sessão ativa</div>
          <div class="empty-state-sub">As sessões do Antigravity das últimas 24h aparecerão aqui em tempo real com checklist e timeline de ferramentas.</div>
        </div>`,
    };
  }

  let selected = sessions.find((s) => s && s.id === selectedSessionId) || sessions[0];
  selectedSessionId = selected.id;

  const sig = JSON.stringify([
    'sessoes',
    selectedSessionId,
    sessions.map((s) => [
      s.id,
      s.title,
      s.status,
      s.current_step,
      s.current_tool,
      s.current_detail,
      s.thinking_title,
      s.last_prompt,
      s.step_index,
      s.context_tokens,
      s.task_done,
      s.task_total,
      s.updated_at_ms,
      s.tasks,
      s.recent_actions,
    ]),
  ]);

  const activeCount = sessions.filter((s) => s && (s.status === 'running' || s.status === 'waiting')).length;

  const listHtml = sessions.map((s) => {
    const meta = sessionStatusMeta(s.status);
    const isSel = s.id === selected.id;
    const stepText = s.current_tool
      ? `${s.current_tool}${s.current_detail ? ` · ${s.current_detail}` : ''}`
      : (s.current_step || meta.label);
    const taskCount = Number(s.task_total) > 0
      ? `<span class="badge num" style="padding:1px 6px;font-size:10px">${Number(s.task_done || 0)}/${Number(s.task_total)}</span>`
      : '';
    const updatedClock = formatTimeMs(s.updated_at_ms, false);

    return `
      <button type="button" class="session-item ${isSel ? 'is-selected' : ''}" data-session-id="${escapeHtml(s.id)}">
        <div class="session-item-top">
          <span class="status-dot ${meta.dot}" aria-hidden="true"></span>
          <span class="session-item-title" title="${escapeHtml(s.title || s.id)}">${escapeHtml(s.title || s.id)}</span>
          ${taskCount}
        </div>
        <div class="session-item-meta">
          <span class="session-item-step" title="${escapeHtml(stepText)}">${escapeHtml(stepText)}</span>
          <span class="num" style="flex-shrink:0">${escapeHtml(updatedClock || meta.label)}</span>
        </div>
      </button>`;
  }).join('');

  const selMeta = sessionStatusMeta(selected.status);
  const stepBadge = selected.step_index !== null && selected.step_index !== undefined
    ? `<span class="badge num">Passo #${escapeHtml(selected.step_index)}</span>`
    : '';
  const ctxBadge = selected.context_tokens !== null && selected.context_tokens !== undefined
    ? `<span class="badge num">Ctx ${escapeHtml(formatCompactNumber(selected.context_tokens))}</span>`
    : '';

  const thinkingText = selected.thinking_title || selected.current_step || '';
  const thinkingBlock = thinkingText
    ? `
      <div class="ai-detail-block">
        <span class="ai-detail-label">Pensamento / Etapa</span>
        <div class="ai-detail-text">${escapeHtml(thinkingText)}</div>
      </div>`
    : '';

  const toolBlock = (selected.current_tool || selected.current_detail)
    ? `
      <div class="ai-detail-block">
        <span class="ai-detail-label">Ferramenta Atual</span>
        ${selected.current_tool ? `<span class="ai-detail-mono">${escapeHtml(selected.current_tool)}</span>` : ''}
        ${selected.current_detail ? `<div class="ai-detail-text" style="color:var(--text-2)">${escapeHtml(selected.current_detail)}</div>` : ''}
      </div>`
    : '';

  const promptBlock = selected.last_prompt
    ? `
      <div class="ai-detail-block">
        <span class="ai-detail-label">Último Prompt</span>
        <div class="ai-detail-text" style="color:var(--text-2)">${escapeHtml(selected.last_prompt)}</div>
      </div>`
    : '';

  const tasks = Array.isArray(selected.tasks) ? selected.tasks : [];
  const tasksBlock = tasks.length
    ? `
      <div class="ai-detail-block">
        <span class="ai-detail-label">Checklist task.md (${Number(selected.task_done || 0)}/${Number(selected.task_total || tasks.length)})</span>
        <div class="task-check-list">
          ${tasks.map((t) => {
            const st = String(t?.state || 'todo').toLowerCase();
            const cls = st === 'done' ? 'is-done' : (st === 'active' ? 'is-active' : '');
            const mark = st === 'done'
              ? '<span class="task-check-mark" style="color:var(--green)">✓</span>'
              : (st === 'active'
                ? '<span class="task-check-mark" style="color:var(--amber)">●</span>'
                : '<span class="task-check-mark" style="color:var(--muted)">○</span>');
            return `<div class="task-check-item ${cls}">${mark}<span>${escapeHtml(t?.text || '')}</span></div>`;
          }).join('')}
        </div>
      </div>`
    : '';

  const actions = Array.isArray(selected.recent_actions) ? selected.recent_actions : [];
  const actionsBlock = actions.length
    ? `
      <div class="ai-detail-block">
        <span class="ai-detail-label">Ações Recentes</span>
        <div class="ai-actions-list">
          ${actions.map((a) => {
            const atStr = formatTimeMs(a?.at_ms, true);
            const stepStr = a?.step_index !== null && a?.step_index !== undefined ? `#${a.step_index}` : '';
            const timeOrStep = atStr || stepStr;
            return `
              <div class="ai-action-row">
                ${timeOrStep ? `<span class="ai-action-time num">${escapeHtml(timeOrStep)}</span>` : ''}
                <span class="ai-action-tool">${escapeHtml(a?.tool || 'tool')}</span>
                <span class="ai-action-detail" title="${escapeHtml(a?.detail || '')}">${escapeHtml(a?.detail || '')}</span>
              </div>`;
          }).join('')}
        </div>
      </div>`
    : '';

  const detailContent = (thinkingBlock || toolBlock || promptBlock || tasksBlock || actionsBlock)
    ? `${thinkingBlock}${toolBlock}${promptBlock}${tasksBlock}${actionsBlock}`
    : '<div class="empty-state" style="padding:10px"><div class="empty-state-sub">Sem detalhes adicionais para esta sessão.</div></div>';

  const html = `
    <div class="ai-master-detail">
      <div class="card" style="min-height:0">
        <div class="card-head">
          <span class="card-label">Sessões (${sessions.length})</span>
          ${activeCount > 0 ? `<span class="badge badge-ok num">${activeCount} ativa${activeCount > 1 ? 's' : ''}</span>` : ''}
        </div>
        <div class="ai-sessions-grid scroll-y" data-ai-sessions-list>${listHtml}</div>
      </div>
      <div class="card" style="min-height:0">
        <div class="card-head">
          <span class="session-item-title" style="font-size:13px" title="${escapeHtml(selected.title || selected.id)}">${escapeHtml(selected.title || selected.id)}</span>
          <div style="display:flex;align-items:center;gap:5px;flex-shrink:0">
            ${ctxBadge}
            ${stepBadge}
            <span class="badge ${selMeta.badge}">${escapeHtml(selMeta.label)}</span>
          </div>
        </div>
        <div class="ai-detail-body scroll-y" data-ai-session-detail>${detailContent}</div>
      </div>
    </div>`;

  return { sig, html };
}

function buildEventsSubView(ctx) {
  const { state, icons, escapeHtml } = ctx;
  const list = Array.isArray(state?.notifications) ? state.notifications : [];

  const sig = JSON.stringify([
    'eventos',
    list.map((n) => [n?.id, n?.kind, n?.title, n?.body, n?.level, n?.created_at_ms]),
  ]);

  if (!list.length) {
    return {
      sig,
      html: `
        <div class="card empty-state" style="flex:1">
          <span style="color:var(--muted);opacity:0.8">${icons.icon('bell', 30)}</span>
          <div class="empty-state-title">Nenhum evento recente</div>
          <div class="empty-state-sub">Notificações dos agentes, conclusões de tarefas e alertas aparecerão aqui ao vivo.</div>
        </div>`,
    };
  }

  const ordered = list.slice().reverse();
  const rowsHtml = ordered.map((n) => {
    const timeStr = formatTimeMs(n?.created_at_ms, true) || '--:--';
    const badgeCls = notificationLevelBadge(n?.level, n?.kind);
    const kindLabel = formatEventKind(n?.kind);
    const title = n?.title || kindLabel;
    const body = n?.body || '';

    return `
      <div class="ai-event-row">
        <div class="ai-event-head">
          <span class="badge ${badgeCls}">${escapeHtml(kindLabel)}</span>
          <span class="ai-event-title" title="${escapeHtml(title)}">${escapeHtml(title)}</span>
          <span class="ai-sub-muted num" style="flex-shrink:0">${escapeHtml(timeStr)}</span>
        </div>
        ${body ? `<div class="ai-event-body">${escapeHtml(body)}</div>` : ''}
      </div>`;
  }).join('');

  const html = `
    <div class="card" style="flex:1;min-height:0">
      <div class="card-head">
        <span class="card-label">Eventos ao vivo</span>
        <span class="badge num">${list.length} ${list.length === 1 ? 'evento' : 'eventos'}</span>
      </div>
      <div class="ai-events-list scroll-y" data-ai-events-list>${rowsHtml}</div>
    </div>`;

  return { sig, html };
}

function buildToolbarControls(ctx) {
  const { state, icons, escapeHtml, soundEnabled } = ctx;
  const providerLabel = state?.ai?.provider_label || 'Antigravity';
  const approvalOn = Boolean(state?.remote_approval_enabled);

  const sig = [
    providerLabel,
    approvalOn ? 1 : 0,
    approvalToggling ? 1 : 0,
    soundEnabled ? 1 : 0,
  ].join('|');

  const html = `
    <span class="badge badge-ai ai-provider-badge">${icons.icon('ai', 12)}<span>${escapeHtml(providerLabel)}</span></span>
    <button type="button" class="ai-ctrl-btn ${approvalOn ? 'is-on' : 'is-off'}" data-action="toggle-approval" ${approvalToggling ? 'disabled' : ''} title="Alternar aprovação remota">
      ${icons.icon(approvalOn ? 'check' : 'x', 12)}
      <span>Aprovação: ${approvalOn ? 'ON' : 'OFF'}</span>
    </button>
    <button type="button" class="ai-ctrl-btn ${soundEnabled ? 'is-on' : 'is-off'}" data-action="toggle-sound" title="Alternar som de alertas">
      ${icons.icon(soundEnabled ? 'bell' : 'bell_off', 12)}
      <span>Som: ${soundEnabled ? 'ON' : 'OFF'}</span>
    </button>
    <button type="button" class="ai-ctrl-btn" data-action="open-token" title="Alterar token de acesso">
      ${icons.icon('lock', 12)}
      <span>Token</span>
    </button>`;

  return { sig, html };
}

async function handleToggleApproval() {
  if (!latestCtx || approvalToggling) return;
  approvalToggling = true;
  const nextVal = !Boolean(latestCtx.state?.remote_approval_enabled);
  lastToolbarSig = '';
  render(latestCtx);

  try {
    const res = await latestCtx.apiRequest('POST', '/api/settings', {
      remote_approval_enabled: nextVal,
    });
    if (latestCtx.state) {
      latestCtx.state.remote_approval_enabled =
        typeof res?.remote_approval_enabled === 'boolean' ? res.remote_approval_enabled : nextVal;
    }
  } catch {
    // Keep server-authoritative state on failure
  } finally {
    approvalToggling = false;
    lastToolbarSig = '';
    if (latestCtx) render(latestCtx);
  }
}

function ensureShell(mount) {
  if (mountedEl === mount && mount.querySelector('.ai-layout')) {
    return;
  }
  mountedEl = mount;
  lastToolbarSig = '';
  lastSegSig = '';
  lastPanelSig = '';

  mount.innerHTML = `
    <div class="ai-layout">
      <div class="ai-toolbar">
        <div class="segmented" role="tablist" aria-label="Sub-abas de IA e Agentes" data-ai-seg>
          <button type="button" class="seg-btn active" role="tab" aria-selected="true" data-subtab="uso">Uso</button>
          <button type="button" class="seg-btn" role="tab" aria-selected="false" data-subtab="sessoes">Sessões</button>
          <button type="button" class="seg-btn" role="tab" aria-selected="false" data-subtab="eventos">Eventos</button>
        </div>
        <div class="ai-controls" data-ai-controls></div>
      </div>
      <div class="ai-panel" data-ai-panel></div>
    </div>`;

  mount.addEventListener('click', (ev) => {
    const segBtn = ev.target.closest('.seg-btn[data-subtab]');
    if (segBtn) {
      const next = segBtn.dataset.subtab;
      if (next && next !== activeSubTab) {
        activeSubTab = next;
        lastPanelSig = '';
        if (latestCtx) render(latestCtx);
      }
      return;
    }

    const sessionBtn = ev.target.closest('.session-item[data-session-id]');
    if (sessionBtn) {
      const id = sessionBtn.dataset.sessionId;
      if (id && id !== selectedSessionId) {
        selectedSessionId = id;
        lastPanelSig = '';
        if (latestCtx) render(latestCtx);
      }
      return;
    }

    const actionEl = ev.target.closest('[data-action]');
    if (!actionEl || !latestCtx) return;
    const action = actionEl.dataset.action;

    if (action === 'toggle-approval') {
      handleToggleApproval();
    } else if (action === 'toggle-sound') {
      latestCtx.toggleSound();
    } else if (action === 'open-token') {
      latestCtx.showTokenGate('');
    }
  });
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;
  latestCtx = ctx;

  ensureShell(mount);

  const sessions = resolveSessions(ctx.state);
  const activeSessions = sessions.filter((s) => s && (s.status === 'running' || s.status === 'waiting')).length;
  const eventsCount = Array.isArray(ctx.state?.notifications) ? ctx.state.notifications.length : 0;

  const segSig = `${activeSubTab}|${activeSessions}|${eventsCount}`;
  if (segSig !== lastSegSig) {
    lastSegSig = segSig;
    mount.querySelectorAll('.seg-btn[data-subtab]').forEach((btn) => {
      const key = btn.dataset.subtab;
      const isActive = key === activeSubTab;
      btn.classList.toggle('active', isActive);
      btn.setAttribute('aria-selected', isActive ? 'true' : 'false');
      if (key === 'sessoes') {
        btn.textContent = activeSessions > 0 ? `Sessões (${activeSessions})` : 'Sessões';
      } else if (key === 'eventos') {
        btn.textContent = eventsCount > 0 ? `Eventos (${eventsCount})` : 'Eventos';
      }
    });
  }

  const controlsEl = mount.querySelector('[data-ai-controls]');
  if (controlsEl) {
    const tb = buildToolbarControls(ctx);
    if (tb.sig !== lastToolbarSig) {
      lastToolbarSig = tb.sig;
      controlsEl.innerHTML = tb.html;
    }
  }

  const panel = mount.querySelector('[data-ai-panel]');
  if (!panel) return;

  let view;
  if (activeSubTab === 'sessoes') {
    view = buildSessionsSubView(ctx);
  } else if (activeSubTab === 'eventos') {
    view = buildEventsSubView(ctx);
  } else {
    view = buildUsageSubView(ctx);
  }

  if (view.sig !== lastPanelSig) {
    const prevListScroll = panel.querySelector('[data-ai-sessions-list]')?.scrollTop ?? 0;
    const prevDetailScroll = panel.querySelector('[data-ai-session-detail]')?.scrollTop ?? 0;
    const prevEventsScroll = panel.querySelector('[data-ai-events-list]')?.scrollTop ?? 0;
    const prevTab = lastPanelSig.startsWith('["sessoes"')
      ? 'sessoes'
      : (lastPanelSig.startsWith('["eventos"') ? 'eventos' : 'uso');

    lastPanelSig = view.sig;
    panel.innerHTML = view.html;

    if (activeSubTab === 'sessoes' && prevTab === 'sessoes') {
      const nextList = panel.querySelector('[data-ai-sessions-list]');
      const nextDetail = panel.querySelector('[data-ai-session-detail]');
      if (nextList) nextList.scrollTop = prevListScroll;
      if (nextDetail) nextDetail.scrollTop = prevDetailScroll;
    } else if (activeSubTab === 'eventos' && prevTab === 'eventos') {
      const nextEvents = panel.querySelector('[data-ai-events-list]');
      if (nextEvents) nextEvents.scrollTop = prevEventsScroll;
    }
  }
}
