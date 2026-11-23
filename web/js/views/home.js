const DEFAULT_WIDGETS = ['ai', 'weather', 'agenda', 'media', 'github'];
const SUPPORTED_WIDGETS = new Set(['ai', 'weather', 'agenda', 'media', 'github', 'pc']);

let mountedEl = null;
let lastClockSig = '';
let lastLayoutSig = '';
let cardSigs = new Map();
let mediaArtworkKey = '';
let latestCtx = null;

function formatRate(bps, formatBytes) {
  const n = Number(bps);
  if (!Number.isFinite(n) || n <= 0) return '0 B/s';
  return `${formatBytes(n)}/s`;
}

function shortCpuName(raw) {
  const clean = String(raw || '')
    .replace(/\(R\)|\(TM\)|CPU|Processor|@.*$/gi, '')
    .replace(/\s+/g, ' ')
    .trim();
  return clean.length > 26 ? `${clean.slice(0, 25)}…` : clean;
}

function isDateOnlyIso(iso) {
  return typeof iso === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(iso.trim());
}

function parseLocalDateParts(iso) {
  if (!iso || typeof iso !== 'string') return null;
  const m = iso.trim().match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (!m) return null;
  return { y: Number(m[1]), mo: Number(m[2]), d: Number(m[3]) };
}

function parseEventDate(iso, endOfDay = false) {
  if (!iso || typeof iso !== 'string') return null;
  const trimmed = iso.trim();
  if (/^\d{4}-\d{2}-\d{2}$/.test(trimmed)) {
    const [y, m, d] = trimmed.split('-').map(Number);
    return endOfDay
      ? new Date(y, m - 1, d, 23, 59, 59, 999)
      : new Date(y, m - 1, d, 0, 0, 0, 0);
  }
  const dt = new Date(trimmed);
  return Number.isNaN(dt.getTime()) ? null : dt;
}

function safeArtworkUrl(raw) {
  return typeof raw === 'string' && raw.startsWith('data:image/') ? raw : '';
}

function formatTimeShort(date) {
  if (!date) return '';
  const hh = String(date.getHours()).padStart(2, '0');
  const mm = String(date.getMinutes()).padStart(2, '0');
  return `${hh}:${mm}`;
}

function isSameDay(a, b) {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function isTomorrow(date, now) {
  const tom = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return isSameDay(date, tom);
}

const SHORT_DAYS_PT = ['Dom', 'Seg', 'Ter', 'Qua', 'Qui', 'Sex', 'Sáb'];

function pickNextAgendaEvent(events, now) {
  if (!Array.isArray(events) || !events.length) return null;
  const nowMs = now.getTime();
  const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, 0, 0, 0).getTime();
  const todayEnd = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 23, 59, 59, 999).getTime();

  let best = null;
  for (const ev of events) {
    if (!ev || !ev.start_iso) continue;
    const allDay = Boolean(ev.all_day) || isDateOnlyIso(ev.start_iso);

    if (allDay) {
      const startParts = parseLocalDateParts(ev.day_key || ev.start_iso);
      const start = startParts
        ? new Date(startParts.y, startParts.mo - 1, startParts.d, 0, 0, 0, 0)
        : parseEventDate(ev.start_iso, false);
      if (!start) continue;

      const endParts = ev.end_iso ? parseLocalDateParts(ev.end_iso) : null;
      const end = endParts
        ? new Date(endParts.y, endParts.mo - 1, endParts.d, 0, 0, 0, 0)
        : null;
      const endMs = end && end.getTime() > start.getTime()
        ? end.getTime()
        : new Date(start.getFullYear(), start.getMonth(), start.getDate(), 23, 59, 59, 999).getTime();

      if (!ev.is_today && endMs < todayStart) continue;
      const isNow = Boolean(ev.is_today) || (start.getTime() <= todayEnd && endMs >= todayStart);
      best = { ev, start, end, isNow, allDay: true };
      break;
    }

    const start = parseEventDate(ev.start_iso, false);
    if (!start) continue;
    const end = ev.end_iso ? parseEventDate(ev.end_iso, false) : null;
    const effectiveEndMs = end && end.getTime() > start.getTime()
      ? end.getTime()
      : start.getTime() + 3_600_000;
    if (effectiveEndMs <= nowMs) continue;
    const isNow = start.getTime() <= nowMs && effectiveEndMs > nowMs;
    best = { ev, start, end, isNow, allDay: false };
    break;
  }
  return best;
}

