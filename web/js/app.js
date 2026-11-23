import * as icons from './icons.js';
import * as charts from './charts.js';
import * as api from './api.js';

import { render as renderHome } from './views/home.js';
import { render as renderPc } from './views/pc.js';
import { render as renderAi } from './views/ai.js';
import { render as renderAgenda } from './views/agenda.js';
import { render as renderWeather } from './views/weather.js';
import { render as renderMedia } from './views/media.js';
import { render as renderGithub } from './views/github.js';
import { render as renderShortcuts } from './views/shortcuts.js';
import { render as renderTimer } from './views/timer.js';

const SOUND_STORAGE_KEY = 'deskpulse_sound';
const WAKE_DURATION_MS = 30_000;
const BURN_IN_SHIFT_MS = 60_000;
const BURN_IN_OFFSETS = [
  [0, 0],
  [12, -8],
  [18, 4],
  [8, 11],
  [-10, 9],
  [-18, -3],
  [-11, -11],
  [4, -12],
];

const views = {
  home: renderHome,
  pc: renderPc,
  ai: renderAi,
  agenda: renderAgenda,
  weather: renderWeather,
  media: renderMedia,
  github: renderGithub,
  shortcuts: renderShortcuts,
  timer: renderTimer,
};

const dom = {
  navRail: document.getElementById('navRail'),
  navButtons: Array.from(document.querySelectorAll('.nav-btn[data-tab]')),
  viewSections: new Map(
    Array.from(document.querySelectorAll('.view[data-view]')).map((el) => [el.dataset.view, el])
  ),
  aiNavBadge: document.getElementById('aiNavBadge'),
  timerNavBadge: document.getElementById('timerNavBadge'),
  fullscreenBtn: document.getElementById('fullscreenBtn'),
  connCluster: document.getElementById('connCluster'),
  connIcon: document.getElementById('connIcon'),
  usbIcon: document.getElementById('usbIcon'),
  screensaverOverlay: document.getElementById('screensaverOverlay'),
  ssContent: document.getElementById('ssContent'),
  ssClock: document.getElementById('ssClock'),
  ssDate: document.getElementById('ssDate'),
  ssHint: document.getElementById('ssHint'),
  approvalOverlay: document.getElementById('approvalOverlay'),
  approvalMount: document.getElementById('approvalMount'),
  tokenOverlay: document.getElementById('tokenOverlay'),
  tokenForm: document.getElementById('tokenForm'),
  tokenInput: document.getElementById('tokenInput'),
  tokenError: document.getElementById('tokenError'),
  tokenCloseBtn: document.getElementById('tokenCloseBtn'),
  saveTokenBtn: document.getElementById('saveTokenBtn'),
};

let state = null;
let activeTab = 'home';
let streamHandle = null;
let connStatus = 'connecting';
let soundEnabled = loadSoundPref();
let wakeUntilMs = 0;
let burnInIndex = 0;
let renderedApprovalKey = null;
let approvalTickTimer = null;
let approvalSubmitting = false;
let lastClockMinute = '';

function loadSoundPref() {
  try {
    const raw = localStorage.getItem(SOUND_STORAGE_KEY);
    return raw === null ? true : raw === '1';
  } catch {
    return true;
  }
}

function toggleSound() {
  soundEnabled = !soundEnabled;
  try {
    localStorage.setItem(SOUND_STORAGE_KEY, soundEnabled ? '1' : '0');
  } catch {
    // Ignore storage errors
  }
  if (soundEnabled) {
    api.ensureAudioUnlocked();
    api.playChime([660, 880]);
  }
  renderActiveView();
  return soundEnabled;
}

function playAlert(isApproval = false) {
  if (soundEnabled) {
    api.alertUser(isApproval);
  } else {
    try {
      if (navigator.vibrate) {
        navigator.vibrate(isApproval ? [160, 80, 160, 80, 220] : [90, 50, 90]);
      }
    } catch {
      // Ignore vibration errors
    }
  }
}

function playChimeSafe(freqs) {
  if (soundEnabled) {
    api.playChime(freqs);
  }
}

function hydrateRailIcons() {
  if (!dom.navRail) return;
  dom.navRail.querySelectorAll('.nav-icon[data-icon]').forEach((el) => {
    el.innerHTML = icons.icon(el.dataset.icon, 20);
  });
}

