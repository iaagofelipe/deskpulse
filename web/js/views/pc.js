let activeSubTab = 'pc';
let mountedEl = null;
let lastToolbarSig = '';
let lastPanelSig = '';
let speedtestInFlight = false;
let speedtestError = '';
let speedtestErrTimer = null;
let latestCtx = null;

function formatRate(bps, formatBytes) {
  const n = Number(bps);
  if (!Number.isFinite(n) || n <= 0) return '0 B/s';
  return `${formatBytes(n)}/s`;
}

function formatDropTime(ms) {
  const n = Number(ms);
  if (!Number.isFinite(n) || n <= 0) return 'Sem quedas';
  const d = new Date(n);
  if (Number.isNaN(d.getTime())) return 'Sem quedas';
  const now = new Date();
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  if (sameDay) {
    return `${hh}:${mm}`;
  }
  const dd = String(d.getDate()).padStart(2, '0');
  const mo = String(d.getMonth() + 1).padStart(2, '0');
  return `${dd}/${mo} ${hh}:${mm}`;
}

function renderUnavailableGauge(label, sub, color, escapeHtml) {
  const safeLabel = escapeHtml(label);
  const safeSub = escapeHtml(sub);
  return `<svg class="gauge-svg" viewBox="0 0 132 120" role="img" aria-label="${safeLabel} N/A">
    <path d="M 26.16 85.00 A 46 46 0 1 1 105.84 85.00" fill="none" stroke="var(--track)" stroke-width="9" stroke-linecap="round"/>
    <text x="66" y="65" text-anchor="middle" fill="var(--muted)" font-size="21" font-weight="700" class="num">N/A</text>
    <text x="66" y="84" text-anchor="middle" fill="${color}" opacity="0.7" font-size="10" font-weight="700" letter-spacing="0.08em">${safeLabel.toUpperCase()}</text>
    <text x="66" y="109" text-anchor="middle" fill="var(--muted)" font-size="10">${safeSub}</text>
  </svg>`;
}

function summarizeDisks(disks) {
  if (!Array.isArray(disks) || !disks.length) return 'Disco: --';
  const primary =
    disks.find((d) => /^c:/i.test(String(d?.name || ''))) ||
    disks.find((d) => String(d?.name || '') === '/') ||
    disks[0];
  if (!primary || !(Number(primary.total_gb) > 0)) return 'Disco: --';
  const used = Number(primary.used_gb || 0);
  const total = Number(primary.total_gb || 1);
  const pct = Math.min(100, Math.max(0, Math.round((used / total) * 100)));
  const cleanName = String(primary.name || 'Disco').replace(/\\+$/, '');
  return `Disco ${cleanName} ${pct}% (${Math.round(used)}/${Math.round(total)} GB)`;
}