function formatAgendaWhen(picked, now) {
  const { ev, start, end, allDay } = picked;
  let dayLabel = '';
  if (ev.is_today || isSameDay(start, now)) {
    dayLabel = 'Hoje';
  } else if (isTomorrow(start, now)) {
    dayLabel = 'Amanhã';
  } else {
    dayLabel = `${SHORT_DAYS_PT[start.getDay()]} ${String(start.getDate()).padStart(2, '0')}/${String(start.getMonth() + 1).padStart(2, '0')}`;
  }

  if (allDay || ev.all_day) {
    return `${dayLabel} · Dia todo`;
  }
  const startStr = formatTimeShort(start);
  if (end && isSameDay(start, end)) {
    return `${dayLabel} · ${startStr}–${formatTimeShort(end)}`;
  }
  return `${dayLabel} · ${startStr}`;
}

function resolveWidgetList(state) {
  const raw = Array.isArray(state?.home_widgets) && state.home_widgets.length
    ? state.home_widgets
    : DEFAULT_WIDGETS;
  const list = [];
  for (const w of raw) {
    const key = String(w || '').trim().toLowerCase();
    if (SUPPORTED_WIDGETS.has(key) && !list.includes(key)) {
      list.push(key);
    }
  }
  return list.length ? list : DEFAULT_WIDGETS;
}

