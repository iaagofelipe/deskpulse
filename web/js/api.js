const TOKEN_STORAGE_KEY = 'deskpulse_token';

let unauthorizedHandler = null;
let wakeLockSentinel = null;
let wakeLockListenerAttached = false;
let audioCtx = null;
let audioUnlocked = false;

export function setUnauthorizedHandler(fn) {
  unauthorizedHandler = typeof fn === 'function' ? fn : null;
}

export function saveToken(token) {
  const clean = String(token ?? '').trim();
  try {
    if (clean) {
      localStorage.setItem(TOKEN_STORAGE_KEY, clean);
    } else {
      localStorage.removeItem(TOKEN_STORAGE_KEY);
    }
  } catch {
    // Ignore storage quota / privacy mode errors
  }
  return clean;
}

export function clearToken() {
  try {
    localStorage.removeItem(TOKEN_STORAGE_KEY);
  } catch {
    // Ignore storage errors
  }
}

export function loadToken() {
  try {
    const hash = window.location.hash || '';
    if (hash.length > 1) {
      const raw = hash.slice(1);
      let hashToken = '';
      if (raw.startsWith('token=')) {
        hashToken = decodeURIComponent(raw.slice(6).split('&')[0] || '').trim();
      } else if (raw.includes('token=')) {
        const params = new URLSearchParams(raw);
        hashToken = (params.get('token') || '').trim();
      }
      if (hashToken) {
        saveToken(hashToken);
        try {
          const cleanUrl = `${window.location.pathname || '/'}${window.location.search || ''}`;
          history.replaceState(null, '', cleanUrl);
        } catch {
          // Ignore history API restrictions
        }
        return hashToken;
      }
    }
  } catch {
    // Fallback to localStorage
  }

  try {
    return (localStorage.getItem(TOKEN_STORAGE_KEY) || '').trim();
  } catch {
    return '';
  }
}

export async function apiRequest(method, path, bodyObj) {
  const token = loadToken();
  const headers = {
    Accept: 'application/json',
  };
  if (token) {
    headers['X-DeskPulse-Token'] = token;
  }

  const init = {
    method: String(method || 'GET').toUpperCase(),
    headers,
    cache: 'no-store',
  };

  if (bodyObj !== undefined && bodyObj !== null) {
    headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(bodyObj);
  }

  const resp = await fetch(path, init);

  if (resp.status === 401 || resp.status === 403) {
    if (unauthorizedHandler) {
      unauthorizedHandler(resp.status);
    }
    const err = new Error('Token inválido ou acesso não autorizado');
    err.status = resp.status;
    err.unauthorized = true;
    throw err;
  }

  const ct = resp.headers.get('content-type') || '';
  let data = null;
  if (ct.includes('application/json')) {
    try {
      data = await resp.json();
    } catch {
      data = null;
    }
  } else {
    const text = await resp.text().catch(() => '');
    data = text ? { message: text } : null;
  }

  if (!resp.ok) {
    const msg = (data && (data.error || data.message)) || `HTTP ${resp.status}`;
    const err = new Error(msg);
    err.status = resp.status;
    err.data = data;
    throw err;
  }

  return data;
}

function dispatchSseFrame(rawFrame, handlers) {
  if (!rawFrame || !rawFrame.trim()) return;
  let eventName = 'message';
  const dataLines = [];

  for (const line of rawFrame.split('\n')) {
    if (!line || line.startsWith(':')) continue;
    const colon = line.indexOf(':');
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? '' : line.slice(colon + 1);
    if (value.startsWith(' ')) value = value.slice(1);

    if (field === 'event') {
      eventName = value.trim() || 'message';
    } else if (field === 'data') {
      dataLines.push(value);
    }
  }

  if (!dataLines.length) return;
  const payloadStr = dataLines.join('\n');
  let parsed = null;
  try {
    parsed = JSON.parse(payloadStr);
  } catch {
    return;
  }

  if (eventName === 'state') {
    handlers.onState?.(parsed);
  } else if (eventName === 'notify') {
    handlers.onNotify?.(parsed);
  } else if (eventName === 'approval') {
    handlers.onApproval?.(parsed);
  } else if (eventName === 'approval-done') {
    handlers.onApprovalDone?.(parsed);
  }
}

