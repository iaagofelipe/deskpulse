const MONTHS_SHORT_PT = ['Jan', 'Fev', 'Mar', 'Abr', 'Mai', 'Jun', 'Jul', 'Ago', 'Set', 'Out', 'Nov', 'Dez'];
const WEEKDAY_LABELS_PT = ['D', 'S', 'T', 'Q', 'Q', 'S', 'S'];

let mountedEl = null;
let lastSig = '';

function formatBestDayLabel(raw) {
  const s = String(raw || '').trim();
  if (!s) return '';
  const m = s.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return s;
  const day = m[3];
  const monthIdx = Number(m[2]) - 1;
  const monthName = MONTHS_SHORT_PT[monthIdx];
  if (!monthName) return s;
  return `${day} ${monthName}`;
}

function buildErrorBanner(errorMsg, escapeHtml) {
  if (!errorMsg) return '';
  return `<div class="gh-error-banner" role="status">${escapeHtml(errorMsg)}</div>`;
}

function buildUnconfiguredHtml(ctx) {
  const { icons } = ctx;
  return `
    <div class="gh-layout">
      <div class="card empty-state" style="flex:1">
        <span style="color:var(--green);opacity:0.85">${icons.icon('github', 38)}</span>
        <div class="empty-state-title">GitHub não configurado</div>
        <div class="empty-state-sub">Configure <code>github.token</code> (ou variável <code>GITHUB_TOKEN</code>) em <code>config.json</code> no PC para visualizar o mapa anual de contribuições, streaks e top repositórios.</div>
      </div>
    </div>`;
}

function buildLoadingHtml(ctx, username) {
  const { icons, escapeHtml } = ctx;
  const userTag = username ? ` de @${escapeHtml(username)}` : '';
  return `
    <div class="gh-layout">
      <div class="card empty-state" style="flex:1">
        <span style="color:var(--green);opacity:0.85">${icons.icon('github', 36)}</span>
        <div class="empty-state-title">Carregando contribuições${userTag}…</div>
        <div class="empty-state-sub">Consultando a API do GitHub no PC. Os dados aparecerão automaticamente assim que a resposta chegar.</div>
      </div>
    </div>`;
}

