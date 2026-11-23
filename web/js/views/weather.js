let mountedEl = null;
let lastSig = '';

function isFahrenheit(unit) {
  const u = String(unit || 'c').trim().toLowerCase();
  return u === 'f' || u.startsWith('fahrenheit');
}

function roundTemp(value) {
  const n = Number(value);
  return value !== null && value !== undefined && Number.isFinite(n)
    ? `${Math.round(n)}°`
    : '—';
}

function buildUnconfiguredHtml(ctx) {
  const { icons } = ctx;
  return `
    <div class="weather-layout">
      <div class="card empty-state" style="flex:1">
        ${icons.weatherIcon(1, 56)}
        <div class="empty-state-title">Clima não configurado</div>
        <div class="empty-state-sub">Configure <code>weather.city</code> em <code>config.json</code> no PC para ativar a previsão via Open-Meteo.</div>
      </div>
    </div>`;
}

function buildPendingHtml(ctx, weather) {
  const { icons, escapeHtml } = ctx;
  const city = String(weather.city || '').trim();
  const err = String(weather.error || '').trim();

  if (err) {
    return `
      <div class="weather-layout">
        <div class="card empty-state" style="flex:1">
          ${icons.weatherIcon(3, 52)}
          <div class="empty-state-title">${escapeHtml(city || 'Previsão indisponível')}</div>
          <span class="badge badge-err" title="${escapeHtml(err)}">${escapeHtml(err)}</span>
          <div class="empty-state-sub">Verifique <code>weather.city</code> em <code>config.json</code> ou a conexão de internet no PC.</div>
        </div>
      </div>`;
  }

  return `
    <div class="weather-layout">
      <div class="card empty-state" style="flex:1">
        ${icons.weatherIcon(1, 52)}
        <div class="empty-state-title">Carregando previsão${city ? ` · ${escapeHtml(city)}` : ''}…</div>
        <div class="empty-state-sub">Consultando Open-Meteo no PC. Os dados aparecerão automaticamente.</div>
      </div>
    </div>`;
}

