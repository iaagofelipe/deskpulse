const ICON_PATHS = {
  // Navigation
  home: '<path d="M3 10.5L12 3l9 7.5"/><path d="M5 9.5V20a1 1 0 0 0 1 1h4.5v-6h3v6H18a1 1 0 0 0 1-1V9.5"/>',
  pc: '<rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6" rx="1"/><path d="M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 15h3M1 9h3M1 15h3"/>',
  ai: '<path d="M12 2.5l2.2 5.3L19.5 10l-5.3 2.2L12 17.5l-2.2-5.3L4.5 10l5.3-2.2L12 2.5z"/><path d="M18.5 15.5l1 2.3 2.3 1-2.3 1-1 2.3-1-2.3-2.3-1 2.3-1 1-2.3z"/>',
  agenda: '<rect x="3" y="4.5" width="18" height="16.5" rx="2"/><path d="M16 2.5v4M8 2.5v4M3 9.5h18"/><path d="M8 14h3M13 14h3M8 17.5h5"/>',
  weather: '<path d="M17.5 19H7a4.5 4.5 0 0 1-.5-8.97A6 6 0 0 1 18.2 8.5a4 4 0 0 1-.7 10.5z"/>',
  media: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  github: '<path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.4 5.4 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/>',
  shortcuts: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/>',
  timer: '<circle cx="12" cy="13.5" r="7.5"/><path d="M12 13.5V9.5"/><path d="M9.5 2.5h5"/><path d="M18 6l1.8-1.8"/>',

  // Media
  play: '<polygon points="7 4.5 20 12 7 19.5 7 4.5" fill="currentColor" stroke="none"/>',
  pause: '<rect x="6" y="4.5" width="4.2" height="15" rx="1.2" fill="currentColor" stroke="none"/><rect x="13.8" y="4.5" width="4.2" height="15" rx="1.2" fill="currentColor" stroke="none"/>',
  prev: '<polygon points="18.5 5.5 9.5 12 18.5 18.5 18.5 5.5" fill="currentColor" stroke="none"/><rect x="5.5" y="5.5" width="2.4" height="13" rx="1" fill="currentColor" stroke="none"/>',
  next: '<polygon points="5.5 5.5 14.5 12 5.5 18.5 5.5 5.5" fill="currentColor" stroke="none"/><rect x="16.1" y="5.5" width="2.4" height="13" rx="1" fill="currentColor" stroke="none"/>',
  music: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',

  // Shortcuts
  mic: '<rect x="9" y="2.5" width="6" height="11" rx="3"/><path d="M5 10.5a7 7 0 0 0 14 0"/><path d="M12 17.5v4M8.5 21.5h7"/>',
  mic_off: '<path d="M2 2l20 20"/><path d="M15 9.3V5.5a3 3 0 0 0-5.6-1.5"/><path d="M9 9v1.5a3 3 0 0 0 5.1 2.1"/><path d="M5 10.5a7 7 0 0 0 11.4 5.4"/><path d="M19 10.5a7 7 0 0 1-.5 2.6"/><path d="M12 17.5v4M8.5 21.5h7"/>',
  volume_off: '<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M22 9l-6 6M16 9l6 6"/>',
  volume_down: '<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/>',
  volume_up: '<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/><path d="M18.8 5.2a9.5 9.5 0 0 1 0 13.6"/>',
  screenshot: '<path d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2"/><circle cx="12" cy="12" r="3.2"/>',
  terminal: '<polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/>',
  folder: '<path d="M3 6.5A2 2 0 0 1 5 4.5h4.2l2 2.2H19a2 2 0 0 1 2 2v8.8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6.5z"/>',
  lock: '<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>',
  globe: '<circle cx="12" cy="12" r="9.5"/><path d="M2.5 12h19"/><path d="M12 2.5a14.5 14.5 0 0 1 0 19 14.5 14.5 0 0 1 0-19z"/>',
  zap: '<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>',
  code: '<polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>',
  sliders: '<line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1.5" y1="14" x2="6.5" y2="14"/><line x1="9.5" y1="8" x2="14.5" y2="8"/><line x1="17.5" y1="16" x2="22.5" y2="16"/>',

  // System & UI
  monitor: '<rect x="2.5" y="3.5" width="19" height="13" rx="2"/><line x1="8" y1="20.5" x2="16" y2="20.5"/><line x1="12" y1="16.5" x2="12" y2="20.5"/>',
  wifi: '<path d="M5 12.6a10 10 0 0 1 14 0"/><path d="M8.5 16.1a5 5 0 0 1 7 0"/><path d="M2 8.8a15 15 0 0 1 20 0"/><circle cx="12" cy="19.5" r="1" fill="currentColor" stroke="none"/>',
  usb: '<circle cx="10" cy="7" r="1.5"/><circle cx="4" cy="20" r="1.5"/><path d="M4.7 18.6L19 4.3"/><path d="M19 4.3l1.2 1.2M15.5 4.3H19v3.5"/><path d="M9.5 13.8L12 16.3l4-4"/><rect x="16" y="13" width="3" height="3" rx=".5"/><path d="M10 8.5v4.3"/>',
  fullscreen: '<path d="M8 3H5a2 2 0 0 0-2 2v3M21 8V5a2 2 0 0 0-2-2h-3M3 16v3a2 2 0 0 0 2 2h3M16 21h3a2 2 0 0 0 2-2v-3"/>',
  exit_fullscreen: '<path d="M8 3v3a2 2 0 0 1-2 2H3M21 8h-3a2 2 0 0 1-2-2V3M3 16h3a2 2 0 0 1 2 2v3M16 21v-3a2 2 0 0 1 2-2h3"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-2.64-6.36"/><polyline points="21 3 21 9 15 9"/>',
  check: '<polyline points="20 6 9 17 4 12"/>',
  x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
  bell: '<path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/>',
  bell_off: '<path d="M13.73 21a2 2 0 0 1-3.46 0"/><path d="M18.63 13A17.9 17.9 0 0 1 18 8a6 6 0 0 0-9.33-5"/><path d="M6.26 6.26A5.86 5.86 0 0 0 6 8c0 7-3 9-3 9h14"/><line x1="2" y1="2" x2="22" y2="22"/>',
  clock: '<circle cx="12" cy="12" r="9.5"/><polyline points="12 6.5 12 12 16 14"/>',
  flame: '<path d="M12 2.5c1.5 3.2 5.5 5.4 5.5 10.3a5.5 5.5 0 1 1-11 0c0-2.1.9-3.9 2.2-5.4.1 1.6 1 2.6 2 2.6 0-2.6.3-5.2 1.3-7.5z"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2.5M12 19.5V22M4.93 4.93l1.77 1.77M17.3 17.3l1.77 1.77M2 12h2.5M19.5 12H22M4.93 19.07l1.77-1.77M17.3 6.7l1.77-1.77"/>',
  moon: '<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>',
  plus: '<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>',
  trash: '<polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/><path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"/>',
  chevron_down: '<polyline points="6 9 12 15 18 9"/>',
  chevron_up: '<polyline points="18 15 12 9 6 15"/>',
  chevron_left: '<polyline points="15 18 9 12 15 6"/>',
  chevron_right: '<polyline points="9 18 15 12 9 6"/>',
  rotate_ccw: '<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><polyline points="3 3 3 8 8 8"/>',
  palette: '<circle cx="13.5" cy="6.5" r="1" fill="currentColor" stroke="none"/><circle cx="17.5" cy="10.5" r="1" fill="currentColor" stroke="none"/><circle cx="8.5" cy="7.5" r="1" fill="currentColor" stroke="none"/><circle cx="6.5" cy="12.5" r="1" fill="currentColor" stroke="none"/><path d="M12 2a10 10 0 0 0 0 20h1.5a2.5 2.5 0 0 0 2.5-2.5c0-.66-.27-1.26-.7-1.7a1.5 1.5 0 0 1 1.1-2.5H18a4 4 0 0 0 4-4C22 6.03 17.52 2 12 2z"/>',
};