function renderPcSubView(ctx) {
  const { state, charts, icons, escapeHtml, formatBytes } = ctx;
  const pc = state?.pc || null;

  if (!pc) {
    return {
      sig: 'pc:loading',
      html: `
        <div class="card empty-state" style="flex:1">
          <div class="empty-state-title">Aguardando telemetria do PC…</div>
          <div class="empty-state-sub">Os dados de CPU, GPU, RAM e rede aparecerão assim que o stream conectar.</div>
        </div>`,
    };
  }

  const cpuPct = Number(pc.cpu_pct || 0);
  const gpu = pc.gpu || null;
  const hasGpu = Boolean(gpu && (gpu.util_pct !== null && gpu.util_pct !== undefined));
  const gpuPct = hasGpu ? Number(gpu.util_pct) : null;
  const gpuTemp = gpu && gpu.temp_c !== null && gpu.temp_c !== undefined && Number.isFinite(Number(gpu.temp_c))
    ? `${Math.round(Number(gpu.temp_c))}°C`
    : '';

  const ramUsed = Number(pc.ram_used_gb || 0);
  const ramTotal = Number(pc.ram_total_gb || 0);
  const ramPct = ramTotal > 0 ? Math.min(100, Math.max(0, (ramUsed / ramTotal) * 100)) : 0;
  const ramSub = ramTotal > 0 ? `${ramUsed.toFixed(1)} / ${Math.round(ramTotal)} GB` : '--';

  const hasVram = Boolean(gpu && Number(gpu.vram_total_mb) > 0);
  const vramUsedGb = hasVram ? Number(gpu.vram_used_mb || 0) / 1024 : 0;
  const vramTotalGb = hasVram ? Number(gpu.vram_total_mb || 0) / 1024 : 0;
  const vramPct = hasVram ? Math.min(100, Math.max(0, (vramUsedGb / vramTotalGb) * 100)) : null;
  const vramSub = hasVram ? `${vramUsedGb.toFixed(1)} / ${vramTotalGb.toFixed(1)} GB` : 'N/A';

  const cpuHist = Array.isArray(pc.cpu_history) ? pc.cpu_history : [];
  const gpuHist = hasGpu && Array.isArray(pc.gpu_history) ? pc.gpu_history : [];
  const downRate = formatRate(pc.net_down_bps, formatBytes);
  const upRate = formatRate(pc.net_up_bps, formatBytes);
  const diskSummary = summarizeDisks(pc.disks);
  const bat = icons ? icons.describeBattery(pc.battery) : null;
  const hardwareFooter = gpu?.name
    ? `${pc.cpu_name || 'CPU'} · ${gpu.name}`
    : (pc.cpu_name || 'CPU');

  const sig = [
    'pc',
    cpuPct.toFixed(1),
    gpuPct !== null ? gpuPct.toFixed(1) : 'na',
    gpuTemp,
    ramPct.toFixed(1),
    ramSub,
    vramPct !== null ? vramPct.toFixed(1) : 'na',
    vramSub,
    bat ? bat.sig : 'nobat',
    downRate,
    upRate,
    diskSummary,
    hardwareFooter,
    cpuHist.join(','),
    gpuHist.join(','),
  ].join('|');

  const cpuGauge = charts.renderArcGauge({
    value: cpuPct,
    max: 100,
    label: 'CPU',
    sub: pc.pc_locked ? 'Bloqueado' : 'Global',
    color: 'var(--cpu)',
    unit: '%',
  });

  const gpuGauge = hasGpu
    ? charts.renderArcGauge({
        value: gpuPct,
        max: 100,
        label: 'GPU',
        sub: gpuTemp || 'NVIDIA',
        color: 'var(--gpu)',
        unit: '%',
      })
    : renderUnavailableGauge('GPU', 'Sem NVIDIA', 'var(--gpu)', escapeHtml);

  const ramGauge = charts.renderArcGauge({
    value: ramPct,
    max: 100,
    label: 'RAM',
    sub: ramSub,
    color: 'var(--ram)',
    unit: '%',
  });

  const vramGauge = hasVram
    ? charts.renderArcGauge({
        value: vramPct,
        max: 100,
        label: 'VRAM',
        sub: vramSub,
        color: 'var(--vram)',
        unit: '%',
      })
    : renderUnavailableGauge('VRAM', 'Indisponível', 'var(--vram)', escapeHtml);

  const chartSvg = charts.renderDualLineChart({
    seriesA: cpuHist,
    seriesB: gpuHist,
    maxY: 100,
    colorA: 'var(--cpu)',
    colorB: 'var(--gpu)',
    height: 84,
    lossMarkers: false,
  });

  const batteryCard = bat
    ? `<div class="gauge-card battery-spec-card is-${bat.tone}" title="${escapeHtml(bat.title)}" aria-label="${escapeHtml(bat.title)}">
        <div class="battery-spec-icon">
          ${icons.batteryIcon({
            percent: bat.percent,
            charging: bat.charging,
            acPlugged: bat.acPlugged,
            height: 26,
          })}
        </div>
        <div class="battery-spec-val num">${bat.percent}<span class="battery-spec-unit">%</span></div>
        <div class="gauge-label">BATERIA</div>
        <div class="gauge-sub battery-spec-sub" title="${escapeHtml(bat.statusText)}">${escapeHtml(bat.statusText)}</div>
      </div>`
    : '';

  const html = `
    <div class="gauge-grid${bat ? ' cols-5' : ''}">
      <div class="gauge-card">${cpuGauge}</div>
      <div class="gauge-card">${gpuGauge}</div>
      <div class="gauge-card">${ramGauge}</div>
      <div class="gauge-card">${vramGauge}</div>
      ${batteryCard}
    </div>
    <div class="card chart-card">
      <div class="card-head">
        <span class="card-label">ÚLTIMO MINUTO</span>
        <div class="chart-legend">
          <span class="legend-item"><span class="legend-dot" style="background:var(--cpu)"></span>CPU</span>
          ${hasGpu ? '<span class="legend-item"><span class="legend-dot" style="background:var(--gpu)"></span>GPU</span>' : ''}
          <span class="num" style="color:var(--green)">↓ ${escapeHtml(downRate)}</span>
          <span class="num" style="color:var(--muted)">↑ ${escapeHtml(upRate)}</span>
        </div>
      </div>
      <div class="chart-wrap">${chartSvg}</div>
      <div class="pc-footer-meta">
        <span style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title="${escapeHtml(hardwareFooter)}">${escapeHtml(hardwareFooter)}</span>
        <span class="num" style="flex-shrink:0">${escapeHtml(diskSummary)}</span>
      </div>
    </div>`;

  return { sig, html };
}

