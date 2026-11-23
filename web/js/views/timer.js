const PRESET_MINUTES = [5, 15, 25, 45];

let presetMinutes = 25;
let totalSecs = presetMinutes * 60;
let remainingSecs = totalSecs;
let status = 'idle'; // 'idle' | 'running' | 'paused' | 'done'
let endAtMs = 0;
let tickInterval = null;

let mountedEl = null;
let latestCtx = null;
let renderedMode = ''; // '' | 'active' | 'done'
let lastRingModeSig = '';
let lastControlsSig = '';
let lastRemainingRendered = -1;

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

function buildActiveShellHtml(ctx) {
  const { icons } = ctx;
  const presetsHtml = PRESET_MINUTES.map((m) => `
      <button type="button" class="timer-preset-btn num" data-action="timer-preset" data-minutes="${m}" aria-pressed="false">
        ${m}m
      </button>`).join('');

  return `
    <div class="timer-layout">
      <div class="timer-ring-col">
        <div class="timer-ring-box" data-timer-ring></div>
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
          <button type="button" class="timer-hero-btn" data-action="timer-toggle">
            <span data-timer-toggle-icon style="display:inline-flex"></span>
            <span data-timer-toggle-label>Iniciar</span>
          </button>
          <button type="button" class="timer-reset-btn" data-action="timer-reset" aria-label="Reiniciar timer" title="Reiniciar">
            ${icons.icon('rotate_ccw', 16)}
            <span>Reiniciar</span>
          </button>
        </div>
      </div>
    </div>`;
}

function updateRingDom(ctx, forceFullRing) {
  if (!mountedEl) return;
  const ringBox = mountedEl.querySelector('[data-timer-ring]');
  if (!ringBox) return;

  const running = status === 'running';
  const paused = status === 'paused';
  const progress = totalSecs > 0 ? Math.max(0, Math.min(1, remainingSecs / totalSecs)) : 0;
  const label = formatMMSS(remainingSecs);
  const sub = running ? 'Em andamento' : paused ? 'Pausado' : 'Pronto';
  const ringModeSig = `${running ? 1 : 0}|${sub}`;

  const svg = ringBox.querySelector('svg');
  const progressCircle = svg ? svg.querySelector('circle[stroke-dasharray]') : null;
  const textEls = svg ? svg.querySelectorAll('text') : [];

  if (
    !forceFullRing &&
    svg &&
    progressCircle &&
    textEls.length >= 1 &&
    ringModeSig === lastRingModeSig
  ) {
    // Patch SVG in place so stroke-dashoffset CSS transition stays smooth across ticks
    const circumference = Number(progressCircle.getAttribute('stroke-dasharray')) || (2 * Math.PI * 90);
    const dashOffset = circumference * (1 - progress);
    progressCircle.setAttribute('stroke-dashoffset', dashOffset.toFixed(2));
    textEls[0].textContent = label;
    svg.setAttribute('aria-label', `Timer ${label} ${sub}`);
    return;
  }

  lastRingModeSig = ringModeSig;
  ringBox.innerHTML = ctx.charts.renderTimerRing({
    progress,
    label,
    sub,
    active: running,
  });
}

function updateControlsDom(ctx) {
  if (!mountedEl) return;
  const controlsSig = `${status}|${presetMinutes}`;
  if (controlsSig === lastControlsSig) return;
  lastControlsSig = controlsSig;

  const running = status === 'running';
  const paused = status === 'paused';

  mountedEl.querySelectorAll('[data-action="timer-preset"]').forEach((btn) => {
    const isActive = Number(btn.dataset.minutes) === presetMinutes;
    btn.classList.toggle('is-active', isActive);
    btn.setAttribute('aria-pressed', isActive ? 'true' : 'false');
  });

  const toggleBtn = mountedEl.querySelector('[data-action="timer-toggle"]');
  const toggleIcon = mountedEl.querySelector('[data-timer-toggle-icon]');
  const toggleLabel = mountedEl.querySelector('[data-timer-toggle-label]');
  if (toggleBtn) {
    toggleBtn.classList.toggle('is-running', running);
  }
  if (toggleIcon) {
    toggleIcon.innerHTML = ctx.icons.icon(running ? 'pause' : 'play', 19);
  }
  if (toggleLabel) {
    toggleLabel.textContent = running ? 'Pausar' : paused ? 'Retomar' : 'Iniciar';
  }
}

function paint(force = false) {
  if (!mountedEl || !latestCtx) return;

  const nextMode = status === 'done' ? 'done' : 'active';
  if (nextMode === 'done') {
    if (force || renderedMode !== 'done') {
      renderedMode = 'done';
      lastRingModeSig = '';
      lastControlsSig = '';
      lastRemainingRendered = -1;
      mountedEl.innerHTML = buildDoneHtml(latestCtx);
    }
    return;
  }

  let mountedFreshShell = false;
  if (renderedMode !== 'active' || !mountedEl.querySelector('.timer-layout')) {
    renderedMode = 'active';
    lastRingModeSig = '';
    lastControlsSig = '';
    lastRemainingRendered = -1;
    mountedEl.innerHTML = buildActiveShellHtml(latestCtx);
    mountedFreshShell = true;
  }

  updateControlsDom(latestCtx);

  if (force || mountedFreshShell || remainingSecs !== lastRemainingRendered) {
    lastRemainingRendered = remainingSecs;
    updateRingDom(latestCtx, mountedFreshShell);
  }
}

function bindMount(mount) {
  if (mountedEl === mount) return;
  mountedEl = mount;
  renderedMode = '';
  lastRingModeSig = '';
  lastControlsSig = '';
  lastRemainingRendered = -1;

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
