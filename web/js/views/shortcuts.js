const PAGE_SIZE = 8;

let mountedEl = null;
let latestCtx = null;
let currentPage = 0;
let lastStructureSig = '';
let lastPagerSig = '';
const firingTimers = new Map();

function safeTileColor(raw) {
  const s = String(raw || '').trim();
  if (/^(#[0-9a-fA-F]{3,8}|var\(--[a-zA-Z0-9_-]+\)|rgba?\([0-9.,\s%]+\)|hsla?\([0-9.,\s%deg]+\))$/.test(s)) {
    return s;
  }
  return 'var(--accent)';
}

function flashTileFeedback(btn, tileId) {
  if (!btn) return;
  const prev = firingTimers.get(tileId);
  if (prev) clearTimeout(prev);
  btn.classList.add('is-firing');
  const timer = setTimeout(() => {
    btn.classList.remove('is-firing');
    firingTimers.delete(tileId);
  }, 220);
  firingTimers.set(tileId, timer);
}

async function triggerShortcut(tileId, btn) {
  if (!latestCtx || !tileId) return;
  flashTileFeedback(btn, tileId);
  try {
    const resp = await latestCtx.apiRequest(
      'POST',
      `/api/shortcuts/${encodeURIComponent(tileId)}/trigger`
    );
    if (resp && resp.media && latestCtx.state) {
      latestCtx.state.media = resp.media;
    }
  } catch (err) {
    const notice = mountedEl?.querySelector('[data-shortcuts-error]');
    if (notice) {
      notice.textContent = (err && err.message) || 'Falha ao executar atalho';
      notice.classList.remove('hidden');
      setTimeout(() => {
        if (notice) {
          notice.textContent = '';
          notice.classList.add('hidden');
        }
      }, 3500);
    }
  }
}

function ensureShell(mount) {
  if (mountedEl === mount && mount.querySelector('.shortcuts-layout')) {
    return;
  }
  mountedEl = mount;
  lastStructureSig = '';
  lastPagerSig = '';

  mount.innerHTML = `
    <div class="shortcuts-layout">
      <div class="badge badge-err hidden" style="align-self:center;padding:5px 12px" role="alert" data-shortcuts-error></div>
      <div class="shortcuts-grid" data-shortcuts-grid></div>
      <div class="shortcuts-pager hidden" data-shortcuts-pager></div>
    </div>`;

  mount.addEventListener('click', (ev) => {
    const tileBtn = ev.target.closest('.shortcut-tile[data-tile-id]');
    if (tileBtn) {
      triggerShortcut(tileBtn.dataset.tileId, tileBtn);
      return;
    }

    const pagerBtn = ev.target.closest('[data-page-action]');
    if (pagerBtn) {
      const action = pagerBtn.dataset.pageAction;
      if (action === 'prev') {
        currentPage = Math.max(0, currentPage - 1);
      } else if (action === 'next') {
        currentPage += 1;
      } else if (action === 'goto') {
        const idx = Number(pagerBtn.dataset.pageIndex);
        if (Number.isFinite(idx) && idx >= 0) {
          currentPage = idx;
        }
      }
      if (latestCtx) render(latestCtx);
    }
  });
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;
  latestCtx = ctx;

  ensureShell(mount);

  const grid = mount.querySelector('[data-shortcuts-grid]');
  const pager = mount.querySelector('[data-shortcuts-pager]');
  if (!grid || !pager) return;

  const tiles = Array.isArray(ctx.state?.shortcuts) ? ctx.state.shortcuts : [];
  if (!tiles.length) {
    if (lastStructureSig !== 'empty') {
      lastStructureSig = 'empty';
      lastPagerSig = '';
      grid.className = 'card empty-state';
      grid.style.flex = '1';
      grid.innerHTML = `
        ${ctx.icons.icon('shortcuts', 36)}
        <div class="empty-state-title">Nenhum atalho configurado</div>
        <div class="empty-state-sub">Adicione blocos [[shortcuts]] em config.toml para controlar apps, hotkeys e áudio no estilo Stream Deck.</div>`;
      pager.classList.add('hidden');
      pager.innerHTML = '';
    }
    return;
  }

  const totalPages = Math.max(1, Math.ceil(tiles.length / PAGE_SIZE));
  if (currentPage >= totalPages) {
    currentPage = totalPages - 1;
  }

  const startIdx = currentPage * PAGE_SIZE;
  const pageTiles = tiles.slice(startIdx, startIdx + PAGE_SIZE);

  // Separate static structure signature from live active toggle state so
  // SSE updates never wipe tactile press animations on unchanged tiles.
  const structureSig = `${currentPage}|` + pageTiles
    .map((t) => `${t.id}\x1f${t.label}\x1f${t.icon}\x1f${t.color}`)
    .join('\x1e');

  if (structureSig !== lastStructureSig) {
    lastStructureSig = structureSig;
    grid.className = 'shortcuts-grid';
    grid.style.flex = '';
    grid.innerHTML = pageTiles
      .map((tile) => {
        const color = safeTileColor(tile.color);
        const activeClass = tile.active === true ? ' is-active' : '';
        const safeId = ctx.escapeHtml(tile.id);
        const safeLabel = ctx.escapeHtml(tile.label || tile.id);
        return `
          <button
            type="button"
            class="shortcut-tile${activeClass}"
            style="--tile-color:${color}"
            data-tile-id="${safeId}"
            aria-pressed="${tile.active === true ? 'true' : 'false'}"
            title="${safeLabel}"
          >
            <span class="tile-icon" style="width:clamp(42px,8vh,52px);height:clamp(42px,8vh,52px)">
              ${ctx.icons.icon(tile.icon, 36)}
            </span>
            <span class="tile-label">${safeLabel}</span>
          </button>`;
      })
      .join('');
  } else {
    // Surgically sync live illuminated active state without rebuilding DOM
    const buttons = grid.querySelectorAll('.shortcut-tile');
    pageTiles.forEach((tile, idx) => {
      const btn = buttons[idx];
      if (!btn) return;
      const isActive = tile.active === true;
      btn.classList.toggle('is-active', isActive);
      btn.setAttribute('aria-pressed', isActive ? 'true' : 'false');
    });
  }

  const pagerSig = `${currentPage}/${totalPages}`;
  if (totalPages > 1) {
    pager.classList.remove('hidden');
    if (pagerSig !== lastPagerSig) {
      lastPagerSig = pagerSig;
      const dots = Array.from({ length: totalPages }, (_, idx) => {
        const active = idx === currentPage ? ' active' : '';
        return `<button type="button" class="pager-dot${active}" data-page-action="goto" data-page-index="${idx}" aria-label="Página ${idx + 1}" aria-current="${idx === currentPage ? 'page' : 'false'}"></button>`;
      }).join('');

      pager.innerHTML = `
        <button type="button" class="pill-btn" style="padding:4px 10px" data-page-action="prev" aria-label="Página anterior" ${currentPage === 0 ? 'disabled' : ''}>
          ${ctx.icons.icon('chevron_left', 15)}
        </button>
        <div style="display:inline-flex;align-items:center;gap:6px">${dots}</div>
        <button type="button" class="pill-btn" style="padding:4px 10px" data-page-action="next" aria-label="Próxima página" ${currentPage >= totalPages - 1 ? 'disabled' : ''}>
          ${ctx.icons.icon('chevron_right', 15)}
        </button>`;
    }
  } else {
    pager.classList.add('hidden');
    if (lastPagerSig !== '') {
      lastPagerSig = '';
      pager.innerHTML = '';
    }
  }
}