function renderInternetSubView(ctx) {
  const { state, charts, escapeHtml, formatBytes, formatDuration } = ctx;
  const net = state?.network || null;

  const online = Boolean(net?.online);
  const latency = net?.latency_ms !== null && net?.latency_ms !== undefined ? Number(net.latency_ms) : null;
  const jitter = net?.jitter_ms !== null && net?.jitter_ms !== undefined ? Number(net.jitter_ms) : null;
  const lossPct = Number(net?.loss_pct || 0);
  const speed = net?.speed_down_mbps !== null && net?.speed_down_mbps !== undefined
    ? Number(net.speed_down_mbps)
    : null;
  const testing = Boolean(net?.testing) || speedtestInFlight;
  const history = Array.isArray(net?.history_ms) ? net.history_ms : [];

  const uptimeStr = online && net?.up_since_ms
    ? formatDuration(Math.max(0, (Date.now() - Number(net.up_since_ms)) / 1000))
    : (online ? 'Conectado' : 'Offline');
  const lastDropStr = formatDropTime(net?.last_drop_ms);
  const todayStr = formatBytes(net?.today_bytes || 0);
  const monthStr = formatBytes(net?.month_bytes || 0);

  const latencyColor = latency === null
    ? 'var(--muted)'
    : (latency < 50 ? 'var(--green)' : (latency < 120 ? 'var(--amber)' : 'var(--red)'));
  const lossColor = lossPct > 0 ? 'var(--red)' : 'var(--green)';

  const sig = [
    'internet',
    online ? 1 : 0,
    latency,
    jitter,
    lossPct.toFixed(1),
    speed !== null ? speed.toFixed(1) : 'na',
    testing ? 1 : 0,
    speedtestError,
    uptimeStr,
    lastDropStr,
    todayStr,
    monthStr,
    history.join(','),
  ].join('|');

  const chartSvg = charts.renderDualLineChart({
    seriesA: history,
    seriesB: [],
    maxY: 100,
    colorA: 'var(--cpu)',
    height: 84,
    lossMarkers: true,
  });

  const errorBanner = speedtestError
    ? `<div class="badge badge-warn" style="align-self:flex-start;padding:5px 10px" role="status">${escapeHtml(speedtestError)}</div>`
    : '';

  const html = `
    ${errorBanner}
    <div class="net-kpis">
      <div class="net-kpi-card">
        <span class="card-label">Latência</span>
        <div class="net-kpi-val num" style="color:${latencyColor}">${latency !== null ? `${latency}<span style="font-size:12px;color:var(--muted);font-weight:600"> ms</span>` : '--'}</div>
      </div>
      <div class="net-kpi-card">
        <span class="card-label">Jitter</span>
        <div class="net-kpi-val num">${jitter !== null ? `${jitter}<span style="font-size:12px;color:var(--muted);font-weight:600"> ms</span>` : '--'}</div>
      </div>
      <div class="net-kpi-card">
        <span class="card-label">Perda</span>
        <div class="net-kpi-val num" style="color:${lossColor}">${lossPct.toFixed(1)}<span style="font-size:12px;color:var(--muted);font-weight:600">%</span></div>
      </div>
      <div class="net-kpi-card">
        <span class="card-label">Velocidade</span>
        <div class="net-kpi-val num" style="color:var(--gpu)">${testing ? '…' : (speed !== null ? `${speed.toFixed(1)}<span style="font-size:12px;color:var(--muted);font-weight:600"> Mbps</span>` : '--')}</div>
      </div>
    </div>
    <div class="card chart-card">
      <div class="card-head">
        <span class="card-label">LATÊNCIA · ÚLTIMAS 60 SONDAS</span>
        <div class="chart-legend">
          <span class="legend-item"><span class="legend-dot" style="background:var(--cpu)"></span>ms</span>
          <span class="legend-item"><span class="legend-dot" style="background:var(--red)"></span>Perda</span>
        </div>
      </div>
      <div class="chart-wrap">${chartSvg}</div>
      <div class="pc-footer-meta">
        <span class="num">Uptime: <strong style="color:var(--text-2)">${escapeHtml(uptimeStr)}</strong> · Queda: <strong style="color:var(--text-2)">${escapeHtml(lastDropStr)}</strong></span>
        <span class="num">Hoje: <strong style="color:var(--text-2)">${escapeHtml(todayStr)}</strong> · Mês: <strong style="color:var(--text-2)">${escapeHtml(monthStr)}</strong></span>
      </div>
    </div>`;

  return { sig, html };
}