function getAiCardData(ctx) {
  const { state, icons, escapeHtml, formatCompactNumber } = ctx;
  const ai = state?.ai || null;
  const w5 = ai?.window_5h || null;
  const wk = ai?.weekly || null;
  const today = ai?.today || null;
  const sessions = Array.isArray(ai?.sessions)
    ? ai.sessions
    : (Array.isArray(state?.antigravity?.sessions) ? state.antigravity.sessions : []);
  const activeCount = sessions.filter((s) => s && (s.status === 'running' || s.status === 'waiting')).length;
  const approvalCount = Array.isArray(state?.approvals) ? state.approvals.length : 0;

  const providerLabel = ai?.provider_label || 'IA & Agentes';
  const w5Pct = w5 && w5.pct !== null && w5.pct !== undefined && Number.isFinite(Number(w5.pct))
    ? Math.max(0, Math.min(100, Number(w5.pct)))
    : null;
  const wkPct = wk && wk.pct !== null && wk.pct !== undefined && Number.isFinite(Number(wk.pct))
    ? Math.max(0, Math.min(100, Number(wk.pct)))
    : null;
  const w5Tokens = Number(w5?.tokens || 0);
  const wkTokens = Number(wk?.tokens || 0);
  const todayTokens = Number(today?.total_tokens || 0);
  const todayMsgs = Number(today?.messages || 0);
  const w5Reset = w5?.reset_label || '';
  const wkReset = wk?.reset_label || '';

  const sig = [
    providerLabel,
    activeCount,
    approvalCount,
    w5Pct !== null ? w5Pct.toFixed(1) : 'n',
    wkPct !== null ? wkPct.toFixed(1) : 'n',
    w5Tokens,
    wkTokens,
    todayTokens,
    todayMsgs,
    w5Reset,
    wkReset,
  ].join('|');

  let badgeHtml = '';
  if (approvalCount > 0) {
    badgeHtml = `<span class="badge badge-err num">${approvalCount} aprovação</span>`;
  } else if (activeCount > 0) {
    badgeHtml = `<span class="badge badge-ai num">${activeCount} ativa${activeCount > 1 ? 's' : ''}</span>`;
  } else {
    badgeHtml = `<span class="badge num">Hoje: ${escapeHtml(formatCompactNumber(todayTokens))}</span>`;
  }

  const renderMetricRow = (labelText, pct, tokens, resetLabel, fallbackRight) => {
    if (pct !== null) {
      const pctRounded = Math.round(pct);
      const valText = resetLabel ? `${pctRounded}% · ${escapeHtml(resetLabel)}` : `${pctRounded}%`;
      return `
        <div class="bento-bar-row">
          <span class="bento-bar-label">${escapeHtml(labelText)}</span>
          <div class="ai-bar-track" style="flex:1">
            <div class="ai-bar-fill" style="width:${pct.toFixed(1)}%"></div>
          </div>
          <span class="bento-bar-val num">${valText}</span>
        </div>`;
    }
    return `
      <div class="bento-bar-row" style="justify-content:space-between">
        <span class="bento-bar-label" style="width:auto">${escapeHtml(labelText)}</span>
        <span class="num" style="color:var(--text);font-weight:700">${escapeHtml(formatCompactNumber(tokens))} <span style="color:var(--muted);font-weight:500">tok</span></span>
        ${fallbackRight ? `<span class="num" style="color:var(--muted);font-size:11px">${escapeHtml(fallbackRight)}</span>` : ''}
      </div>`;
  };

  const right5h = `Hoje: ${formatCompactNumber(todayTokens)}`;
  const rightWk = todayMsgs > 0 ? `${formatCompactNumber(todayMsgs)} msgs` : '';

  const html = `
    <div class="card-head">
      <span class="card-label" style="color:var(--ai)">${icons.icon('ai', 14)}<span>${escapeHtml(providerLabel)}</span></span>
      ${badgeHtml}
    </div>
    <div class="bento-ai-bars">
      ${renderMetricRow('JANELA 5H', w5Pct, w5Tokens, w5Reset, right5h)}
      ${renderMetricRow('SEMANAL', wkPct, wkTokens, wkReset, rightWk)}
    </div>`;

  return { sig, html, targetTab: 'ai', extraClass: 'bento-ai' };
}

function getWeatherCardData(ctx) {
  const { state, icons, escapeHtml } = ctx;
  const w = state?.weather || null;
  const configured = Boolean(w?.configured);
  const city = w?.city || 'Clima';
  const temp = w?.temp !== null && w?.temp !== undefined && Number.isFinite(Number(w.temp))
    ? Math.round(Number(w.temp))
    : null;
  const hi = w?.today_high !== null && w?.today_high !== undefined && Number.isFinite(Number(w.today_high))
    ? Math.round(Number(w.today_high))
    : null;
  const lo = w?.today_low !== null && w?.today_low !== undefined && Number.isFinite(Number(w.today_low))
    ? Math.round(Number(w.today_low))
    : null;
  const code = w?.weather_code ?? null;
  const cond = w?.condition || '';
  const err = w?.error || '';
  const unit = (w?.unit || 'c').toLowerCase().startsWith('f') ? '°F' : '°C';

  const sig = [configured ? 1 : 0, city, temp, hi, lo, code, cond, err, unit].join('|');

  let bodyHtml = '';
  if (!configured) {
    bodyHtml = `
      <div class="bento-weather-main">
        <div class="bento-weather-left">
          ${icons.weatherIcon(1, 38)}
          <div class="bento-agenda-info">
            <div class="bento-agenda-title">Configurar clima</div>
            <div class="bento-agenda-sub">Defina weather.city em config.json</div>
          </div>
        </div>
      </div>`;
  } else if (temp === null) {
    bodyHtml = `
      <div class="bento-weather-main">
        <div class="bento-weather-left">
          ${icons.weatherIcon(code ?? 3, 38)}
          <div class="bento-agenda-info">
            <div class="bento-agenda-title">${escapeHtml(err ? 'Erro ao atualizar' : 'Carregando…')}</div>
            <div class="bento-agenda-sub">${escapeHtml(err || 'Buscando previsão atual')}</div>
          </div>
        </div>
      </div>`;
  } else {
    const hiLoText = hi !== null && lo !== null
      ? `Máx ${hi}°  Mín ${lo}°`
      : (w?.feels_like != null ? `Sensação ${Math.round(Number(w.feels_like))}°` : '');
    bodyHtml = `
      <div class="bento-weather-main">
        <div class="bento-weather-left">
          ${icons.weatherIcon(code ?? 0, 42)}
          <div class="bento-weather-temp num">${temp}°</div>
        </div>
        <div class="bento-weather-meta">
          <div class="bento-weather-cond" title="${escapeHtml(cond)}">${escapeHtml(cond || '—')}</div>
          ${hiLoText ? `<div class="bento-weather-hi-lo num">${escapeHtml(hiLoText)}</div>` : ''}
        </div>
      </div>`;
  }

  const html = `
    <div class="card-head">
      <span class="card-label" style="color:var(--cpu)">${icons.icon('weather', 14)}<span>${escapeHtml(city)}</span></span>
      ${configured && temp !== null ? `<span class="badge num">${escapeHtml(unit)}</span>` : ''}
    </div>
    ${bodyHtml}`;

  return { sig, html, targetTab: 'weather', extraClass: 'bento-weather' };
}

