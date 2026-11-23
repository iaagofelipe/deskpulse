let mountedEl = null;
let latestCtx = null;
let lastArtKey = null;
let isDraggingSeek = false;
let currentMedia = null;
let refs = null;

function formatMmSs(secs) {
  if (secs === null || secs === undefined || !Number.isFinite(Number(secs))) {
    return '--:--';
  }
  const total = Math.max(0, Math.floor(Number(secs)));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) {
    return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  }
  return `${m}:${String(s).padStart(2, '0')}`;
}

function updateSliderFill(slider, val, max) {
  const safeMax = Number(max) > 0 ? Number(max) : 1;
  const pct = Math.max(0, Math.min(100, (Number(val) / safeMax) * 100));
  slider.style.background = `linear-gradient(to right, var(--gpu) 0%, var(--gpu) ${pct.toFixed(1)}%, var(--surface-2) ${pct.toFixed(1)}%, var(--surface-2) 100%)`;
}

async function sendMediaCommand(action, body) {
  if (!latestCtx) return;
  try {
    const updated = await latestCtx.apiRequest('POST', `/api/media/${encodeURIComponent(action)}`, body);
    if (updated && typeof updated === 'object') {
      if (latestCtx.state) {
        latestCtx.state.media = updated;
      }
      applyMediaState(updated, latestCtx);
    }
  } catch {
    // Next SSE tick reconciles real state
  }
}