function isFullscreenActive() {
  return Boolean(document.fullscreenElement || document.webkitFullscreenElement);
}

function syncFullscreenIcon() {
  if (!dom.fullscreenBtn) return;
  const active = isFullscreenActive();
  const iconName = active ? 'exit_fullscreen' : 'fullscreen';
  const label = active ? 'Sair da tela cheia' : 'Tela cheia';
  const span = dom.fullscreenBtn.querySelector('.nav-icon');
  if (span) {
    span.dataset.icon = iconName;
    span.innerHTML = icons.icon(iconName, 20);
  }
  dom.fullscreenBtn.setAttribute('aria-label', label);
  dom.fullscreenBtn.title = label;
}

async function toggleFullscreen() {
  try {
    if (isFullscreenActive()) {
      if (document.exitFullscreen) {
        await document.exitFullscreen();
      } else if (document.webkitExitFullscreen) {
        document.webkitExitFullscreen();
      }
    } else {
      const root = document.documentElement;
      if (root.requestFullscreen) {
        await root.requestFullscreen();
      } else if (root.webkitRequestFullscreen) {
        root.webkitRequestFullscreen();
      }
    }
  } catch {
    // Fullscreen may be blocked by browser policy
  } finally {
    syncFullscreenIcon();
  }
}

function setStatusDot(el, level, title) {
  if (!el) return;
  el.classList.remove('status-ok', 'status-warn', 'status-err', 'status-muted');
  el.classList.add(`status-${level}`);
  if (title) {
    el.title = title;
    el.setAttribute('aria-label', title);
  }
}

function updateConnectionIndicators() {
  if (connStatus === 'connected') {
    setStatusDot(dom.connIcon, 'ok', 'PC conectado (SSE ativo)');
  } else if (connStatus === 'connecting' || connStatus === 'reconnecting') {
    setStatusDot(dom.connIcon, 'warn', 'Conectando ao PC...');
  } else {
    setStatusDot(dom.connIcon, 'err', 'PC desconectado');
  }

  const adb = state?.adb;
  if (!adb) {
    setStatusDot(dom.usbIcon, 'muted', 'Ponte USB ADB aguardando dados');
    return;
  }
  if (adb.reverse_active) {
    const devCount = Array.isArray(adb.devices) ? adb.devices.length : 1;
    setStatusDot(dom.usbIcon, 'ok', `USB ADB ativo (${devCount} dispositivo${devCount > 1 ? 's' : ''})`);
  } else if (Array.isArray(adb.devices) && adb.devices.length > 0) {
    setStatusDot(dom.usbIcon, 'warn', 'USB conectado — aguardando reverse ADB');
  } else if (adb.adb_found) {
    setStatusDot(dom.usbIcon, 'muted', 'ADB disponível — sem cabo USB conectado');
  } else {
    setStatusDot(dom.usbIcon, 'muted', 'ADB não encontrado no PC');
  }
}

function updateAiNavBadge() {
  const badge = dom.aiNavBadge;
  if (!badge) return;

  const pendingApprovals = Array.isArray(state?.approvals) ? state.approvals.length : 0;
  const sessions = state?.ai?.sessions || state?.antigravity?.sessions || [];
  const activeSessions = Array.isArray(sessions)
    ? sessions.filter((s) => s && (s.status === 'running' || s.status === 'waiting')).length
    : 0;

  if (pendingApprovals > 0) {
    badge.textContent = String(pendingApprovals);
    badge.classList.remove('hidden');
    badge.classList.add('is-urgent');
  } else if (activeSessions > 0) {
    badge.textContent = String(activeSessions);
    badge.classList.remove('hidden', 'is-urgent');
  } else {
    badge.textContent = '';
    badge.classList.add('hidden');
    badge.classList.remove('is-urgent');
  }
}

function setTimerBadge(text) {
  const badge = dom.timerNavBadge;
  if (!badge) return;
  const clean = String(text ?? '').trim();
  if (clean) {
    badge.textContent = clean;
    badge.classList.remove('hidden');
  } else {
    badge.textContent = '';
    badge.classList.add('hidden');
  }
}