const ICON_ALIASES = {
  'mic-off': 'mic_off',
  'volume-x': 'volume_off',
  'volume-off': 'volume_off',
  'volume-1': 'volume_down',
  'volume-down': 'volume_down',
  'volume-2': 'volume_up',
  'volume-up': 'volume_up',
  volume: 'volume_up',
  camera: 'screenshot',
  'exit-fullscreen': 'exit_fullscreen',
  'chevron-down': 'chevron_down',
  'chevron-up': 'chevron_up',
  'chevron-left': 'chevron_left',
  'chevron-right': 'chevron_right',
  'bell-off': 'bell_off',
  'rotate-ccw': 'rotate_ccw',
  calendar: 'agenda',
  cpu: 'pc',
  sparkles: 'ai',
  cloud: 'weather',
  grid: 'shortcuts',
  'skip-back': 'prev',
  'skip-forward': 'next',
};

function sanitizeSize(size, fallback) {
  const n = Number(size);
  return Number.isFinite(n) && n > 0 ? Math.round(n) : fallback;
}

export function icon(name, size = 22) {
  const s = sanitizeSize(size, 22);
  const key = String(name || '').trim().toLowerCase();
  const resolved = ICON_ALIASES[key] || key.replace(/-/g, '_');
  const inner = ICON_PATHS[resolved] || ICON_PATHS.zap;
  return `<svg width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${inner}</svg>`;
}