function ensureShell(mount, ctx) {
  if (mountedEl === mount && refs && mount.querySelector('.media-hero')) {
    return;
  }
  mountedEl = mount;
  lastArtKey = null;
  isDraggingSeek = false;

  const { icons } = ctx;

  mount.innerHTML = `
    <div class="media-hero">
      <div class="media-art-col">
        <div class="media-art-box" data-media-art-box>
          <span data-media-art-fallback>${icons.icon('music', 64)}</span>
          <img alt="Capa do álbum" decoding="async" class="hidden" data-media-art-img>
        </div>
      </div>

      <div class="media-info-col">
        <div class="media-app-tag" data-media-app>
          ${icons.icon('music', 12)}
          <span data-media-app-text>Mídia</span>
        </div>

        <h2 class="media-title-lg" data-media-title>Nenhuma mídia tocando</h2>
        <div class="media-artist-lg" data-media-artist>Aguardando reprodução no PC</div>
        <div class="media-album hidden" data-media-album></div>

        <div class="seek-wrap">
          <input
            type="range"
            class="seek-slider"
            min="0"
            max="100"
            step="1"
            value="0"
            disabled
            aria-label="Posição da faixa"
            data-media-seek
          >
          <div class="seek-times num">
            <span data-media-elapsed>--:--</span>
            <span data-media-duration>--:--</span>
          </div>
        </div>

        <div class="media-controls">
          <button type="button" class="ctrl-btn" data-media-action="prev" aria-label="Faixa anterior" title="Anterior">
            ${icons.icon('prev', 22)}
          </button>
          <button type="button" class="ctrl-btn ctrl-play" data-media-action="play-pause" aria-label="Reproduzir ou pausar" title="Play / Pause" data-media-play-btn>
            ${icons.icon('play', 26)}
          </button>
          <button type="button" class="ctrl-btn" data-media-action="next" aria-label="Próxima faixa" title="Próxima">
            ${icons.icon('next', 22)}
          </button>
        </div>

        <div class="media-vol-bar">
          <div class="vol-group">
            <button type="button" class="pill-btn" data-media-action="vol-down" aria-label="Diminuir volume">
              ${icons.icon('volume_down', 15)}
              <span>−</span>
            </button>
            <button type="button" class="pill-btn num" data-media-action="toggle-mute" aria-label="Silenciar som" data-media-mute-btn>
              <span data-media-mute-icon style="display:inline-flex">${icons.icon('volume_up', 15)}</span>
              <span data-media-vol-text>--%</span>
            </button>
            <button type="button" class="pill-btn" data-media-action="vol-up" aria-label="Aumentar volume">
              ${icons.icon('volume_up', 15)}
              <span>+</span>
            </button>
          </div>

          <button type="button" class="pill-btn mic-btn" data-media-action="toggle-mic" aria-label="Alternar microfone" data-media-mic-btn>
            <span data-media-mic-icon style="display:inline-flex">${icons.icon('mic', 15)}</span>
            <span data-media-mic-text>Mic</span>
          </button>
        </div>
      </div>
    </div>`;

  refs = {
    artBox: mount.querySelector('[data-media-art-box]'),
    artFallback: mount.querySelector('[data-media-art-fallback]'),
    artImg: mount.querySelector('[data-media-art-img]'),
    appText: mount.querySelector('[data-media-app-text]'),
    title: mount.querySelector('[data-media-title]'),
    artist: mount.querySelector('[data-media-artist]'),
    album: mount.querySelector('[data-media-album]'),
    seek: mount.querySelector('[data-media-seek]'),
    elapsed: mount.querySelector('[data-media-elapsed]'),
    duration: mount.querySelector('[data-media-duration]'),
    playBtn: mount.querySelector('[data-media-play-btn]'),
    muteBtn: mount.querySelector('[data-media-mute-btn]'),
    muteIcon: mount.querySelector('[data-media-mute-icon]'),
    volText: mount.querySelector('[data-media-vol-text]'),
    micBtn: mount.querySelector('[data-media-mic-btn]'),
    micIcon: mount.querySelector('[data-media-mic-icon]'),
    micText: mount.querySelector('[data-media-mic-text]'),
  };

  const seek = refs.seek;
  const startDrag = () => {
    isDraggingSeek = true;
  };
  const endDrag = () => {
    isDraggingSeek = false;
  };

  seek.addEventListener('pointerdown', startDrag);
  seek.addEventListener('mousedown', startDrag);
  seek.addEventListener('touchstart', startDrag, { passive: true });
  seek.addEventListener('pointerup', endDrag);
  seek.addEventListener('pointercancel', endDrag);
  seek.addEventListener('mouseup', endDrag);
  seek.addEventListener('touchend', endDrag);

  seek.addEventListener('input', () => {
    isDraggingSeek = true;
    const val = Number(seek.value);
    refs.elapsed.textContent = formatMmSs(val);
    updateSliderFill(seek, val, Number(seek.max));
  });

  seek.addEventListener('change', () => {
    const val = Number(seek.value);
    isDraggingSeek = false;
    refs.elapsed.textContent = formatMmSs(val);
    updateSliderFill(seek, val, Number(seek.max));
    sendMediaCommand('seek', { value: val });
  });

  mount.addEventListener('click', (ev) => {
    const btn = ev.target.closest('button[data-media-action]');
    if (!btn) return;
    const action = btn.dataset.mediaAction;
    const vol = Number(currentMedia?.volume_pct ?? 50);

    if (action === 'play-pause' || action === 'prev' || action === 'next' || action === 'toggle-mute' || action === 'toggle-mic') {
      sendMediaCommand(action);
    } else if (action === 'vol-down') {
      const nextVol = Math.max(0, vol - 5);
      sendMediaCommand('volume', { level: nextVol });
    } else if (action === 'vol-up') {
      const nextVol = Math.min(100, vol + 5);
      sendMediaCommand('volume', { level: nextVol });
    }
  });
}