function setTab(tabId) {
  if (!views[tabId]) return;
  activeTab = tabId;

  dom.navButtons.forEach((btn) => {
    const isActive = btn.dataset.tab === tabId;
    btn.classList.toggle('active', isActive);
    btn.setAttribute('aria-selected', isActive ? 'true' : 'false');
  });

  dom.viewSections.forEach((section, key) => {
    section.classList.toggle('active', key === tabId);
  });

  renderActiveView();
}

function buildContext(tabId) {
  const mount = dom.viewSections.get(tabId) || null;
  return {
    state,
    activeTab,
    mount,
    el: mount,
    apiRequest: api.apiRequest,
    setTab,
    icons,
    charts,
    setTimerBadge,
    soundEnabled,
    toggleSound,
    showTokenGate,
    alertUser: playAlert,
    playChime: playChimeSafe,
    escapeHtml: api.escapeHtml,
    formatBytes: api.formatBytes,
    formatCompactNumber: api.formatCompactNumber,
    formatDuration: api.formatDuration,
    formatClock: api.formatClock,
    formatFullDatePt: api.formatFullDatePt,
  };
}

function renderActiveView() {
  const renderer = views[activeTab];
  const mount = dom.viewSections.get(activeTab);
  if (!renderer || !mount) return;
  try {
    renderer(buildContext(activeTab));
  } catch (err) {
    console.error(`[DeskPulse] Failed to render view "${activeTab}":`, err);
  }
}

// ============================================================================
// Token Gate Overlay
// ============================================================================

function showTokenError(msg) {
  if (!dom.tokenError) return;
  if (msg) {
    dom.tokenError.textContent = msg;
    dom.tokenError.classList.remove('hidden');
  } else {
    dom.tokenError.textContent = '';
    dom.tokenError.classList.add('hidden');
  }
}

function showTokenGate(errorMsg = '') {
  if (!dom.tokenOverlay) return;
  showTokenError(errorMsg);
  const current = api.loadToken();
  if (dom.tokenInput) {
    dom.tokenInput.value = current;
  }
  if (dom.tokenCloseBtn) {
    dom.tokenCloseBtn.classList.toggle('hidden', !current);
  }
  dom.tokenOverlay.classList.remove('hidden');
  updateScreensaver();
  setTimeout(() => dom.tokenInput?.focus(), 40);
}

function hideTokenGate() {
  if (!dom.tokenOverlay) return;
  dom.tokenOverlay.classList.add('hidden');
  showTokenError('');
  updateScreensaver();
}

async function handleTokenSubmit(ev) {
  ev.preventDefault();
  const raw = dom.tokenInput ? dom.tokenInput.value.trim() : '';
  if (!raw) {
    showTokenError('Informe o token de acesso do DeskPulse.');
    return;
  }

  showTokenError('');
  if (dom.saveTokenBtn) dom.saveTokenBtn.disabled = true;
  api.saveToken(raw);

  try {
    const initialState = await api.apiRequest('GET', '/api/state');
    state = initialState;
    hideTokenGate();
    connectStream(raw);
    handleStateUpdate(initialState);
  } catch (err) {
    if (err && err.unauthorized) {
      showTokenError('Token recusado pelo PC. Verifique e tente novamente.');
    } else {
      // Network might be temporarily unreachable; keep token and attempt stream
      hideTokenGate();
      connectStream(raw);
    }
  } finally {
    if (dom.saveTokenBtn) dom.saveTokenBtn.disabled = false;
  }
}

// ============================================================================
// Remote Approval Overlay (#approvalOverlay)
// ============================================================================

function stopApprovalTicker() {
  if (approvalTickTimer !== null) {
    clearInterval(approvalTickTimer);
    approvalTickTimer = null;
  }
}

function resolveApprovalTiming(req, now) {
  const created = Number(req?.created_at_ms);
  const startMs = Number.isFinite(created) && created > 0 ? created : now;
  const rawExpires = Number(req?.expires_at_ms);
  const rawTimeout = Number(req?.timeout_secs);
  let expiresMs;
  if (Number.isFinite(rawExpires) && rawExpires > 0) {
    expiresMs = rawExpires;
  } else if (Number.isFinite(rawTimeout) && rawTimeout > 0) {
    expiresMs = startMs + rawTimeout * 1000;
  } else {
    expiresMs = startMs + 45_000;
  }
  const totalMs = Math.max(1000, expiresMs - startMs);
  const remMs = Math.max(0, expiresMs - now);
  const pct = Math.max(0, Math.min(100, (remMs / totalMs) * 100));
  const remSec = Math.ceil(remMs / 1000);
  return { startMs, expiresMs, totalMs, remMs, pct, remSec };
}