function getAgendaCardData(ctx, now) {
  const { state, icons, escapeHtml } = ctx;
  const agenda = state?.agenda || null;
  const events = Array.isArray(agenda?.events) ? agenda.events : [];
  const picked = pickNextAgendaEvent(events, now);
  const todayCount = events.filter((e) => e && e.is_today).length;

  const title = picked ? picked.ev.title : 'Livre hoje';
  const when = picked ? formatAgendaWhen(picked, now) : '';
  const loc = picked?.ev?.location ? picked.ev.location : '';
  const sub = picked
    ? (loc ? `${when} · ${loc}` : when)
    : (agenda?.configured ? 'Nenhum compromisso próximo' : 'Conecte sua agenda para ver eventos');
  const isNow = Boolean(picked?.isNow);

  const sig = [
    Boolean(agenda?.configured) ? 1 : 0,
    todayCount,
    picked?.ev?.id || '',
    title,
    sub,
    isNow ? 1 : 0,
  ].join('|');

  let badgeHtml = '';
  if (isNow) {
    badgeHtml = '<span class="badge badge-accent">Agora</span>';
  } else if (todayCount > 0) {
    badgeHtml = `<span class="badge num">${todayCount} hoje</span>`;
  } else if (events.length > 0) {
    badgeHtml = `<span class="badge num">${events.length} próx.</span>`;
  }

  const html = `
    <div class="card-head">
      <span class="card-label" style="color:var(--ram)">${icons.icon('agenda', 14)}<span>A seguir</span></span>
      ${badgeHtml}
    </div>
    <div class="bento-agenda-row">
      <div class="event-bar" style="background:${isNow ? 'var(--accent)' : 'var(--ram)'}"></div>
      <div class="bento-agenda-info">
        <div class="bento-agenda-title" title="${escapeHtml(title)}">${escapeHtml(title)}</div>
        <div class="bento-agenda-sub" title="${escapeHtml(sub)}">${escapeHtml(sub)}</div>
      </div>
    </div>`;

  return { sig, html, targetTab: 'agenda', extraClass: 'bento-agenda' };
}