function buildConfiguredHtml(ctx, weather) {
  const { icons, escapeHtml, formatClock } = ctx;

  const fahrenheit = isFahrenheit(weather.unit);
  const windUnit = fahrenheit ? 'mph' : 'km/h';
  const tempText = roundTemp(weather.temp);
  const feelsText = roundTemp(weather.feels_like);
  const humidityText = weather.humidity_pct !== null && weather.humidity_pct !== undefined && Number.isFinite(Number(weather.humidity_pct))
    ? `${Math.round(Number(weather.humidity_pct))}%`
    : '—';
  const windText = weather.wind_speed !== null && weather.wind_speed !== undefined && Number.isFinite(Number(weather.wind_speed))
    ? `${Math.round(Number(weather.wind_speed))} ${windUnit}`
    : '—';
  const updatedText = Number(weather.updated_at_ms || 0) > 0
    ? formatClock(new Date(Number(weather.updated_at_ms)))
    : '—';

  const code = weather.weather_code ?? 0;
  const condition = String(weather.condition || '').trim() || '—';
  const city = String(weather.city || '').trim() || 'Local configurado';
  const err = String(weather.error || '').trim();

  const hourly = Array.isArray(weather.hourly) ? weather.hourly.slice(0, 24) : [];
  const daily = Array.isArray(weather.daily) ? weather.daily.slice(0, 6) : [];

  const hourlyHtml = hourly.length
    ? hourly
        .map((h, idx) => {
          const isNow = idx === 0;
          const label = isNow ? 'Agora' : String(h?.time_label || '');
          const hCode = h?.weather_code ?? code;
          const hTemp = roundTemp(h?.temp);
          return `
            <div class="hourly-slot${isNow ? ' is-now' : ''}">
              <span class="hourly-time num">${escapeHtml(label)}</span>
              ${icons.weatherIcon(hCode, 24)}
              <span class="hourly-temp num">${escapeHtml(hTemp)}</span>
            </div>`;
        })
        .join('')
    : '<div class="empty-state-sub" style="padding:8px 2px">Sem previsão horária disponível.</div>';

  const dailyHtml = daily.length
    ? daily
        .map((d, idx) => {
          const isToday = idx === 0;
          const dayLabel = String(d?.day_label || '');
          const dCode = d?.weather_code ?? 0;
          const hiText = roundTemp(d?.high);
          const loText = roundTemp(d?.low);
          const precip = Math.max(0, Math.round(Number(d?.precip_pct || 0)));
          return `
            <div class="daily-card${isToday ? ' is-today' : ''}">
              <span class="daily-day">${escapeHtml(dayLabel)}</span>
              ${icons.weatherIcon(dCode, 26)}
              <div class="daily-temps num">
                <span class="daily-hi">${escapeHtml(hiText)}</span>
                <span class="daily-lo">/ ${escapeHtml(loText)}</span>
              </div>
              <span class="daily-precip num">${precip > 0 ? `${precip}%` : ''}</span>
            </div>`;
        })
        .join('')
    : '<div class="empty-state" style="grid-column:1/-1;padding:6px"><div class="empty-state-sub">Sem previsão diária disponível.</div></div>';

  return `
    <div class="weather-layout">
      <section class="weather-hero" aria-label="Clima atual">
        <div class="weather-hero-left">
          <div class="weather-hero-icon">${icons.weatherIcon(code, 64)}</div>
          <div class="weather-temp-lg num">${escapeHtml(tempText)}</div>
          <div class="weather-hero-titles">
            <div class="weather-hero-cond" title="${escapeHtml(condition)}">${escapeHtml(condition)}</div>
            <div class="weather-hero-city" title="${escapeHtml(city)}">${escapeHtml(city)}</div>
            ${err ? `<span class="badge badge-err" style="align-self:flex-start;margin-top:2px" title="${escapeHtml(err)}">${escapeHtml(err)}</span>` : ''}
          </div>
        </div>
        <div class="weather-hero-meta">
          <div class="weather-meta-item">
            <span class="weather-meta-label">Sensação</span>
            <span class="weather-meta-val num">${escapeHtml(feelsText)}</span>
          </div>
          <div class="weather-meta-item">
            <span class="weather-meta-label">Umidade</span>
            <span class="weather-meta-val num">${escapeHtml(humidityText)}</span>
          </div>
          <div class="weather-meta-item">
            <span class="weather-meta-label">Vento</span>
            <span class="weather-meta-val num">${escapeHtml(windText)}</span>
          </div>
          <div class="weather-meta-item">
            <span class="weather-meta-label">Atualizado</span>
            <span class="weather-meta-val num">${escapeHtml(updatedText)}</span>
          </div>
        </div>
      </section>

      <section class="card weather-hourly-card" aria-label="Previsão das próximas 24 horas">
        <div class="card-head">
          <span class="card-label" style="color:var(--cpu)">
            ${icons.icon('clock', 13)}
            <span>PRÓXIMAS 24 HORAS</span>
          </span>
        </div>
        <div class="weather-hourly-strip" data-weather-hourly>${hourlyHtml}</div>
      </section>

      <section class="card weather-daily-card" aria-label="Previsão dos próximos 6 dias">
        <div class="card-head">
          <span class="card-label">
            ${icons.icon('weather', 13)}
            <span>PRÓXIMOS 6 DIAS</span>
          </span>
        </div>
        <div class="weather-daily-grid">${dailyHtml}</div>
      </section>
    </div>`;
}

export function render(ctx) {
  const mount = ctx.mount || ctx.el;
  if (!mount) return;

  if (mountedEl !== mount) {
    mountedEl = mount;
    lastSig = '';
  }

  const weather = ctx.state?.weather || null;
  if (!weather || !weather.configured) {
    const sig = 'weather:unconfigured';
    if (sig !== lastSig) {
      lastSig = sig;
      mount.innerHTML = buildUnconfiguredHtml(ctx);
    }
    return;
  }

  const hasTemp = weather.temp !== null && weather.temp !== undefined && Number.isFinite(Number(weather.temp));
  if (!hasTemp) {
    const sig = JSON.stringify(['weather:pending', weather.city || '', weather.error || '']);
    if (sig !== lastSig) {
      lastSig = sig;
      mount.innerHTML = buildPendingHtml(ctx, weather);
    }
    return;
  }

  const sig = JSON.stringify([
    'weather:ready',
    weather.city || '',
    weather.unit || 'c',
    weather.temp,
    weather.feels_like,
    weather.humidity_pct,
    weather.wind_speed,
    weather.weather_code,
    weather.condition || '',
    weather.updated_at_ms || 0,
    weather.error || '',
    weather.hourly || [],
    weather.daily || [],
  ]);

  if (sig === lastSig) return;
  lastSig = sig;

  const prevHourlyEl = mount.querySelector('[data-weather-hourly]');
  const prevScrollLeft = prevHourlyEl ? prevHourlyEl.scrollLeft : 0;

  mount.innerHTML = buildConfiguredHtml(ctx, weather);

  if (prevScrollLeft > 0) {
    const nextHourlyEl = mount.querySelector('[data-weather-hourly]');
    if (nextHourlyEl) {
      nextHourlyEl.scrollLeft = prevScrollLeft;
    }
  }
}