function applyMediaState(media, ctx) {
  if (!refs) return;
  const { icons } = ctx;
  currentMedia = media || null;

  const playing = Boolean(media?.playing);
  const title = media?.title || 'Nenhuma mídia tocando';
  const artist = media?.artist || (media?.app ? media.app : 'Inicie Spotify, YouTube ou player no PC');
  const album = media?.album || '';
  const app = media?.app || (playing ? 'Reproduzindo' : 'Sem mídia ativa');
  const rawArt = media?.artwork_data_url;
  const art = typeof rawArt === 'string' && rawArt.startsWith('data:image/') ? rawArt : '';
  const artKey = art ? `${art.length}:${art.slice(-24)}` : '';

  refs.artBox.classList.toggle('is-playing', playing);
  if (artKey !== lastArtKey) {
    lastArtKey = artKey;
    if (art) {
      refs.artImg.src = art;
      refs.artImg.classList.remove('hidden');
      refs.artFallback.classList.add('hidden');
    } else {
      refs.artImg.removeAttribute('src');
      refs.artImg.classList.add('hidden');
      refs.artFallback.classList.remove('hidden');
    }
  }

  if (refs.appText.textContent !== app) refs.appText.textContent = app;
  if (refs.title.textContent !== title) {
    refs.title.textContent = title;
    refs.title.title = title;
  }
  if (refs.artist.textContent !== artist) {
    refs.artist.textContent = artist;
    refs.artist.title = artist;
  }
  if (album) {
    if (refs.album.textContent !== album) {
      refs.album.textContent = album;
      refs.album.title = album;
    }
    refs.album.classList.remove('hidden');
  } else {
    refs.album.textContent = '';
    refs.album.classList.add('hidden');
  }

  const playStateKey = playing ? 'pause' : 'play';
  if (refs.playBtn.dataset.iconState !== playStateKey) {
    refs.playBtn.dataset.iconState = playStateKey;
    refs.playBtn.innerHTML = icons.icon(playStateKey, 26);
    refs.playBtn.setAttribute('aria-label', playing ? 'Pausar' : 'Reproduzir');
  }

  const dur = media?.duration_secs !== null && media?.duration_secs !== undefined && Number(media.duration_secs) > 0
    ? Math.floor(Number(media.duration_secs))
    : null;
  const pos = media?.position_secs !== null && media?.position_secs !== undefined
    ? Math.max(0, Math.floor(Number(media.position_secs)))
    : null;
  const canSeek = Boolean(media?.can_seek) && dur !== null && dur > 0;

  refs.seek.disabled = !canSeek;
  if (!isDraggingSeek) {
    if (dur !== null && dur > 0) {
      const clampedPos = pos !== null ? Math.min(dur, pos) : 0;
      refs.seek.max = String(dur);
      refs.seek.value = String(clampedPos);
      refs.elapsed.textContent = formatMmSs(clampedPos);
      refs.duration.textContent = formatMmSs(dur);
      updateSliderFill(refs.seek, clampedPos, dur);
    } else {
      refs.seek.max = '100';
      refs.seek.value = '0';
      refs.elapsed.textContent = pos !== null ? formatMmSs(pos) : '--:--';
      refs.duration.textContent = '--:--';
      updateSliderFill(refs.seek, 0, 100);
    }
  }

  const volPct = media && Number.isFinite(Number(media.volume_pct))
    ? Math.max(0, Math.min(100, Math.round(Number(media.volume_pct))))
    : null;
  const muted = Boolean(media?.muted);
  const muteIconName = muted || volPct === 0 ? 'volume_off' : (volPct !== null && volPct < 50 ? 'volume_down' : 'volume_up');
  if (refs.muteIcon.dataset.iconState !== muteIconName) {
    refs.muteIcon.dataset.iconState = muteIconName;
    refs.muteIcon.innerHTML = icons.icon(muteIconName, 15);
  }
  const volLabel = muted ? 'Mudo' : (volPct !== null ? `${volPct}%` : '--%');
  if (refs.volText.textContent !== volLabel) {
    refs.volText.textContent = volLabel;
  }
  refs.muteBtn.classList.toggle('pill-danger', muted);

  const micMuted = Boolean(media?.mic_muted);
  const micIconName = micMuted ? 'mic_off' : 'mic';
  if (refs.micIcon.dataset.iconState !== micIconName) {
    refs.micIcon.dataset.iconState = micIconName;
    refs.micIcon.innerHTML = icons.icon(micIconName, 15);
  }
  const micLabel = micMuted ? 'Mic mudo' : 'Mic';
  if (refs.micText.textContent !== micLabel) {
    refs.micText.textContent = micLabel;
  }
  refs.micBtn.classList.toggle('is-muted', micMuted);
  refs.micBtn.setAttribute('aria-pressed', micMuted ? 'true' : 'false');
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;
  latestCtx = ctx;

  ensureShell(mount, ctx);
  applyMediaState(ctx.state?.media || null, ctx);
}