function buildConfiguredHtml(ctx, gh) {
  const { icons, charts, escapeHtml, formatCompactNumber } = ctx;

  const username = String(gh.username || '').trim();
  const total = Math.max(0, Number(gh.total_contributions || 0));
  const commits = Math.max(0, Number(gh.commits || 0));
  const priv = Math.max(0, Number(gh.private_contributions || 0));
  const issues = Math.max(0, Number(gh.issues || 0));
  const currentStreak = Math.max(0, Number(gh.current_streak || 0));
  const longestStreak = Math.max(0, Number(gh.longest_streak || 0));
  const bestDayCount = Math.max(0, Number(gh.best_day_count || 0));
  const bestDayFormatted = formatBestDayLabel(gh.best_day_label);
  const thisWeekTotal = Math.max(0, Number(gh.this_week_total || 0));
  const lastWeekTotal = Math.max(0, Number(gh.last_week_total || 0));
  const thisWeekDays = Array.isArray(gh.this_week_days) ? gh.this_week_days : [0, 0, 0, 0, 0, 0, 0];
  const lastWeekDays = Array.isArray(gh.last_week_days) ? gh.last_week_days : [0, 0, 0, 0, 0, 0, 0];
  const topRepos = Array.isArray(gh.top_repos) ? gh.top_repos.slice(0, 4) : [];
  const weeks = Array.isArray(gh.weeks) ? gh.weeks : [];
  const errorMsg = String(gh.error || '').trim();

  const summaryText = `${formatCompactNumber(total)} contribuições • ${formatCompactNumber(commits)} commits • ${formatCompactNumber(priv)} privados • ${formatCompactNumber(issues)} issues`;

  const heatmapSvg = charts.renderGithubHeatmap({
    weeks,
    compact: false,
  });

  const delta = thisWeekTotal - lastWeekTotal;
  let deltaBadge;
  if (delta > 0) {
    deltaBadge = `<span class="badge badge-ok num">+${delta} vs sem. passada</span>`;
  } else if (delta < 0) {
    deltaBadge = `<span class="badge badge-warn num">${delta} vs sem. passada</span>`;
  } else {
    deltaBadge = '<span class="badge num">igual</span>';
  }

  const compareSvg = charts.renderCompareBars({
    labels: WEEKDAY_LABELS_PT,
    current: thisWeekDays,
    previous: lastWeekDays,
    colorCurrent: 'var(--green)',
    colorPrevious: '#2b363e',
    height: 82,
  });

  const bestDayText = bestDayCount > 0
    ? (bestDayFormatted ? `${bestDayCount} em ${bestDayFormatted}` : `${bestDayCount}`)
    : '—';

  const maxRepoCommits = Math.max(1, ...topRepos.map((r) => Math.max(0, Number(r?.commits || 0))));
  const reposHtml = topRepos.length
    ? topRepos.map((r) => {
        const name = String(r?.name || 'repositório');
        const count = Math.max(0, Number(r?.commits || 0));
        const pct = count > 0 ? Math.max(4, (count / maxRepoCommits) * 100) : 0;
        return `
          <div class="gh-repo-item">
            <div class="gh-repo-head">
              <span class="gh-repo-name" title="${escapeHtml(name)}">${escapeHtml(name)}</span>
              <span class="gh-repo-count num">${escapeHtml(formatCompactNumber(count))}</span>
            </div>
            <div class="gh-repo-bar" aria-hidden="true">
              <div class="gh-repo-bar-fill" style="width:${pct.toFixed(1)}%"></div>
            </div>
          </div>`;
      }).join('')
    : '<div class="empty-state" style="padding:6px"><div class="empty-state-sub">Sem commits em repositórios</div></div>';

  return `
    <div class="gh-layout">
      ${buildErrorBanner(errorMsg, escapeHtml)}
      <div class="card gh-heatmap-card">
        <div class="card-head">
          <span class="card-label" style="color:var(--green)">
            ${icons.icon('github', 13)}
            <span>ÚLTIMO ANO${username ? ` · @${escapeHtml(username)}` : ''}</span>
          </span>
          <span class="gh-year-summary num" title="${escapeHtml(summaryText)}">${escapeHtml(summaryText)}</span>
        </div>
        <div class="gh-heatmap-grid">${heatmapSvg}</div>
      </div>
      <div class="gh-bottom-3">
        <div class="card gh-streak-card">
          <div class="card-head">
            <span class="card-label" style="color:var(--gh-4)">
              ${icons.icon('flame', 13)}
              <span>STREAK</span>
            </span>
          </div>
          <div class="gh-streak-hero">
            <span class="gh-streak-num num">${currentStreak}</span>
            <span class="gh-streak-unit">${currentStreak === 1 ? 'dia' : 'dias'}</span>
          </div>
          <div class="gh-streak-meta">
            <div class="gh-streak-row">
              <span style="color:var(--muted)">Recorde</span>
              <span class="num" style="color:var(--text);font-weight:600">${longestStreak} ${longestStreak === 1 ? 'dia' : 'dias'}</span>
            </div>
            <div class="gh-streak-row">
              <span style="color:var(--muted)">Melhor dia</span>
              <span class="num" style="color:var(--text);font-weight:600;text-align:right">${escapeHtml(bestDayText)}</span>
            </div>
          </div>
        </div>
        <div class="card gh-week-card">
          <div class="card-head">
            <span class="card-label">ESTA SEMANA</span>
            ${deltaBadge}
          </div>
          <div class="gh-week-top">
            <span class="gh-week-num num">${thisWeekTotal}</span>
            <div class="chart-legend" style="gap:8px;font-size:10px">
              <span class="legend-item"><span class="legend-dot" style="background:var(--green)"></span>Atual</span>
              <span class="legend-item"><span class="legend-dot" style="background:#2b363e"></span>Ant.</span>
            </div>
          </div>
          <div class="chart-wrap">${compareSvg}</div>
        </div>
        <div class="card" style="min-height:0">
          <div class="card-head">
            <span class="card-label">TOP REPOS</span>
            <span class="ai-sub-muted">commits</span>
          </div>
          <div class="gh-repos-list">${reposHtml}</div>
        </div>
      </div>
    </div>`;
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;

  if (mountedEl !== mount) {
    mountedEl = mount;
    lastSig = '';
  }

  const gh = ctx.state?.github || null;
  if (!gh || !gh.configured) {
    const sig = 'gh:unconfigured';
    if (sig !== lastSig) {
      lastSig = sig;
      mount.innerHTML = buildUnconfiguredHtml(ctx);
    }
    return;
  }

  const hasData = (Array.isArray(gh.weeks) && gh.weeks.length > 0) || Number(gh.total_contributions || 0) > 0;
  const err = String(gh.error || '').trim();

  if (!hasData && !err && !gh.updated_at_ms) {
    const sig = `gh:loading|${gh.username || ''}`;
    if (sig !== lastSig) {
      lastSig = sig;
      mount.innerHTML = buildLoadingHtml(ctx, gh.username || '');
    }
    return;
  }

  const sig = JSON.stringify([
    'gh:ready',
    gh.username || '',
    gh.total_contributions || 0,
    gh.commits || 0,
    gh.private_contributions || 0,
    gh.issues || 0,
    gh.current_streak || 0,
    gh.longest_streak || 0,
    gh.best_day_count || 0,
    gh.best_day_label || '',
    gh.this_week_total || 0,
    gh.last_week_total || 0,
    gh.this_week_days || [],
    gh.last_week_days || [],
    gh.top_repos || [],
    gh.weeks?.length || 0,
    gh.updated_at_ms || 0,
    err,
  ]);

  if (sig === lastSig) return;
  lastSig = sig;
  mount.innerHTML = buildConfiguredHtml(ctx, gh);
}