export function startStream({
  token,
  onState,
  onNotify,
  onApproval,
  onApprovalDone,
  onConnChange,
  onUnauthorized,
} = {}) {
  let stopped = false;
  let activeController = null;
  let reconnectTimer = null;
  let backoffMs = 1000;
  const maxBackoffMs = 5000;
  const handlers = { onState, onNotify, onApproval, onApprovalDone };

  const scheduleReconnect = () => {
    if (stopped) return;
    onConnChange?.('reconnecting');
    const delay = backoffMs;
    backoffMs = Math.min(maxBackoffMs, Math.round(backoffMs * 1.6));
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, delay);
  };

  const connect = async () => {
    if (stopped) return;
    const activeToken = String(token ?? loadToken()).trim();
    if (!activeToken) {
      onConnChange?.('unauthorized');
      onUnauthorized?.();
      return;
    }

    activeController = new AbortController();
    onConnChange?.('connecting');

    try {
      const resp = await fetch('/api/stream', {
        method: 'GET',
        headers: {
          'X-DeskPulse-Token': activeToken,
          Accept: 'text/event-stream',
        },
        cache: 'no-store',
        signal: activeController.signal,
      });

      if (resp.status === 401 || resp.status === 403) {
        onConnChange?.('unauthorized');
        onUnauthorized?.();
        return;
      }

      if (!resp.ok || !resp.body) {
        scheduleReconnect();
        return;
      }

      backoffMs = 1000;
      onConnChange?.('connected');

      const reader = resp.body.getReader();
      const decoder = new TextDecoder('utf-8');
      let buffer = '';

      while (!stopped) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n');

        let sepIndex = buffer.indexOf('\n\n');
        while (sepIndex !== -1) {
          const frame = buffer.slice(0, sepIndex);
          buffer = buffer.slice(sepIndex + 2);
          dispatchSseFrame(frame, handlers);
          sepIndex = buffer.indexOf('\n\n');
        }
      }

      if (!stopped && buffer.trim()) {
        dispatchSseFrame(buffer, handlers);
      }

      if (!stopped) {
        scheduleReconnect();
      }
    } catch (err) {
      if (stopped || (err && err.name === 'AbortError')) return;
      scheduleReconnect();
    }
  };

  connect();

  const stop = () => {
    stopped = true;
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (activeController) {
      try {
        activeController.abort();
      } catch {
        // Ignore abort errors
      }
      activeController = null;
    }
    onConnChange?.('disconnected');
  };

  return {
    stop,
    abort: stop,
  };
}

export async function requestWakeLock() {
  if (typeof navigator === 'undefined' || !navigator.wakeLock || !navigator.wakeLock.request) {
    return false;
  }

  if (!wakeLockListenerAttached && typeof document !== 'undefined') {
    wakeLockListenerAttached = true;
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible' && !wakeLockSentinel) {
        requestWakeLock();
      }
    });
  }

  if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
    return false;
  }

  try {
    if (wakeLockSentinel && !wakeLockSentinel.released) {
      return true;
    }
    wakeLockSentinel = await navigator.wakeLock.request('screen');
    wakeLockSentinel.addEventListener('release', () => {
      wakeLockSentinel = null;
    });
    return true;
  } catch {
    wakeLockSentinel = null;
    return false;
  }
}

export function ensureAudioUnlocked() {
  if (typeof window === 'undefined') return null;
  const Ctor = window.AudioContext || window.webkitAudioContext;
  if (!Ctor) return null;

  try {
    if (!audioCtx) {
      audioCtx = new Ctor();
    }
    if (audioCtx.state === 'suspended') {
      audioCtx.resume().catch(() => {});
    }
    if (!audioUnlocked) {
      const buf = audioCtx.createBuffer(1, 1, 22050);
      const src = audioCtx.createBufferSource();
      src.buffer = buf;
      src.connect(audioCtx.destination);
      src.start(0);
      if (audioCtx.state === 'running') {
        audioUnlocked = true;
      }
    }
    return audioCtx;
  } catch {
    return null;
  }
}

