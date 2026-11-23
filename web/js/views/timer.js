const PRESET_MINUTES = [5, 15, 25, 45];

let presetMinutes = 25;
let totalSecs = presetMinutes * 60;
let remainingSecs = totalSecs;
let status = 'idle'; // 'idle' | 'running' | 'paused' | 'done'
let endAtMs = 0;
let tickInterval = null;

let mountedEl = null;
let latestCtx = null;
let lastSig = '';

function formatMMSS(secs) {
  const clamped = Math.max(0, Math.floor(Number(secs) || 0));
  const m = Math.floor(clamped / 60);
  const s = clamped % 60;
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
}

function formatBadgeText(secs) {
  const clamped = Math.max(0, Math.floor(Number(secs) || 0));
  if (clamped <= 0) return null;
  if (clamped >= 60) {
    return `${Math.floor(clamped / 60)}m`;
  }
  return `0:${String(clamped).padStart(2, '0')}`;
}

function syncRailBadge() {
  if (!latestCtx || typeof latestCtx.setTimerBadge !== 'function') return;
  if (status === 'running' && remainingSecs > 0) {
    latestCtx.setTimerBadge(formatBadgeText(remainingSecs));
  } else {
    latestCtx.setTimerBadge(null);
  }
}

function stopTicker() {
  if (tickInterval !== null) {
    clearInterval(tickInterval);
    tickInterval = null;
  }
}

function finishTimer() {
  stopTicker();
  remainingSecs = 0;
  endAtMs = 0;
  status = 'done';
  syncRailBadge();

  if (latestCtx && typeof latestCtx.alertUser === 'function') {
    latestCtx.alertUser(false);
  }

  paint(true);
}

function onTick() {
  if (status !== 'running') {
    stopTicker();
    return;
  }

  const nextRemaining = Math.max(0, Math.ceil((endAtMs - Date.now()) / 1000));
  if (nextRemaining <= 0) {
    finishTimer();
    return;
  }

  if (nextRemaining !== remainingSecs) {
    remainingSecs = nextRemaining;
    syncRailBadge();
    if (mountedEl && mountedEl.classList.contains('active')) {
      paint(false);
    }
  }
}

function startCountdown() {
  if (remainingSecs <= 0) {
    remainingSecs = totalSecs;
  }
  status = 'running';
  endAtMs = Date.now() + remainingSecs * 1000;
  stopTicker();
  tickInterval = setInterval(onTick, 250);
  syncRailBadge();
  paint(true);
}

function pauseCountdown() {
  if (status !== 'running') return;
  remainingSecs = Math.max(1, Math.ceil((endAtMs - Date.now()) / 1000));
  stopTicker();
  endAtMs = 0;
  status = 'paused';
  syncRailBadge();
  paint(true);
}

function resetCountdown() {
  stopTicker();
  remainingSecs = totalSecs;
  endAtMs = 0;
  status = 'idle';
  syncRailBadge();
  paint(true);
}

function restartCountdown() {
  stopTicker();
  totalSecs = presetMinutes * 60;
  remainingSecs = totalSecs;
  startCountdown();
}

function selectPreset(minutes) {
  const mins = Number(minutes);
  if (!Number.isFinite(mins) || mins <= 0) return;
  stopTicker();
  presetMinutes = mins;
  totalSecs = mins * 60;
  remainingSecs = totalSecs;
  endAtMs = 0;
  status = 'idle';
  syncRailBadge();
  paint(true);
}

function buildDoneHtml(ctx) {
  const { icons, escapeHtml } = ctx;
  return `
    <div class="timer-done-card" role="alert">
      <div class="timer-done-icon">${icons.icon('bell', 28)}</div>
      <h2 class="timer-done-title">Tempo esgotado!</h2>
      <div class="timer-done-sub">Sessão de ${escapeHtml(presetMinutes)} min concluída.</div>
      <div class="timer-done-actions">
        <button type="button" class="timer-reset-btn" data-action="timer-dismiss" style="flex:1">
          ${icons.icon('check', 18)}
          <span>Dispensar</span>
        </button>
        <button type="button" class="timer-hero-btn" data-action="timer-restart" style="flex:1">
          ${icons.icon('rotate_ccw', 18)}
          <span>Reiniciar</span>
        </button>
      </div>
    </div>`;
}