function renderToolbarRight(ctx) {
  const { state, icons, escapeHtml, formatDuration } = ctx;
  if (activeSubTab === 'pc') {
    const up = state?.pc?.uptime_sec;
    const upText = Number.isFinite(Number(up)) && Number(up) > 0
      ? `Uptime ${formatDuration(up)}`
      : 'PC';
    const saver = Boolean(state?.pc?.battery?.battery_saver);
    return {
      sig: `tb-pc|${upText}|${saver ? 1 : 0}`,
      html: `
        <div class="pc-toolbar-badges">
          ${saver ? '<span class="badge badge-warn">Economia</span>' : ''}
          <span class="badge num">${icons.icon('clock', 12)}<span>${escapeHtml(upText)}</span></span>
        </div>`,
    };
  }

  const net = state?.network || null;
  const online = Boolean(net?.online);
  const testing = Boolean(net?.testing) || speedtestInFlight;
  return {
    sig: `tb-net|${online ? 1 : 0}|${testing ? 1 : 0}`,
    html: `
      <div style="display:flex;align-items:center;gap:8px">
        <span class="badge ${online ? 'badge-ok' : 'badge-err'}">${icons.icon('wifi', 12)}<span>${online ? 'Online' : 'Offline'}</span></span>
        <button type="button" class="speedtest-btn ${testing ? 'is-loading' : ''}" data-action="speedtest" ${testing ? 'disabled' : ''}>
          ${icons.icon('zap', 14)}
          <span>${testing ? 'Testando…' : 'Testar velocidade'}</span>
        </button>
      </div>`,
  };
}

function setSpeedtestError(msg) {
  speedtestError = msg;
  if (speedtestErrTimer) {
    clearTimeout(speedtestErrTimer);
    speedtestErrTimer = null;
  }
  if (msg) {
    speedtestErrTimer = setTimeout(() => {
      speedtestError = '';
      speedtestErrTimer = null;
      if (latestCtx && latestCtx.activeTab === 'pc') {
        render(latestCtx);
      }
    }, 6000);
  }
}

async function handleSpeedtest() {
  if (!latestCtx || speedtestInFlight) return;
  speedtestInFlight = true;
  setSpeedtestError('');
  render(latestCtx);

  try {
    const snap = await latestCtx.apiRequest('POST', '/api/network/speedtest');
    if (latestCtx.state && snap && typeof snap === 'object') {
      latestCtx.state.network = snap;
    }
  } catch (err) {
    const friendly = (err && err.message) || 'Não foi possível iniciar o teste de velocidade.';
    setSpeedtestError(friendly);
  } finally {
    speedtestInFlight = false;
    if (latestCtx) {
      render(latestCtx);
    }
  }
}

function ensureShell(mount) {
  if (mountedEl === mount && mount.querySelector('.pc-layout')) {
    return;
  }
  mountedEl = mount;
  lastToolbarSig = '';
  lastPanelSig = '';

  mount.innerHTML = `
    <div class="pc-layout">
      <div class="pc-toolbar">
        <div class="segmented" role="tablist" aria-label="Sub-aba de PC e Internet">
          <button type="button" class="seg-btn active" role="tab" aria-selected="true" data-subtab="pc">PC</button>
          <button type="button" class="seg-btn" role="tab" aria-selected="false" data-subtab="internet">Internet</button>
        </div>
        <div data-pc-toolbar-right></div>
      </div>
      <div class="pc-panel" data-pc-panel></div>
    </div>`;

  mount.addEventListener('click', (ev) => {
    const segBtn = ev.target.closest('.seg-btn[data-subtab]');
    if (segBtn) {
      const next = segBtn.dataset.subtab;
      if (next && next !== activeSubTab) {
        activeSubTab = next;
        lastToolbarSig = '';
        lastPanelSig = '';
        if (latestCtx) render(latestCtx);
      }
      return;
    }

    const speedBtn = ev.target.closest('[data-action="speedtest"]');
    if (speedBtn) {
      handleSpeedtest();
    }
  });
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;
  latestCtx = ctx;

  ensureShell(mount);

  mount.querySelectorAll('.seg-btn[data-subtab]').forEach((btn) => {
    const active = btn.dataset.subtab === activeSubTab;
    btn.classList.toggle('active', active);
    btn.setAttribute('aria-selected', active ? 'true' : 'false');
  });

  const tbRight = mount.querySelector('[data-pc-toolbar-right]');
  if (tbRight) {
    const tb = renderToolbarRight(ctx);
    if (tb.sig !== lastToolbarSig) {
      lastToolbarSig = tb.sig;
      tbRight.innerHTML = tb.html;
    }
  }

  const panel = mount.querySelector('[data-pc-panel]');
  if (!panel) return;

  const view = activeSubTab === 'pc' ? renderPcSubView(ctx) : renderInternetSubView(ctx);
  if (view.sig !== lastPanelSig) {
    lastPanelSig = view.sig;
    panel.innerHTML = view.html;
  }
}