function updateApprovalCountdown() {
  const approvals = Array.isArray(state?.approvals) ? state.approvals : [];
  if (!approvals.length) {
    stopApprovalTicker();
    return;
  }

  const req = approvals[0];
  const now = Date.now();
  const { expiresMs, pct, remSec } = resolveApprovalTiming(req, now);

  const fillEl = dom.approvalMount?.querySelector('.approval-countdown-fill');
  if (fillEl) {
    fillEl.style.width = `${pct.toFixed(1)}%`;
  }
  const secEl = dom.approvalMount?.querySelector('[data-approval-sec]');
  if (secEl) {
    secEl.textContent = `${remSec}s`;
  }

  // Local fallback cleanup if approval expired well past server timeout
  if (now > expiresMs + 2000) {
    state.approvals = approvals.filter((a) => a.id !== req.id);
    renderedApprovalKey = null;
    updateAiNavBadge();
    renderApprovalOverlay();
  }
}

async function decideApproval(id, decision) {
  if (approvalSubmitting || !id) return;
  approvalSubmitting = true;

  const buttons = dom.approvalMount?.querySelectorAll('button[data-decision]') || [];
  buttons.forEach((b) => {
    b.disabled = true;
  });

  try {
    await api.apiRequest('POST', `/api/approvals/${encodeURIComponent(id)}`, {
      decision,
    });
    if (state && Array.isArray(state.approvals)) {
      state.approvals = state.approvals.filter((a) => a.id !== id);
    }
  } catch (err) {
    // 404 means the approval already expired or was resolved on PC
    if (err && err.status === 404 && state && Array.isArray(state.approvals)) {
      state.approvals = state.approvals.filter((a) => a.id !== id);
    } else {
      buttons.forEach((b) => {
        b.disabled = false;
      });
    }
  } finally {
    approvalSubmitting = false;
    renderedApprovalKey = null;
    updateAiNavBadge();
    renderApprovalOverlay();
    renderActiveView();
  }
}

function renderApprovalOverlay() {
  if (!dom.approvalOverlay || !dom.approvalMount) return;
  const approvals = Array.isArray(state?.approvals) ? state.approvals : [];

  if (!approvals.length) {
    dom.approvalOverlay.classList.add('hidden');
    dom.approvalMount.innerHTML = '';
    renderedApprovalKey = null;
    stopApprovalTicker();
    updateScreensaver();
    return;
  }

  const req = approvals[0];
  const key = `${req.id}|${approvals.length}`;
  if (dom.screensaverOverlay) {
    dom.screensaverOverlay.classList.add('hidden');
  }
  dom.approvalOverlay.classList.remove('hidden');

  if (renderedApprovalKey !== key) {
    renderedApprovalKey = key;
    const now = Date.now();
    const { pct, remSec } = resolveApprovalTiming(req, now);

    const queueBadge = approvals.length > 1
      ? `<span class="badge badge-ai num">1 de ${approvals.length}</span>`
      : '';
    const cwdLine = req.cwd ? ` · ${req.cwd}` : '';

    dom.approvalMount.innerHTML = `
      <div class="approval-head">
        <h2 class="approval-title">${api.escapeHtml(req.title || 'Aprovação remota pendente')}</h2>
        <div style="display:flex;align-items:center;gap:6px;flex-shrink:0">
          ${queueBadge}
          <span class="badge badge-warn num" data-approval-sec>${remSec}s</span>
        </div>
      </div>
      <div class="approval-tool">${api.escapeHtml((req.tool_name || 'ferramenta') + cwdLine)}</div>
      <div class="approval-summary">${api.escapeHtml(req.summary || 'Sem detalhes adicionais.')}</div>
      <div class="approval-countdown" aria-hidden="true">
        <div class="approval-countdown-fill" style="width:${pct.toFixed(1)}%"></div>
      </div>
      <div class="approval-actions">
        <button type="button" class="btn-allow" data-decision="allow" data-approval-id="${api.escapeHtml(req.id)}">
          ${icons.icon('check', 18)}
          <span>Permitir</span>
        </button>
        <button type="button" class="btn-deny" data-decision="deny" data-approval-id="${api.escapeHtml(req.id)}">
          ${icons.icon('x', 18)}
          <span>Negar</span>
        </button>
        <button type="button" class="btn-pc" data-decision="ask" data-approval-id="${api.escapeHtml(req.id)}">
          ${icons.icon('monitor', 18)}
          <span>Responder no PC</span>
        </button>
      </div>
    `;

    dom.approvalMount.querySelectorAll('button[data-decision]').forEach((btn) => {
      btn.addEventListener('click', () => {
        decideApproval(btn.dataset.approvalId, btn.dataset.decision);
      });
    });
  }

  if (approvalTickTimer === null) {
    approvalTickTimer = setInterval(updateApprovalCountdown, 200);
  }
  updateApprovalCountdown();
  updateScreensaver();
}