function getMediaCardData(ctx) {
  const { state, icons, escapeHtml } = ctx;
  const media = state?.media || null;
  const playing = Boolean(media?.playing);
  const title = media?.title || 'Nada tocando';
  const artist = media?.artist || (media?.app ? media.app : 'Toque para abrir controles');
  const app = media?.app || 'Mídia';
  const art = safeArtworkUrl(media?.artwork_data_url);
  const artKey = art ? `${art.length}:${art.slice(-24)}` : '';
  const statusLabel = playing ? 'Tocando' : (media?.status === 'Paused' ? 'Pausado' : '');

  const sig = [playing ? 1 : 0, title, artist, app, statusLabel, artKey].join('|');

  const html = `
    <div class="card-head">
      <span class="card-label" style="color:var(--gpu)">${icons.icon('media', 14)}<span>${escapeHtml(app)}</span></span>
      ${statusLabel ? `<span class="badge ${playing ? 'badge-ok' : ''}">${escapeHtml(statusLabel)}</span>` : ''}
    </div>
    <div class="bento-media-row">
      <div class="bento-media-thumb" data-home-media-thumb>
        ${art ? '' : icons.icon('music', 20)}
      </div>
      <div class="bento-media-meta">
        <div class="bento-media-title" title="${escapeHtml(title)}">${escapeHtml(title)}</div>
        <div class="bento-media-artist" title="${escapeHtml(artist)}">${escapeHtml(artist)}</div>
      </div>
      <button type="button" class="mini-play-btn" data-action="media-play-pause" aria-label="${playing ? 'Pausar mídia' : 'Reproduzir mídia'}">
        ${icons.icon(playing ? 'pause' : 'play', 18)}
      </button>
    </div>`;

  return { sig, html, targetTab: 'media', extraClass: 'bento-media', art, artKey };
}

function getPcStripCardData(ctx) {
  const { state, icons, escapeHtml, formatBytes } = ctx;
  const pc = state?.pc || null;

  const cpuPct = pc && Number.isFinite(Number(pc.cpu_pct)) ? Math.round(Number(pc.cpu_pct)) : null;
  const gpuPct = pc?.gpu && pc.gpu.util_pct !== null && pc.gpu.util_pct !== undefined
    ? Math.round(Number(pc.gpu.util_pct))
    : null;
  const gpuTemp = pc?.gpu && pc.gpu.temp_c !== null && pc.gpu.temp_c !== undefined
    ? Math.round(Number(pc.gpu.temp_c))
    : null;
  const ramPct = pc && pc.ram_total_gb > 0
    ? Math.round((Number(pc.ram_used_gb) / Number(pc.ram_total_gb)) * 100)
    : null;
  const ramSub = pc && pc.ram_total_gb > 0
    ? `${Number(pc.ram_used_gb).toFixed(1)}/${Math.round(Number(pc.ram_total_gb))} GB`
    : '—';
  const downStr = pc ? formatRate(pc.net_down_bps, formatBytes) : '0 B/s';
  const upStr = pc ? formatRate(pc.net_up_bps, formatBytes) : '0 B/s';
  const cpuLabel = pc?.cpu_name ? shortCpuName(pc.cpu_name) : 'Ao vivo';

  const sig = ['pcstrip', cpuPct, gpuPct, gpuTemp, ramPct, ramSub, downStr, upStr, cpuLabel].join('|');

  const metricBox = (label, val, sub, color) => `
    <div style="display:flex;flex-direction:column;gap:2px;min-width:0;padding:4px 8px;border-radius:var(--radius-sm);background:var(--surface-2);border:1px solid var(--border)">
      <span style="font-size:10px;font-weight:700;letter-spacing:0.06em;color:var(--muted);text-transform:uppercase">${escapeHtml(label)}</span>
      <span class="num" style="font-size:17px;font-weight:700;line-height:1.1;color:${color}">${escapeHtml(val)}</span>
      <span class="num" style="font-size:10px;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${escapeHtml(sub)}</span>
    </div>`;

  const html = `
    <div class="card-head">
      <span class="card-label" style="color:var(--cpu)">${icons.icon('pc', 14)}<span>Telemetria do PC</span></span>
      <span class="badge num">${escapeHtml(cpuLabel)}</span>
    </div>
    <div class="bento-pc-strip">
      ${metricBox('CPU', cpuPct !== null ? `${cpuPct}%` : '--', 'Uso global', 'var(--cpu)')}
      ${metricBox('GPU', gpuPct !== null ? `${gpuPct}%` : 'N/A', gpuTemp !== null ? `${gpuTemp}°C` : (pc?.gpu ? 'Ativa' : 'Indisponível'), 'var(--gpu)')}
      ${metricBox('RAM', ramPct !== null ? `${ramPct}%` : '--', ramSub, 'var(--ram)')}
      ${metricBox('Rede', `↓ ${downStr}`, `↑ ${upStr}`, 'var(--green)')}
    </div>`;

  return {
    sig,
    html,
    targetTab: 'pc',
    extraClass: 'bento-github',
    borderColor: 'rgba(96, 165, 250, 0.55)',
  };
}