export function playChime(freqs = [660, 880]) {
  if (typeof window === 'undefined') return;
  const Ctor = window.AudioContext || window.webkitAudioContext;
  if (!Ctor) return;

  try {
    if (!audioCtx) {
      audioCtx = new Ctor();
    }
    if (audioCtx.state === 'suspended') {
      audioCtx.resume().catch(() => {});
    }

    const notes = Array.isArray(freqs) && freqs.length ? freqs : [660, 880];
    const now = audioCtx.currentTime;

    notes.forEach((freq, i) => {
      const f = Number(freq);
      if (!Number.isFinite(f) || f <= 0) return;
      const start = now + i * 0.11;
      const dur = 0.22;

      const osc = audioCtx.createOscillator();
      const gain = audioCtx.createGain();
      osc.type = 'sine';
      osc.frequency.setValueAtTime(f, start);

      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.exponentialRampToValueAtTime(0.14, start + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + dur);

      osc.connect(gain);
      gain.connect(audioCtx.destination);

      osc.start(start);
      osc.stop(start + dur + 0.02);
    });
  } catch {
    // Ignore autoplay / audio context restrictions
  }
}

export function alertUser(isApproval = false) {
  try {
    if (typeof navigator !== 'undefined' && typeof navigator.vibrate === 'function') {
      navigator.vibrate(isApproval ? [160, 80, 160, 80, 220] : [90, 50, 90]);
    }
  } catch {
    // Ignore vibration unsupported errors
  }

  playChime(isApproval ? [784, 988, 1175] : [587, 880]);
}

export function escapeHtml(str) {
  return String(str ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

export function formatBytes(bytes) {
  const n = Number(bytes);
  if (!Number.isFinite(n) || n <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let idx = 0;
  let val = n;
  while (val >= 1024 && idx < units.length - 1) {
    val /= 1024;
    idx++;
  }
  const fixed = idx === 0 ? Math.round(val).toString() : (val >= 100 ? val.toFixed(0) : val.toFixed(1));
  return `${fixed} ${units[idx]}`;
}

export function formatCompactNumber(n) {
  const num = Number(n);
  if (!Number.isFinite(num)) return '0';
  const abs = Math.abs(num);
  const sign = num < 0 ? '-' : '';
  if (abs >= 1e9) return `${sign}${(abs / 1e9).toFixed(abs >= 1e10 ? 0 : 1)}B`;
  if (abs >= 1e6) return `${sign}${(abs / 1e6).toFixed(abs >= 1e7 ? 0 : 1)}M`;
  if (abs >= 1e3) return `${sign}${(abs / 1e3).toFixed(abs >= 1e4 ? 0 : 1)}k`;
  return `${sign}${Math.round(abs)}`;
}

export function formatDuration(secs) {
  const total = Math.max(0, Math.floor(Number(secs) || 0));
  const days = Math.floor(total / 86400);
  const hours = Math.floor((total % 86400) / 3600);
  const mins = Math.floor((total % 3600) / 60);
  const remSecs = total % 60;

  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${mins}m`;
  if (mins > 0) return `${mins}m ${String(remSecs).padStart(2, '0')}s`;
  return `${remSecs}s`;
}

export function formatClock(date = new Date()) {
  const d = date instanceof Date ? date : new Date(date);
  if (Number.isNaN(d.getTime())) return '00:00';
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  return `${hh}:${mm}`;
}

const WEEKDAYS_PT = [
  'domingo',
  'segunda-feira',
  'terça-feira',
  'quarta-feira',
  'quinta-feira',
  'sexta-feira',
  'sábado',
];

const MONTHS_PT = [
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

export function formatFullDatePt(date = new Date()) {
  const d = date instanceof Date ? date : new Date(date);
  if (Number.isNaN(d.getTime())) return '';
  try {
    return new Intl.DateTimeFormat('pt-BR', {
      weekday: 'long',
      day: 'numeric',
      month: 'long',
    }).format(d);
  } catch {
    return `${WEEKDAYS_PT[d.getDay()]}, ${d.getDate()} de ${MONTHS_PT[d.getMonth()]}`;
  }
}