// ============================================================================
// OLED Night Mode & PC-Lock Screensaver (#screensaverOverlay)
// ============================================================================

function isNightHour(hour, startHour, endHour) {
  const s = Number(startHour);
  const e = Number(endHour);
  if (!Number.isFinite(s) || !Number.isFinite(e) || s === e) return false;
  if (s < e) {
    return hour >= s && hour < e;
  }
  // Midnight wrap-around (e.g. 23:00 -> 07:00)
  return hour >= s || hour < e;
}

function evaluateScreensaverReason() {
  if (!state || !state.night_mode) return null;
  if (dom.tokenOverlay && !dom.tokenOverlay.classList.contains('hidden')) return null;
  if (Array.isArray(state.approvals) && state.approvals.length > 0) return null;
  if (Date.now() < wakeUntilMs) return null;

  const nm = state.night_mode;
  const byLock = Boolean(nm.dim_when_pc_locked) && Boolean(state.pc && state.pc.pc_locked);
  const byNight = Boolean(nm.enabled) && isNightHour(new Date().getHours(), nm.start_hour, nm.end_hour);

  if (byLock) return 'locked';
  if (byNight) return 'night';
  return null;
}

function updateScreensaverClock(reason) {
  const now = new Date();
  if (dom.ssClock) {
    dom.ssClock.textContent = api.formatClock(now);
  }
  if (dom.ssDate) {
    dom.ssDate.textContent = api.formatFullDatePt(now);
  }
  if (dom.ssHint) {
    dom.ssHint.textContent = reason === 'locked'
      ? 'PC bloqueado · toque para acordar'
      : 'Modo noturno OLED · toque para acordar';
  }
}

function updateScreensaver() {
  if (!dom.screensaverOverlay) return;
  const reason = evaluateScreensaverReason();
  if (reason) {
    updateScreensaverClock(reason);
    dom.screensaverOverlay.classList.remove('hidden');
  } else {
    dom.screensaverOverlay.classList.add('hidden');
  }
}

function wakeScreensaver() {
  wakeUntilMs = Date.now() + WAKE_DURATION_MS;
  api.ensureAudioUnlocked();
  api.requestWakeLock();
  updateScreensaver();
}

function applyBurnInShift() {
  if (!dom.ssContent || !dom.screensaverOverlay) return;
  if (dom.screensaverOverlay.classList.contains('hidden')) return;
  burnInIndex = (burnInIndex + 1) % BURN_IN_OFFSETS.length;
  const [dx, dy] = BURN_IN_OFFSETS[burnInIndex];
  dom.ssContent.style.transform = `translate3d(${dx}px, ${dy}px, 0)`;
}

// ============================================================================
// Live SSE Stream Orchestration
// ============================================================================

function handleStateUpdate(nextState) {
  state = nextState;
  updateConnectionIndicators();
  updateAiNavBadge();
  renderApprovalOverlay();
  updateScreensaver();
  renderActiveView();
}