function getGithubCardData(ctx) {
  const { state, icons, charts, escapeHtml, formatCompactNumber } = ctx;
  const gh = state?.github || null;
  if (!gh || !gh.configured) {
    return getPcStripCardData(ctx);
  }

  const weeks = Array.isArray(gh.weeks) ? gh.weeks : [];
  const total = Number(gh.total_contributions || 0);
  const streak = Number(gh.current_streak || 0);
  const username = gh.username || '';
  const updated = gh.updated_at_ms || 0;
  const err = gh.error || '';

  const sig = ['gh', username, total, streak, weeks.length, updated, err].join('|');

  let bodyHtml = '';
  if (err && !weeks.length) {
    bodyHtml = `
      <div class="bento-github-row">
        <div class="bento-agenda-sub">${escapeHtml(err)}</div>
      </div>`;
  } else {
    bodyHtml = `
      <div class="bento-github-row">
        <div class="bento-github-map">
          ${charts.renderGithubHeatmap({ weeks, compact: true })}
        </div>
        <div class="bento-github-stats">
          <div class="num" style="font-size:20px;font-weight:700;line-height:1.05;color:var(--text)">${escapeHtml(formatCompactNumber(total))}</div>
          <div style="font-size:10px;color:var(--muted)">contribuições</div>
          <div class="num" style="font-size:11px;color:var(--gh-4);font-weight:600;margin-top:2px">${streak} ${streak === 1 ? 'dia' : 'dias'} streak</div>
        </div>
      </div>`;
  }

  const html = `
    <div class="card-head">
      <span class="card-label" style="color:var(--green)">${icons.icon('github', 14)}<span>GitHub${username ? ` · @${escapeHtml(username)}` : ''}</span></span>
      <span class="badge badge-ok num">${streak} ${streak === 1 ? 'dia' : 'dias'} streak</span>
    </div>
    ${bodyHtml}`;

  return {
    sig,
    html,
    targetTab: 'github',
    extraClass: 'bento-github',
    borderColor: '',
  };
}

function buildWidgetData(key, ctx, now) {
  switch (key) {
    case 'ai':
      return getAiCardData(ctx);
    case 'weather':
      return getWeatherCardData(ctx);
    case 'agenda':
      return getAgendaCardData(ctx, now);
    case 'media':
      return getMediaCardData(ctx);
    case 'github':
      return getGithubCardData(ctx);
    case 'pc':
      return getPcStripCardData(ctx);
    default:
      return null;
  }
}