function buildActiveHtml(ctx) {
  const { icons, charts } = ctx;
  const running = status === 'running';
  const paused = status === 'paused';
  const progress = totalSecs > 0 ? Math.max(0, Math.min(1, remainingSecs / totalSecs)) : 0;
  const subLabel = running ? 'Em andamento' : paused ? 'Pausado' : 'Pronto';

  const ringSvg = charts.renderTimerRing({
    progress,
    label: formatMMSS(remainingSecs),
    sub: subLabel,
    active: running,
  });

  const presetsHtml = PRESET_MINUTES.map((m) => {
    const isActive = presetMinutes === m;
    return `
      <button type="button" class="timer-preset-btn num${isActive ? ' is-active' : ''}" data-action="timer-preset" data-minutes="${m}" aria-pressed="${isActive ? 'true' : 'false'}">
        ${m}m
      </button>`;
  }).join('');

  const primaryLabel = running ? 'Pausar' : paused ? 'Retomar' : 'Iniciar';
  const primaryIcon = running ? 'pause' : 'play';

  return `
    <div class="timer-layout">
      <div class="timer-ring-col">
        <div class="timer-ring-box">${ringSvg}</div>
      </div>
      <div class="timer-controls-col">
        <div class="timer-preset-block">
          <span class="card-label">
            ${icons.icon('timer', 13)}
            <span>POMODORO &amp; FOCO</span>
          </span>
          <div class="preset-row" role="group" aria-label="Durações rápidas">
            ${presetsHtml}
          </div>
        </div>
        <div class="timer-actions">
          <button type="button" class="timer-hero-btn${running ? ' is-running' : ''}" data-action="timer-toggle">
            ${icons.icon(primaryIcon, 19)}
            <span>${primaryLabel}</span>
          </button>
          <button type="button" class="timer-reset-btn" data-action="timer-reset" aria-label="Reiniciar timer" title="Reiniciar">
            ${icons.icon('rotate_ccw', 16)}
            <span>Reiniciar</span>
          </button>
        </div>
      </div>
    </div>`;
}

function paint(force = false) {
  if (!mountedEl || !latestCtx) return;

  const sig = `${status}|${presetMinutes}|${totalSecs}|${remainingSecs}`;
  if (!force && sig === lastSig) return;
  lastSig = sig;

  if (status === 'done') {
    mountedEl.innerHTML = buildDoneHtml(latestCtx);
  } else {
    mountedEl.innerHTML = buildActiveHtml(latestCtx);
  }
}

function bindMount(mount) {
  if (mountedEl === mount) return;
  mountedEl = mount;
  lastSig = '';

  mount.addEventListener('click', (ev) => {
    const presetBtn = ev.target.closest('[data-action="timer-preset"]');
    if (presetBtn) {
      selectPreset(presetBtn.dataset.minutes);
      return;
    }

    const toggleBtn = ev.target.closest('[data-action="timer-toggle"]');
    if (toggleBtn) {
      if (status === 'running') {
        pauseCountdown();
      } else {
        startCountdown();
      }
      return;
    }

    const resetBtn = ev.target.closest('[data-action="timer-reset"]');
    if (resetBtn) {
      resetCountdown();
      return;
    }

    const dismissBtn = ev.target.closest('[data-action="timer-dismiss"]');
    if (dismissBtn) {
      resetCountdown();
      return;
    }

    const restartBtn = ev.target.closest('[data-action="timer-restart"]');
    if (restartBtn) {
      restartCountdown();
    }
  });
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;
  latestCtx = ctx;

  bindMount(mount);

  if (status === 'running') {
    const nextRemaining = Math.max(0, Math.ceil((endAtMs - Date.now()) / 1000));
    if (nextRemaining <= 0) {
      finishTimer();
      return;
    }
    remainingSecs = nextRemaining;
  }

  syncRailBadge();
  paint(false);
}