function connectStream(token) {
  if (streamHandle) {
    streamHandle.stop();
    streamHandle = null;
  }

  streamHandle = api.startStream({
    token,
    onState: handleStateUpdate,
    onNotify: (note) => {
      if (state && note) {
        const list = Array.isArray(state.notifications) ? state.notifications.slice() : [];
        if (!list.some((n) => n && n.id === note.id)) {
          list.push(note);
          if (list.length > 30) list.splice(0, list.length - 30);
          state.notifications = list;
        }
      }
      playAlert(false);
      renderActiveView();
    },
    onApproval: (req) => {
      if (!req || !req.id) return;
      if (!state) state = { approvals: [] };
      const list = Array.isArray(state.approvals) ? state.approvals.slice() : [];
      const idx = list.findIndex((a) => a.id === req.id);
      if (idx >= 0) {
        list[idx] = req;
      } else {
        list.push(req);
      }
      state.approvals = list;
      wakeUntilMs = Date.now() + WAKE_DURATION_MS;
      playAlert(true);
      updateAiNavBadge();
      renderApprovalOverlay();
      renderActiveView();
    },
    onApprovalDone: ({ id } = {}) => {
      if (!id || !state || !Array.isArray(state.approvals)) return;
      state.approvals = state.approvals.filter((a) => a.id !== id);
      updateAiNavBadge();
      renderApprovalOverlay();
      renderActiveView();
    },
    onConnChange: (status) => {
      connStatus = status;
      updateConnectionIndicators();
    },
    onUnauthorized: () => {
      connStatus = 'unauthorized';
      updateConnectionIndicators();
      showTokenGate('Sessão não autorizada. Informe o token válido do DeskPulse.');
    },
  });
}

function registerServiceWorker() {
  if ('serviceWorker' in navigator) {
    navigator.serviceWorker.register('/sw.js').catch(() => {});
  }
}

function wireEvents() {
  dom.navButtons.forEach((btn) => {
    btn.addEventListener('click', () => {
      setTab(btn.dataset.tab);
    });
  });

  if (dom.fullscreenBtn) {
    dom.fullscreenBtn.addEventListener('click', toggleFullscreen);
  }
  document.addEventListener('fullscreenchange', syncFullscreenIcon);
  document.addEventListener('webkitfullscreenchange', syncFullscreenIcon);

  if (dom.connCluster) {
    dom.connCluster.addEventListener('click', () => {
      if (connStatus !== 'connected') {
        showTokenGate();
      }
    });
  }

  if (dom.tokenForm) {
    dom.tokenForm.addEventListener('submit', handleTokenSubmit);
  }
  if (dom.tokenCloseBtn) {
    dom.tokenCloseBtn.addEventListener('click', hideTokenGate);
  }

  if (dom.screensaverOverlay) {
    dom.screensaverOverlay.addEventListener('click', wakeScreensaver);
    dom.screensaverOverlay.addEventListener('keydown', (ev) => {
      if (ev.key === 'Enter' || ev.key === ' ') {
        ev.preventDefault();
        wakeScreensaver();
      }
    });
  }

  const unlockOnce = () => {
    api.ensureAudioUnlocked();
    api.requestWakeLock();
  };
  window.addEventListener('pointerdown', unlockOnce, { passive: true });
  window.addEventListener('keydown', unlockOnce, { passive: true });

  api.setUnauthorizedHandler(() => {
    if (streamHandle) {
      streamHandle.stop();
      streamHandle = null;
    }
    connStatus = 'unauthorized';
    updateConnectionIndicators();
    showTokenGate('Token inválido ou expirado. Insira o token atual do DeskPulse.');
  });
}

function startClockTimers() {
  setInterval(() => {
    updateScreensaver();
    const minute = api.formatClock(new Date());
    if (minute !== lastClockMinute) {
      lastClockMinute = minute;
      if (activeTab === 'home') {
        renderActiveView();
      }
    }
  }, 1000);

  setInterval(applyBurnInShift, BURN_IN_SHIFT_MS);
}

function init() {
  hydrateRailIcons();
  syncFullscreenIcon();
  wireEvents();
  registerServiceWorker();
  api.requestWakeLock();
  startClockTimers();

  setTab(activeTab);

  const token = api.loadToken();
  if (!token) {
    connStatus = 'disconnected';
    updateConnectionIndicators();
    showTokenGate();
  } else {
    connectStream(token);
  }
}

init();