function ensureShell(mount) {
  if (mountedEl === mount && mount.querySelector('.home-layout')) {
    return;
  }
  mountedEl = mount;
  lastClockSig = '';
  lastLayoutSig = '';
  cardSigs = new Map();
  mediaArtworkKey = '';

  mount.innerHTML = `
    <div class="home-layout">
      <header class="home-header">
        <div class="home-clock num" data-home-clock>00:00</div>
        <div class="home-date" data-home-date></div>
      </header>
      <div class="bento-grid" data-home-grid></div>
    </div>`;

  const grid = mount.querySelector('[data-home-grid]');
  if (grid) {
    grid.addEventListener('click', (ev) => {
      const playBtn = ev.target.closest('[data-action="media-play-pause"]');
      if (playBtn) {
        ev.stopPropagation();
        if (!latestCtx) return;
        playBtn.disabled = true;
        latestCtx
          .apiRequest('POST', '/api/media/play-pause')
          .catch(() => {})
          .finally(() => {
            playBtn.disabled = false;
          });
        return;
      }

      const card = ev.target.closest('.bento-card[data-target-tab]');
      if (card && latestCtx) {
        const target = card.dataset.targetTab;
        if (target) latestCtx.setTab(target);
      }
    });

    grid.addEventListener('keydown', (ev) => {
      if (ev.key !== 'Enter' && ev.key !== ' ') return;
      if (ev.target.closest('button')) return;
      const card = ev.target.closest('.bento-card[data-target-tab]');
      if (card && latestCtx) {
        ev.preventDefault();
        const target = card.dataset.targetTab;
        if (target) latestCtx.setTab(target);
      }
    });
  }
}

function syncMediaArtwork(cardEl, art, artKey, icons) {
  const thumb = cardEl.querySelector('[data-home-media-thumb]');
  if (!thumb) return;
  const validArt = safeArtworkUrl(art);
  if (!validArt) {
    if (thumb.querySelector('img')) {
      thumb.innerHTML = icons.icon('music', 20);
    }
    mediaArtworkKey = '';
    return;
  }
  let img = thumb.querySelector('img');
  if (!img) {
    thumb.innerHTML = '';
    img = document.createElement('img');
    img.alt = '';
    img.decoding = 'async';
    thumb.appendChild(img);
    img.src = validArt;
    mediaArtworkKey = artKey;
  } else if (mediaArtworkKey !== artKey) {
    img.src = validArt;
    mediaArtworkKey = artKey;
  }
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;
  latestCtx = ctx;

  ensureShell(mount);

  const now = new Date();
  const clockText = ctx.formatClock(now);
  const dateText = ctx.formatFullDatePt(now);
  const clockSig = `${clockText}|${dateText}`;
  if (clockSig !== lastClockSig) {
    lastClockSig = clockSig;
    const clockEl = mount.querySelector('[data-home-clock]');
    const dateEl = mount.querySelector('[data-home-date]');
    if (clockEl) clockEl.textContent = clockText;
    if (dateEl) dateEl.textContent = dateText;
  }

  const grid = mount.querySelector('[data-home-grid]');
  if (!grid) return;

  const widgets = resolveWidgetList(ctx.state);
  const layoutSig = widgets.join(',');
  if (layoutSig !== lastLayoutSig) {
    lastLayoutSig = layoutSig;
    cardSigs.clear();
    mediaArtworkKey = '';
    grid.innerHTML = widgets
      .map(
        (key) =>
          `<div class="bento-card" role="button" tabindex="0" data-widget-slot="${key}"></div>`
      )
      .join('');
  }

  for (const key of widgets) {
    const cardEl = grid.querySelector(`[data-widget-slot="${key}"]`);
    if (!cardEl) continue;
    const data = buildWidgetData(key, ctx, now);
    if (!data) continue;

    if (cardSigs.get(key) === data.sig) {
      continue;
    }

    cardEl.className = `bento-card ${data.extraClass}`;
    cardEl.dataset.targetTab = data.targetTab;
    cardEl.style.borderTopColor = data.borderColor || '';

    if (key === 'media') {
      const existingImg = cardEl.querySelector('[data-home-media-thumb] img');
      const preserveImg = existingImg && data.art && mediaArtworkKey === data.artKey;
      cardEl.innerHTML = data.html;
      if (preserveImg) {
        const nextThumb = cardEl.querySelector('[data-home-media-thumb]');
        if (nextThumb) {
          nextThumb.innerHTML = '';
          nextThumb.appendChild(existingImg);
        }
      } else {
        syncMediaArtwork(cardEl, data.art, data.artKey, ctx.icons);
      }
    } else {
      cardEl.innerHTML = data.html;
    }

    cardSigs.set(key, data.sig);
  }
}
