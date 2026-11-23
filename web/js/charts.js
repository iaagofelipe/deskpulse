let _uid = 0;

function esc(value) {
  return String(value ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function safeColor(color, fallback) {
  const s = String(color || '').trim();
  if (/^(#[0-9a-fA-F]{3,8}|var\(--[a-zA-Z0-9_-]+\)|rgba?\([0-9.,\s%]+\)|hsla?\([0-9.,\s%deg]+\)|[a-zA-Z]+)$/.test(s)) {
    return s;
  }
  return fallback;
}

function clamp(n, min, max) {
  return Math.max(min, Math.min(max, n));
}

function polarToCartesian(cx, cy, r, angleDeg) {
  const rad = (angleDeg * Math.PI) / 180;
  return {
    x: cx + r * Math.cos(rad),
    y: cy + r * Math.sin(rad),
  };
}

function describeArc(cx, cy, r, startAngle, endAngle) {
  const start = polarToCartesian(cx, cy, r, startAngle);
  const end = polarToCartesian(cx, cy, r, endAngle);
  const sweep = endAngle - startAngle;
  const largeArcFlag = sweep > 180 ? 1 : 0;
  return `M ${start.x.toFixed(2)} ${start.y.toFixed(2)} A ${r} ${r} 0 ${largeArcFlag} 1 ${end.x.toFixed(2)} ${end.y.toFixed(2)}`;
}

function roundedTopRectPath(x, y, w, h, r) {
  if (h <= 0 || w <= 0) return '';
  const radius = Math.min(r, w / 2, h);
  return [
    `M ${x.toFixed(2)} ${(y + h).toFixed(2)}`,
    `L ${x.toFixed(2)} ${(y + radius).toFixed(2)}`,
    `Q ${x.toFixed(2)} ${y.toFixed(2)} ${(x + radius).toFixed(2)} ${y.toFixed(2)}`,
    `L ${(x + w - radius).toFixed(2)} ${y.toFixed(2)}`,
    `Q ${(x + w).toFixed(2)} ${y.toFixed(2)} ${(x + w).toFixed(2)} ${(y + radius).toFixed(2)}`,
    `L ${(x + w).toFixed(2)} ${(y + h).toFixed(2)}`,
    'Z',
  ].join(' ');
}

function buildSmoothSegments(points) {
  if (!points.length) return '';
  if (points.length === 1) {
    const p = points[0];
    return `M ${p.x.toFixed(2)} ${p.y.toFixed(2)} L ${(p.x + 0.1).toFixed(2)} ${p.y.toFixed(2)}`;
  }
  let d = `M ${points[0].x.toFixed(2)} ${points[0].y.toFixed(2)}`;
  for (let i = 1; i < points.length; i++) {
    const prev = points[i - 1];
    const curr = points[i];
    const mx = (prev.x + curr.x) / 2;
    const my = (prev.y + curr.y) / 2;
    d += ` Q ${prev.x.toFixed(2)} ${prev.y.toFixed(2)} ${mx.toFixed(2)} ${my.toFixed(2)}`;
  }
  const last = points[points.length - 1];
  d += ` L ${last.x.toFixed(2)} ${last.y.toFixed(2)}`;
  return d;
}

function localTodayIso() {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${day}`;
}

export function renderArcGauge({
  value = 0,
  max = 100,
  label = '',
  sub = '',
  color = 'var(--cpu)',
  unit = '%',
} = {}) {
  const safeCol = safeColor(color, 'var(--cpu)');
  const numVal = Number(value);
  const valid = value !== null && value !== undefined && Number.isFinite(numVal);
  const safeMax = Number.isFinite(Number(max)) && Number(max) > 0 ? Number(max) : 100;
  const ratio = valid ? clamp(numVal / safeMax, 0, 1) : 0;

  const cx = 66;
  const cy = 62;
  const r = 46;
  const startAngle = 150;
  const totalSweep = 240;
  const endAngle = startAngle + totalSweep;
  const valAngle = startAngle + totalSweep * ratio;

  const trackPath = describeArc(cx, cy, r, startAngle, endAngle);
  const valuePath = ratio > 0.004 ? describeArc(cx, cy, r, startAngle, valAngle) : '';

  const formattedVal = valid
    ? (Math.abs(numVal) >= 100 || Number.isInteger(numVal) ? Math.round(numVal) : numVal.toFixed(1))
    : '—';
  const unitText = valid && unit ? esc(unit) : '';
  const safeLabel = esc(label);
  const safeSub = esc(sub);

  return `<svg class="gauge-svg" viewBox="0 0 132 120" role="img" aria-label="${safeLabel} ${esc(formattedVal)}${unitText}">
  <path d="${trackPath}" fill="none" stroke="rgba(255,255,255,0.09)" stroke-width="9" stroke-linecap="round"/>
  ${valuePath ? `<path d="${valuePath}" fill="none" stroke="${safeCol}" stroke-width="9" stroke-linecap="round"/>` : ''}
  <text x="${cx}" y="65" text-anchor="middle" fill="var(--text)" font-size="23" font-weight="700" class="num" style="font-variant-numeric:tabular-nums">${esc(formattedVal)}${unitText ? `<tspan font-size="12" font-weight="600" fill="var(--muted)" dx="1">${unitText}</tspan>` : ''}</text>
  ${safeLabel ? `<text x="${cx}" y="84" text-anchor="middle" fill="var(--muted)" font-size="10" font-weight="700" letter-spacing="0.08em">${safeLabel.toUpperCase()}</text>` : ''}
  ${safeSub ? `<text x="${cx}" y="109" text-anchor="middle" fill="var(--text-2)" font-size="10">${safeSub}</text>` : ''}
</svg>`;
}

export function renderDualLineChart({
  seriesA = [],
  seriesB = [],
  maxY = 100,
  colorA = 'var(--cpu)',
  colorB = 'var(--gpu)',
  height = 84,
  lossMarkers = false,
} = {}) {
  const h = Number.isFinite(Number(height)) && Number(height) >= 36 ? Math.round(Number(height)) : 84;
  const w = 420;
  const padT = 8;
  const padB = lossMarkers ? 14 : 8;
  const padL = 4;
  const padR = 4;
  const plotW = w - padL - padR;
  const plotH = h - padT - padB;

  const colA = safeColor(colorA, 'var(--cpu)');
  const colB = safeColor(colorB, 'var(--gpu)');

  const a = Array.isArray(seriesA) ? seriesA : [];
  const b = Array.isArray(seriesB) ? seriesB : [];

  let dataMax = 0;
  for (const v of a) {
    const n = Number(v);
    if (Number.isFinite(n) && n > dataMax) dataMax = n;
  }
  for (const v of b) {
    const n = Number(v);
    if (Number.isFinite(n) && n > dataMax) dataMax = n;
  }

  let effectiveMax = Number(maxY);
  if (!Number.isFinite(effectiveMax) || effectiveMax <= 0) {
    effectiveMax = Math.max(10, dataMax * 1.15);
  } else if (lossMarkers && dataMax > effectiveMax) {
    effectiveMax = Math.ceil((dataMax * 1.15) / 10) * 10;
  }

  const xAt = (idx, len) => {
    const denom = Math.max(len - 1, 1);
    return padL + (idx / denom) * plotW;
  };
  const yAt = (val) => {
    const ratio = clamp(val / effectiveMax, 0, 1);
    return padT + plotH - ratio * plotH;
  };

  const guides = [0, 0.5, 1].map((frac) => {
    const y = (padT + plotH * frac).toFixed(2);
    return `<line x1="${padL}" y1="${y}" x2="${w - padR}" y2="${y}" stroke="rgba(255,255,255,0.07)" stroke-width="1" stroke-dasharray="${frac === 1 ? '' : '3 4'}"/>`;
  }).join('');

  const buildSeriesSvg = (arr, col, withArea, drawLoss) => {
    if (!arr.length) return '';
    const segments = [];
    const losses = [];
    let current = [];

    for (let i = 0; i < arr.length; i++) {
      const raw = arr[i];
      const n = raw === null || raw === undefined ? -1 : Number(raw);
      const isLoss = !Number.isFinite(n) || (drawLoss && n < 0);
      if (isLoss) {
        if (current.length) {
          segments.push(current);
          current = [];
        }
        if (drawLoss) {
          losses.push(xAt(i, arr.length));
        }
      } else {
        current.push({ x: xAt(i, arr.length), y: yAt(Math.max(0, n)) });
      }
    }
    if (current.length) segments.push(current);

    let out = '';
    let gradId = '';
    if (withArea && segments.length) {
      gradId = `dp_grad_${++_uid}`;
      out += `<defs><linearGradient id="${gradId}" x1="0" y1="0" x2="0" y2="1">` +
        `<stop offset="0%" stop-color="${col}" stop-opacity="0.26"/>` +
        `<stop offset="100%" stop-color="${col}" stop-opacity="0.01"/>` +
        `</linearGradient></defs>`;
    }

    const baseY = (padT + plotH).toFixed(2);
    for (const seg of segments) {
      const linePath = buildSmoothSegments(seg);
      if (withArea && seg.length > 1) {
        const firstX = seg[0].x.toFixed(2);
        const lastX = seg[seg.length - 1].x.toFixed(2);
        out += `<path d="${linePath} L ${lastX} ${baseY} L ${firstX} ${baseY} Z" fill="url(#${gradId})"/>`;
      }
      out += `<path d="${linePath}" fill="none" stroke="${col}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke"/>`;
    }

    if (drawLoss && losses.length) {
      const markerY = padT + plotH + 5;
      for (const lx of losses) {
        const xf = lx.toFixed(2);
        out += `<line x1="${xf}" y1="${padT}" x2="${xf}" y2="${(padT + plotH).toFixed(2)}" stroke="rgba(239,68,68,0.32)" stroke-width="1.2" stroke-dasharray="2 2"/>`;
        out += `<circle cx="${xf}" cy="${markerY.toFixed(2)}" r="2.6" fill="var(--red)"/>`;
      }
    }
    return out;
  };

  return `<svg viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" role="img" aria-label="Gráfico de tendência">
  ${guides}
  ${buildSeriesSvg(b, colB, false, false)}
  ${buildSeriesSvg(a, colA, true, lossMarkers)}
</svg>`;
}

export function renderBarChart({
  items = [],
  color = 'var(--ai)',
  height = 110,
} = {}) {
  const h = Number.isFinite(Number(height)) && Number(height) >= 48 ? Math.round(Number(height)) : 110;
  const w = 320;
  const col = safeColor(color, 'var(--ai)');
  const list = Array.isArray(items) ? items : [];

  const padT = 12;
  const padB = 22;
  const padL = 8;
  const padR = 8;
  const plotW = w - padL - padR;
  const plotH = h - padT - padB;
  const baseY = padT + plotH;

  const guides = [0, 0.5, 1].map((frac) => {
    const y = (padT + plotH * frac).toFixed(2);
    return `<line x1="${padL}" y1="${y}" x2="${w - padR}" y2="${y}" stroke="rgba(255,255,255,0.07)" stroke-width="1"/>`;
  }).join('');

  if (!list.length) {
    return `<svg viewBox="0 0 ${w} ${h}" role="img" aria-label="Gráfico de barras">${guides}</svg>`;
  }

  const values = list.map((it) => {
    const v = Number(it?.value ?? it?.total_tokens ?? 0);
    return Number.isFinite(v) && v > 0 ? v : 0;
  });
  const maxVal = Math.max(1, ...values);

  const slotW = plotW / list.length;
  const barW = clamp(slotW * 0.56, 8, 30);

  const bars = list.map((it, i) => {
    const v = values[i];
    const barH = v > 0 ? Math.max(3, (v / maxVal) * plotH) : 2;
    const cx = padL + slotW * i + slotW / 2;
    const x = cx - barW / 2;
    const y = baseY - barH;
    const isLast = i === list.length - 1;
    const opacity = v === 0 ? '0.18' : (isLast ? '1' : '0.74');
    const label = esc(it?.label ?? '');
    const path = roundedTopRectPath(x, y, barW, barH, 4);

    return `<g>
      <path d="${path}" fill="${col}" opacity="${opacity}"><title>${label}: ${esc(v)}</title></path>
      <text x="${cx.toFixed(2)}" y="${(h - 6).toFixed(2)}" text-anchor="middle" fill="${isLast ? 'var(--text-2)' : 'var(--muted)'}" font-size="10" font-weight="${isLast ? '600' : '500'}">${label}</text>
    </g>`;
  }).join('');

  return `<svg viewBox="0 0 ${w} ${h}" role="img" aria-label="Gráfico de barras">
  ${guides}
  ${bars}
</svg>`;
}

export function renderCompareBars({
  labels = [],
  current = [],
  previous = [],
  colorCurrent = 'var(--green)',
  colorPrevious = '#2b363e',
  height = 96,
} = {}) {
  const h = Number.isFinite(Number(height)) && Number(height) >= 48 ? Math.round(Number(height)) : 96;
  const w = 300;
  const colCur = safeColor(colorCurrent, 'var(--green)');
  const colPrev = safeColor(colorPrevious, '#2b363e');

  const curArr = Array.isArray(current) ? current : [];
  const prevArr = Array.isArray(previous) ? previous : [];
  const fallbackLabels = ['D', 'S', 'T', 'Q', 'Q', 'S', 'S'];
  const count = Math.max(labels.length, curArr.length, prevArr.length, 7);

  const padT = 10;
  const padB = 20;
  const padL = 6;
  const padR = 6;
  const plotW = w - padL - padR;
  const plotH = h - padT - padB;
  const baseY = padT + plotH;

  let maxVal = 1;
  for (let i = 0; i < count; i++) {
    const c = Number(curArr[i] ?? 0);
    const p = Number(prevArr[i] ?? 0);
    if (Number.isFinite(c) && c > maxVal) maxVal = c;
    if (Number.isFinite(p) && p > maxVal) maxVal = p;
  }

  const guides = [0, 0.5, 1].map((frac) => {
    const y = (padT + plotH * frac).toFixed(2);
    return `<line x1="${padL}" y1="${y}" x2="${w - padR}" y2="${y}" stroke="rgba(255,255,255,0.07)" stroke-width="1"/>`;
  }).join('');

  const slotW = plotW / count;
  const gap = 2.5;
  const barW = clamp((slotW * 0.7 - gap) / 2, 4, 14);

  let pairs = '';
  for (let i = 0; i < count; i++) {
    const pVal = Math.max(0, Number(prevArr[i] ?? 0) || 0);
    const cVal = Math.max(0, Number(curArr[i] ?? 0) || 0);
    const pH = pVal > 0 ? Math.max(3, (pVal / maxVal) * plotH) : 2;
    const cH = cVal > 0 ? Math.max(3, (cVal / maxVal) * plotH) : 2;

    const cx = padL + slotW * i + slotW / 2;
    const px = cx - gap / 2 - barW;
    const rx = cx + gap / 2;
    const lbl = esc(labels[i] ?? fallbackLabels[i % 7]);

    pairs += `<g>
      <path d="${roundedTopRectPath(px, baseY - pH, barW, pH, 3)}" fill="${colPrev}" opacity="${pVal > 0 ? '0.95' : '0.4'}"><title>${lbl} anterior: ${pVal}</title></path>
      <path d="${roundedTopRectPath(rx, baseY - cH, barW, cH, 3)}" fill="${colCur}" opacity="${cVal > 0 ? '1' : '0.25'}"><title>${lbl} atual: ${cVal}</title></path>
      <text x="${cx.toFixed(2)}" y="${(h - 5).toFixed(2)}" text-anchor="middle" fill="var(--muted)" font-size="10" font-weight="500">${lbl}</text>
    </g>`;
  }

  return `<svg viewBox="0 0 ${w} ${h}" role="img" aria-label="Comparação semanal">
  ${guides}
  ${pairs}
</svg>`;
}

const MONTH_PT = ['Jan', 'Fev', 'Mar', 'Abr', 'Mai', 'Jun', 'Jul', 'Ago', 'Set', 'Out', 'Nov', 'Dez'];

export function renderGithubHeatmap({
  weeks = [],
  compact = false,
} = {}) {
  const rawWeeks = Array.isArray(weeks) && weeks.length > 0
    ? weeks
    : Array.from({ length: 52 }, () => Array.from({ length: 7 }, () => ({ date: '', count: 0, level: 0 })));

  const numWeeks = rawWeeks.length;
  const cell = compact ? 9 : 11;
  const gap = compact ? 2.5 : 3;
  const step = cell + gap;
  const radius = compact ? 2 : 2.5;

  const leftPad = compact ? 2 : 28;
  const topPad = compact ? 2 : 16;
  const rightPad = 2;
  const bottomPad = 2;

  const width = Math.ceil(leftPad + numWeeks * step - gap + rightPad);
  const height = Math.ceil(topPad + 7 * step - gap + bottomPad);
  const todayIso = localTodayIso();

  let monthLabels = '';
  if (!compact) {
    let lastMonth = -1;
    let lastLabelX = -100;
    for (let wIdx = 0; wIdx < numWeeks; wIdx++) {
      const week = Array.isArray(rawWeeks[wIdx]) ? rawWeeks[wIdx] : [];
      const firstDated = week.find((c) => c && typeof c.date === 'string' && c.date.length >= 7);
      if (!firstDated) continue;
      const parts = firstDated.date.split('-');
      const monthIdx = Number(parts[1]) - 1;
      if (monthIdx >= 0 && monthIdx < 12 && monthIdx !== lastMonth) {
        const x = leftPad + wIdx * step;
        if (x - lastLabelX >= 28) {
          monthLabels += `<text x="${x.toFixed(1)}" y="11" fill="var(--muted)" font-size="9.5" font-weight="500">${MONTH_PT[monthIdx]}</text>`;
          lastLabelX = x;
        }
        lastMonth = monthIdx;
      }
    }
  }

  let weekdayLabels = '';
  if (!compact) {
    const marks = [
      { row: 1, text: 'Seg' },
      { row: 3, text: 'Qua' },
      { row: 5, text: 'Sex' },
    ];
    weekdayLabels = marks.map(({ row, text }) => {
      const y = topPad + row * step + cell * 0.78;
      return `<text x="0" y="${y.toFixed(1)}" fill="var(--muted)" font-size="9" font-weight="500">${text}</text>`;
    }).join('');
  }

  let rects = '';
  for (let wIdx = 0; wIdx < numWeeks; wIdx++) {
    const week = Array.isArray(rawWeeks[wIdx]) ? rawWeeks[wIdx] : [];
    for (let dIdx = 0; dIdx < week.length && dIdx < 7; dIdx++) {
      const item = week[dIdx] || {};
      const lvl = clamp(Math.round(Number(item.level) || 0), 0, 4);
      const x = leftPad + wIdx * step;
      const y = topPad + dIdx * step;
      const isToday = !compact && item.date && item.date === todayIso;
      const strokeAttr = isToday ? ' stroke="#ffffff" stroke-width="1.3"' : '';
      const tip = item.date
        ? `<title>${esc(item.date)}: ${esc(item.count ?? 0)} contribuições</title>`
        : '';
      rects += `<rect x="${x.toFixed(1)}" y="${y.toFixed(1)}" width="${cell}" height="${cell}" rx="${radius}" fill="var(--gh-${lvl})"${strokeAttr}>${tip}</rect>`;
    }
  }

  return `<svg viewBox="0 0 ${width} ${height}" role="img" aria-label="Mapa de contribuições GitHub">
  ${monthLabels}
  ${weekdayLabels}
  ${rects}
</svg>`;
}

export function renderTimerRing({
  progress = 1,
  label = '25:00',
  sub = '',
  active = false,
} = {}) {
  const ratio = clamp(Number.isFinite(Number(progress)) ? Number(progress) : 1, 0, 1);
  const cx = 110;
  const cy = 110;
  const r = 90;
  const circumference = 2 * Math.PI * r;
  const dashOffset = circumference * (1 - ratio);
  const strokeColor = active ? 'var(--green)' : (ratio <= 0 ? 'var(--amber)' : 'var(--accent)');
  const safeLabel = esc(label);
  const safeSub = esc(sub);

  return `<svg viewBox="0 0 220 220" role="img" aria-label="Timer ${safeLabel} ${safeSub}">
  ${active ? `<circle cx="${cx}" cy="${cy}" r="98" fill="none" stroke="rgba(34,197,94,0.16)" stroke-width="2"/>` : ''}
  <circle cx="${cx}" cy="${cy}" r="${r}" fill="none" stroke="rgba(255,255,255,0.08)" stroke-width="11"/>
  <circle cx="${cx}" cy="${cy}" r="${r}" fill="none" stroke="${strokeColor}" stroke-width="11" stroke-linecap="round" stroke-dasharray="${circumference.toFixed(2)}" stroke-dashoffset="${dashOffset.toFixed(2)}" transform="rotate(-90 ${cx} ${cy})" style="transition:stroke-dashoffset 220ms linear, stroke 200ms ease"/>
  <text x="${cx}" y="${safeSub ? '113' : '122'}" text-anchor="middle" fill="var(--text)" font-size="42" font-weight="700" letter-spacing="-0.03em" class="num" style="font-variant-numeric:tabular-nums">${safeLabel}</text>
  ${safeSub ? `<text x="${cx}" y="139" text-anchor="middle" fill="${active ? 'var(--green)' : 'var(--muted)'}" font-size="12" font-weight="600" letter-spacing="0.06em">${safeSub.toUpperCase()}</text>` : ''}
</svg>`;
}
