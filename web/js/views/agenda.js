const WEEKDAYS_PT = [
  'domingo',
  'segunda-feira',
  'terça-feira',
  'quarta-feira',
  'quinta-feira',
  'sexta-feira',
  'sábado',
];

const MONTHS_FULL_PT = [
  'janeiro',
  'fevereiro',
  'março',
  'abril',
  'maio',
  'junho',
  'julho',
  'agosto',
  'setembro',
  'outubro',
  'novembro',
  'dezembro',
];

const MONTHS_SHORT_PT = [
  'Jan',
  'Fev',
  'Mar',
  'Abr',
  'Mai',
  'Jun',
  'Jul',
  'Ago',
  'Set',
  'Out',
  'Nov',
  'Dez',
];

let mountedEl = null;
let latestCtx = null;
let drawerOpen = false;
let refreshing = false;
let refreshError = '';
let lastSeenSyncMs = 0;
let submittingTodo = false;

let lastHeaderSig = '';
let lastAgendaSig = '';
let lastTodoSig = '';

function pad2(n) {
  return String(n).padStart(2, '0');
}

function localDateKey(date) {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

function isDateOnlyIso(raw) {
  return typeof raw === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(raw.trim());
}

function parseLocalDateOnly(raw) {
  if (typeof raw !== 'string') return null;
  const m = raw.trim().match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return null;
  const y = Number(m[1]);
  const mo = Number(m[2]);
  const d = Number(m[3]);
  if (!Number.isFinite(y) || !Number.isFinite(mo) || !Number.isFinite(d)) return null;
  return new Date(y, mo - 1, d, 0, 0, 0, 0);
}

function parseEventDate(iso) {
  if (!iso || typeof iso !== 'string') return null;
  const trimmed = iso.trim();
  if (!trimmed) return null;
  if (isDateOnlyIso(trimmed)) {
    return parseLocalDateOnly(trimmed);
  }
  const dt = new Date(trimmed);
  return Number.isNaN(dt.getTime()) ? null : dt;
}

function resolveDayKey(ev) {
  if (ev && isDateOnlyIso(ev.day_key)) {
    return ev.day_key.trim();
  }
  if (ev && isDateOnlyIso(ev.start_iso)) {
    return ev.start_iso.trim();
  }
  const dt = parseEventDate(ev?.start_iso);
  return dt ? localDateKey(dt) : '';
}

function formatTimeHHMM(date) {
  if (!date || Number.isNaN(date.getTime())) return '';
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
}

function formatTodayBanner(now) {
  const weekday = WEEKDAYS_PT[now.getDay()] || '';
  const month = MONTHS_FULL_PT[now.getMonth()] || '';
  return `HOJE • ${weekday}, ${now.getDate()} de ${month}`;
}

function formatUpcomingDayHeader(dayKey, tomorrowKey) {
  const d = parseLocalDateOnly(dayKey);
  if (!d) return dayKey || 'PRÓXIMOS DIAS';
  const weekday = WEEKDAYS_PT[d.getDay()] || '';
  const monthShort = MONTHS_SHORT_PT[d.getMonth()] || '';
  const dayNum = pad2(d.getDate());
  if (dayKey === tomorrowKey) {
    return `AMANHÃ • ${weekday}, ${dayNum} ${monthShort}`;
  }
  return `${weekday}, ${dayNum} ${monthShort}`;
}

function formatSourceLabel(source) {
  const s = String(source || '').trim().toLowerCase();
  if (s === 'ics') return 'Calendário ICS';
  if (s === 'push') return 'Google Calendar';
  return String(source || '').trim();
}

function isTimedEventNow(ev, start, end, nowMs) {
  if (!start) return false;
  const startMs = start.getTime();
  const effectiveEndMs = end && end.getTime() > startMs
    ? end.getTime()
    : startMs + 3_600_000;
  return startMs <= nowMs && effectiveEndMs > nowMs;
}

function renderEventCard(ev, isToday, nowMs, escapeHtml) {
  const allDay = Boolean(ev.all_day) || isDateOnlyIso(ev.start_iso);
  const start = parseEventDate(ev.start_iso);
  const end = ev.end_iso ? parseEventDate(ev.end_iso) : null;
  const isNow = !allDay && isToday && isTimedEventNow(ev, start, end, nowMs);

  const barColor = isNow
    ? 'var(--amber)'
    : isToday
      ? 'var(--accent)'
      : 'var(--ram)';

  let timeHtml = '';
  if (allDay) {
    timeHtml = '<span class="agenda-allday-tag">Dia todo</span>';
  } else {
    const startStr = formatTimeHHMM(start) || '--:--';
    const endStr = end ? formatTimeHHMM(end) : '';
    timeHtml = `
      <span class="agenda-time-start num">${escapeHtml(startStr)}</span>
      ${endStr && endStr !== startStr ? `<span class="agenda-time-end num">${escapeHtml(endStr)}</span>` : ''}`;
  }

  const subParts = [];
  if (ev.location && String(ev.location).trim()) {
    subParts.push(String(ev.location).trim());
  }
  const sourceText = formatSourceLabel(ev.source);
  if (sourceText) {
    subParts.push(sourceText);
  }
  const subText = subParts.join(' · ');

  const cardClasses = [
    'agenda-event-card',
    isToday ? 'is-today-card' : '',
    isNow ? 'is-now' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return `
    <article class="${cardClasses}">
      <div class="event-bar" style="background:${barColor}"></div>
      <div class="agenda-time-col">${timeHtml}</div>
      <div class="agenda-event-content">
        <div class="agenda-event-head">
          <div class="agenda-event-title" title="${escapeHtml(ev.title || '(Sem título)')}">${escapeHtml(ev.title || '(Sem título)')}</div>
          ${isNow ? '<span class="badge badge-warn">Agora</span>' : ''}
        </div>
        ${subText ? `<div class="agenda-event-sub" title="${escapeHtml(subText)}">${escapeHtml(subText)}</div>` : ''}
      </div>
    </article>`;
}

function buildAgendaListHtml(ctx, agenda, now) {
  const { icons, escapeHtml } = ctx;
  const events = Array.isArray(agenda?.events) ? agenda.events : [];
  const configured = Boolean(agenda?.configured);

  if (!configured && events.length === 0) {
    return `
      <div class="card empty-state" style="flex:1">
        <span style="color:var(--amber);opacity:0.9">${icons.icon('agenda', 38)}</span>
        <div class="empty-state-title">Agenda não configurada</div>
        <div class="empty-state-sub">Defina <code>calendar_ics_url</code> em <code>config.json</code> no PC para sincronizar seu calendário automaticamente, ou envie eventos via integração <code>gcalendar</code>.</div>
      </div>`;
  }

  const todayKey = localDateKey(now);
  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  const tomorrowKey = localDateKey(tomorrow);
  const nowMs = now.getTime();

  const todayEvents = [];
  const upcomingMap = new Map();

  for (const ev of events) {
    if (!ev) continue;
    const dayKey = resolveDayKey(ev);
    const isToday = Boolean(ev.is_today) || dayKey === todayKey;
    if (isToday) {
      todayEvents.push(ev);
    } else {
      const groupKey = dayKey || 'outros';
      if (!upcomingMap.has(groupKey)) {
        upcomingMap.set(groupKey, []);
      }
      upcomingMap.get(groupKey).push(ev);
    }
  }

  const todayBanner = formatTodayBanner(now);
  const todaySectionHtml = `
    <section class="agenda-day-group" aria-label="Eventos de hoje">
      <div class="agenda-day-label is-today">${escapeHtml(todayBanner)}</div>
      ${
        todayEvents.length > 0
          ? todayEvents.map((ev) => renderEventCard(ev, true, nowMs, escapeHtml)).join('')
          : `<div class="agenda-empty-today">
              <span>Nenhum evento hoje</span>
              <span class="badge">Dia livre</span>
            </div>`
      }
    </section>`;

  const sortedUpcomingKeys = Array.from(upcomingMap.keys()).sort();
  let upcomingHtml = '';
  if (sortedUpcomingKeys.length > 0) {
    upcomingHtml = sortedUpcomingKeys
      .map((dayKey) => {
        const list = upcomingMap.get(dayKey) || [];
        const headerText = formatUpcomingDayHeader(dayKey, tomorrowKey);
        return `
          <section class="agenda-day-group" aria-label="${escapeHtml(headerText)}">
            <div class="agenda-day-label">${escapeHtml(headerText)}</div>
            ${list.map((ev) => renderEventCard(ev, false, nowMs, escapeHtml)).join('')}
          </section>`;
      })
      .join('');
  } else if (todayEvents.length === 0) {
    upcomingHtml = `
      <div class="agenda-empty-today" style="border-style:solid;opacity:0.75">
        <span>Nenhum compromisso nos próximos dias.</span>
      </div>`;
  }

  return `${todaySectionHtml}${upcomingHtml}`;
}

function buildTodoItemsHtml(todos, ctx) {
  const { icons, escapeHtml } = ctx;
  if (!todos.length) {
    return '<div class="todo-empty">Nenhuma tarefa cadastrada.<br>Adicione acima para acompanhar aqui.</div>';
  }

  return todos
    .map((item) => {
      const done = Boolean(item?.done);
      const id = String(item?.id ?? '');
      const text = String(item?.text ?? '');
      return `
        <div class="todo-item${done ? ' is-done' : ''}" data-todo-id="${escapeHtml(id)}">
          <button type="button" class="todo-toggle-row" data-action="todo-toggle" data-todo-id="${escapeHtml(id)}" aria-pressed="${done ? 'true' : 'false'}">
            <span class="todo-check" aria-hidden="true">${done ? icons.icon('check', 12) : ''}</span>
            <span class="todo-text">${escapeHtml(text)}</span>
          </button>
          <button type="button" class="todo-del-btn" data-action="todo-delete" data-todo-id="${escapeHtml(id)}" aria-label="Excluir tarefa" title="Excluir">
            ${icons.icon('trash', 13)}
          </button>
        </div>`;
    })
    .join('');
}

function applyTodosLocally(nextTodos) {
  if (!latestCtx || !Array.isArray(nextTodos)) return;
  if (!latestCtx.state) latestCtx.state = {};
  latestCtx.state.todos = nextTodos;
  render(latestCtx);
}

async function handleRefreshAgenda() {
  if (!latestCtx || refreshing) return;
  refreshing = true;
  refreshError = '';
  render(latestCtx);

  try {
    const snap = await latestCtx.apiRequest('POST', '/api/agenda/refresh');
    if (snap && typeof snap === 'object') {
      if (!latestCtx.state) latestCtx.state = {};
      latestCtx.state.agenda = snap;
    }
  } catch (err) {
    refreshError = (err && err.message) ? String(err.message) : 'Falha ao atualizar agenda';
  } finally {
    refreshing = false;
    if (latestCtx) render(latestCtx);
  }
}

async function handleAddTodo(formEl) {
  if (!latestCtx || submittingTodo) return;
  const input = formEl.querySelector('[data-todo-input]');
  const addBtn = formEl.querySelector('[data-todo-add]');
  const text = input ? input.value.trim() : '';
  if (!text) return;

  submittingTodo = true;
  if (addBtn) addBtn.disabled = true;

  try {
    const res = await latestCtx.apiRequest('POST', '/api/todos', { text });
    if (input) {
      input.value = '';
    }
    if (res && Array.isArray(res.todos)) {
      applyTodosLocally(res.todos);
    }
  } catch {
    // Keep user input intact on network failure
  } finally {
    submittingTodo = false;
    if (addBtn) addBtn.disabled = false;
    if (input && drawerOpen) {
      input.focus();
    }
  }
}

async function handleToggleTodo(id) {
  if (!latestCtx || !id) return;
  try {
    const res = await latestCtx.apiRequest('POST', `/api/todos/${encodeURIComponent(id)}/toggle`);
    if (res && Array.isArray(res.todos)) {
      applyTodosLocally(res.todos);
    }
  } catch {
    // Ignore transient errors; SSE resyncs state
  }
}

async function handleDeleteTodo(id) {
  if (!latestCtx || !id) return;
  try {
    const res = await latestCtx.apiRequest('DELETE', `/api/todos/${encodeURIComponent(id)}`);
    if (res && Array.isArray(res.todos)) {
      applyTodosLocally(res.todos);
    }
  } catch {
    // Ignore transient errors; SSE resyncs state
  }
}

async function handleClearDoneTodos() {
  if (!latestCtx) return;
  try {
    const res = await latestCtx.apiRequest('POST', '/api/todos/clear-done');
    if (res && Array.isArray(res.todos)) {
      applyTodosLocally(res.todos);
    }
  } catch {
    // Ignore transient errors; SSE resyncs state
  }
}

function ensureShell(mount, ctx) {
  if (mountedEl === mount && mount.querySelector('.agenda-layout')) {
    return;
  }

  mountedEl = mount;
  lastHeaderSig = '';
  lastAgendaSig = '';
  lastTodoSig = '';

  const { icons } = ctx;
  mount.innerHTML = `
    <div class="agenda-layout">
      <header class="agenda-header">
        <div class="agenda-title-group">
          <span style="color:var(--amber);display:inline-flex">${icons.icon('agenda', 19)}</span>
          <h1 class="agenda-title">Agenda</h1>
        </div>
        <div class="agenda-actions">
          <span class="agenda-sync-badge" data-agenda-sync></span>
          <button type="button" class="pill-btn" data-action="agenda-refresh">
            ${icons.icon('refresh', 14)}
            <span data-refresh-label>Atualizar</span>
          </button>
          <button type="button" class="todo-toggle-pill" data-action="todo-toggle-drawer" aria-expanded="false">
            <span data-todo-pill-label>To-Do (0/0)</span>
            <span data-todo-pill-icon style="display:inline-flex">${icons.icon('chevron_down', 14)}</span>
          </button>
        </div>
      </header>
      <div class="agenda-body">
        <div class="agenda-list scroll-y" data-agenda-list></div>
        <aside class="todo-drawer is-collapsed" data-todo-drawer aria-label="Lista de tarefas">
          <div class="todo-drawer-head">
            <span class="card-label">
              ${icons.icon('check', 13)}
              <span>To-Do</span>
              <span class="badge num" data-todo-open-badge>0</span>
            </span>
            <button type="button" class="todo-clear-btn" data-action="todo-clear-done" disabled>Limpar feitos</button>
          </div>
          <form class="todo-form" data-todo-form autocomplete="off">
            <label class="sr-only" for="agendaTodoInput">Nova tarefa</label>
            <input id="agendaTodoInput" class="todo-input" data-todo-input type="text" maxlength="240" placeholder="Nova tarefa..." autocomplete="off">
            <button type="submit" class="todo-add-btn" data-todo-add aria-label="Adicionar tarefa" title="Adicionar">
              ${icons.icon('plus', 16)}
            </button>
          </form>
          <div class="todo-list scroll-y" data-todo-list></div>
        </aside>
      </div>
    </div>`;

  mount.addEventListener('click', (ev) => {
    const refreshBtn = ev.target.closest('[data-action="agenda-refresh"]');
    if (refreshBtn) {
      handleRefreshAgenda();
      return;
    }

    const drawerBtn = ev.target.closest('[data-action="todo-toggle-drawer"]');
    if (drawerBtn) {
      drawerOpen = !drawerOpen;
      if (latestCtx) render(latestCtx);
      return;
    }

    const clearBtn = ev.target.closest('[data-action="todo-clear-done"]');
    if (clearBtn) {
      handleClearDoneTodos();
      return;
    }

    const delBtn = ev.target.closest('[data-action="todo-delete"]');
    if (delBtn) {
      handleDeleteTodo(delBtn.dataset.todoId);
      return;
    }

    const toggleBtn = ev.target.closest('[data-action="todo-toggle"]');
    if (toggleBtn) {
      handleToggleTodo(toggleBtn.dataset.todoId);
    }
  });

  const formEl = mount.querySelector('[data-todo-form]');
  if (formEl) {
    formEl.addEventListener('submit', (ev) => {
      ev.preventDefault();
      handleAddTodo(formEl);
    });
  }
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;
  latestCtx = ctx;

  ensureShell(mount, ctx);

  const agenda = ctx.state?.agenda || null;
  const todos = Array.isArray(ctx.state?.todos) ? ctx.state.todos : [];
  const todoTotal = todos.length;
  const todoOpenCount = todos.filter((t) => t && !t.done).length;
  const todoDoneCount = todoTotal - todoOpenCount;

  const lastSyncMs = Number(agenda?.last_sync_ms || 0);
  if (lastSyncMs > lastSeenSyncMs) {
    lastSeenSyncMs = lastSyncMs;
    if (!agenda?.error) {
      refreshError = '';
    }
  }
  const effectiveError = String(refreshError || agenda?.error || '').trim();
  const syncClock = lastSyncMs > 0 ? ctx.formatClock(new Date(lastSyncMs)) : '';

  // 1. Update header controls only when changed
  const headerSig = JSON.stringify([
    effectiveError,
    syncClock,
    Boolean(agenda?.configured),
    refreshing,
    drawerOpen,
    todoOpenCount,
    todoTotal,
  ]);
  if (headerSig !== lastHeaderSig) {
    lastHeaderSig = headerSig;

    const syncEl = mount.querySelector('[data-agenda-sync]');
    if (syncEl) {
      if (effectiveError) {
        syncEl.className = 'badge badge-err agenda-sync-badge';
        syncEl.title = effectiveError;
        syncEl.textContent = effectiveError;
      } else if (syncClock) {
        syncEl.className = 'badge badge-ok num agenda-sync-badge';
        syncEl.title = `Última sincronização às ${syncClock}`;
        syncEl.textContent = `Sync ${syncClock}`;
      } else if (agenda?.configured) {
        syncEl.className = 'badge agenda-sync-badge';
        syncEl.title = 'Agenda conectada';
        syncEl.textContent = 'Conectada';
      } else {
        syncEl.className = 'hidden';
        syncEl.title = '';
        syncEl.textContent = '';
      }
    }

    const refreshBtn = mount.querySelector('[data-action="agenda-refresh"]');
    const refreshLbl = mount.querySelector('[data-refresh-label]');
    if (refreshBtn) {
      refreshBtn.disabled = refreshing;
    }
    if (refreshLbl) {
      refreshLbl.textContent = refreshing ? 'Atualizando…' : 'Atualizar';
    }

    const pillBtn = mount.querySelector('[data-action="todo-toggle-drawer"]');
    const pillLbl = mount.querySelector('[data-todo-pill-label]');
    const pillIcon = mount.querySelector('[data-todo-pill-icon]');
    if (pillBtn) {
      pillBtn.classList.toggle('is-open', drawerOpen);
      pillBtn.setAttribute('aria-expanded', drawerOpen ? 'true' : 'false');
    }
    if (pillLbl) {
      pillLbl.textContent = `To-Do (${todoOpenCount}/${todoTotal})`;
    }
    if (pillIcon) {
      pillIcon.innerHTML = ctx.icons.icon(drawerOpen ? 'chevron_up' : 'chevron_down', 14);
    }
  }

  // 2. Update main Agenda timeline only when events or current minute changes
  const now = new Date();
  const minuteKey = `${localDateKey(now)} ${formatTimeHHMM(now)}`;
  const events = Array.isArray(agenda?.events) ? agenda.events : [];
  const agendaSig = JSON.stringify([
    Boolean(agenda?.configured),
    minuteKey,
    events,
  ]);
  if (agendaSig !== lastAgendaSig) {
    lastAgendaSig = agendaSig;
    const listEl = mount.querySelector('[data-agenda-list]');
    if (listEl) {
      const prevScroll = listEl.scrollTop;
      listEl.innerHTML = buildAgendaListHtml(ctx, agenda, now);
      listEl.scrollTop = prevScroll;
    }
  }

  // 3. Update To-Do drawer visibility & list WITHOUT touching the quick-add <input>
  const drawerEl = mount.querySelector('[data-todo-drawer]');
  if (drawerEl) {
    drawerEl.classList.toggle('is-collapsed', !drawerOpen);
  }

  const todoSig = JSON.stringify([drawerOpen, todoOpenCount, todoDoneCount, todos]);
  if (todoSig !== lastTodoSig) {
    lastTodoSig = todoSig;

    const openBadge = mount.querySelector('[data-todo-open-badge]');
    if (openBadge) {
      openBadge.textContent = `${todoOpenCount}/${todoTotal}`;
    }

    const clearBtn = mount.querySelector('[data-action="todo-clear-done"]');
    if (clearBtn) {
      clearBtn.disabled = todoDoneCount === 0;
    }

    const todoListEl = mount.querySelector('[data-todo-list]');
    if (todoListEl) {
      const prevScroll = todoListEl.scrollTop;
      todoListEl.innerHTML = buildTodoItemsHtml(todos, ctx);
      todoListEl.scrollTop = prevScroll;
    }
  }
}