export function weatherIcon(code, size = 44) {
  const s = sanitizeSize(size, 44);
  const c = Number(code);
  const wrap = (inner) =>
    `<svg width="${s}" height="${s}" viewBox="0 0 48 48" fill="none" aria-hidden="true" focusable="false">${inner}</svg>`;

  // 0: Clear sky (Sun)
  if (c === 0) {
    return wrap(
      '<circle cx="24" cy="24" r="9.5" fill="#fbbf24" stroke="#f59e0b" stroke-width="1.5"/>' +
      '<g stroke="#fbbf24" stroke-width="2.6" stroke-linecap="round">' +
      '<line x1="24" y1="5" x2="24" y2="10"/>' +
      '<line x1="24" y1="38" x2="24" y2="43"/>' +
      '<line x1="5" y1="24" x2="10" y2="24"/>' +
      '<line x1="38" y1="24" x2="43" y2="24"/>' +
      '<line x1="10.6" y1="10.6" x2="14.1" y2="14.1"/>' +
      '<line x1="33.9" y1="33.9" x2="37.4" y2="37.4"/>' +
      '<line x1="37.4" y1="10.6" x2="33.9" y2="14.1"/>' +
      '<line x1="14.1" y1="33.9" x2="10.6" y2="37.4"/>' +
      '</g>'
    );
  }

  // 1-2: Mainly clear / Partly cloudy
  if (c === 1 || c === 2) {
    return wrap(
      '<circle cx="19" cy="18" r="7" fill="#fbbf24"/>' +
      '<g stroke="#fbbf24" stroke-width="2.2" stroke-linecap="round">' +
      '<line x1="19" y1="6" x2="19" y2="8.8"/>' +
      '<line x1="7" y1="18" x2="9.8" y2="18"/>' +
      '<line x1="10.5" y1="9.5" x2="12.6" y2="11.6"/>' +
      '<line x1="27.5" y1="9.5" x2="25.4" y2="11.6"/>' +
      '</g>' +
      '<path d="M16 36h18.5a6.5 6.5 0 0 0 1.2-12.9A8.5 8.5 0 0 0 19.3 20a6 6 0 0 0-3.3 16z" fill="#cbd5e1" stroke="#94a3b8" stroke-width="1.2"/>'
    );
  }

  // 3: Overcast
  if (c === 3) {
    return wrap(
      '<path d="M14 29h17a5.5 5.5 0 0 0 .8-10.9A7.5 7.5 0 0 0 17.5 15a5 5 0 0 0-3.5 14z" fill="#64748b"/>' +
      '<path d="M13 37h21a6.5 6.5 0 0 0 1.1-12.9A8.5 8.5 0 0 0 18.8 21a6 6 0 0 0-5.8 16z" fill="#94a3b8" stroke="#cbd5e1" stroke-width="1"/>'
    );
  }

  // 45, 48: Fog
  if (c === 45 || c === 48) {
    return wrap(
      '<path d="M12 28h23a5.5 5.5 0 0 0 1-10.9A8 8 0 0 0 20.5 14a5.5 5.5 0 0 0-8.5 14z" fill="#94a3b8"/>' +
      '<g stroke="#cbd5e1" stroke-width="2.4" stroke-linecap="round" opacity="0.85">' +
      '<line x1="10" y1="33" x2="36" y2="33"/>' +
      '<line x1="14" y1="38" x2="38" y2="38"/>' +
      '<line x1="8" y1="43" x2="30" y2="43"/>' +
      '</g>'
    );
  }

  // 95-99: Thunderstorm
  if (c >= 95 && c <= 99) {
    return wrap(
      '<path d="M12 28h23a6 6 0 0 0 1-11.9A8.5 8.5 0 0 0 19.6 12.5a6 6 0 0 0-7.6 15.5z" fill="#64748b" stroke="#94a3b8" stroke-width="1.2"/>' +
      '<polygon points="24 24 18.5 34 23.5 34 21 43 30 31.5 25 31.5 27.5 24" fill="#fbbf24"/>' +
      '<g stroke="#60a5fa" stroke-width="2.2" stroke-linecap="round">' +
      '<line x1="14" y1="32" x2="12" y2="37"/>' +
      '<line x1="34" y1="32" x2="32" y2="37"/>' +
      '</g>'
    );
  }

  // 71-77, 85-86: Snow
  if ((c >= 71 && c <= 77) || c === 85 || c === 86) {
    return wrap(
      '<path d="M12 28h23a6 6 0 0 0 1-11.9A8.5 8.5 0 0 0 19.6 12.5a6 6 0 0 0-7.6 15.5z" fill="#94a3b8" stroke="#cbd5e1" stroke-width="1.2"/>' +
      '<g fill="#e0f2fe">' +
      '<circle cx="16" cy="34" r="2.2"/>' +
      '<circle cx="24" cy="36" r="2.2"/>' +
      '<circle cx="32" cy="34" r="2.2"/>' +
      '<circle cx="20" cy="41" r="1.9"/>' +
      '<circle cx="28" cy="41" r="1.9"/>' +
      '</g>'
    );
  }

  // 51-67, 80-82: Drizzle / Rain / Showers
  if ((c >= 51 && c <= 67) || (c >= 80 && c <= 82)) {
    return wrap(
      '<path d="M12 28h23a6 6 0 0 0 1-11.9A8.5 8.5 0 0 0 19.6 12.5a6 6 0 0 0-7.6 15.5z" fill="#94a3b8" stroke="#cbd5e1" stroke-width="1.2"/>' +
      '<g stroke="#60a5fa" stroke-width="2.5" stroke-linecap="round">' +
      '<line x1="16" y1="32" x2="13.5" y2="39"/>' +
      '<line x1="24" y1="32" x2="21.5" y2="41"/>' +
      '<line x1="32" y1="32" x2="29.5" y2="39"/>' +
      '</g>'
    );
  }

  // Fallback: Soft cloud
  return wrap(
    '<path d="M12 34h23a6.5 6.5 0 0 0 1.1-12.9A8.5 8.5 0 0 0 19.6 17.5a6 6 0 0 0-7.6 16.5z" fill="#94a3b8" stroke="#cbd5e1" stroke-width="1.2"/>'
  );
}
