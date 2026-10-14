// SSPUR Pulse: the weather where the visitor is. Loaded only on the overview and weather pages.
// Location: a precise place the visitor chose (localStorage), else an approximate one from the IP
// through BigDataCloud's client-side API, else the board city nearest to the browser's time zone.
// Weather: the SSPUR-computed board for board cities, else Open-Meteo, cached for 15 minutes.
(() => {
  'use strict';
  const D = document, W = window, $ = (s, e = D) => e.querySelector(s);
  const base = (D.currentScript && D.currentScript.src || '').replace(/assets\/here\.js.*$/, '');
  const BDC = 'https://api.bigdatacloud.net/data/reverse-geocode-client';
  const get = (st, k) => { try { return JSON.parse(st.getItem(k) || 'null'); } catch (e) { return null; } };
  const put = (st, k, v) => { try { if (v == null) st.removeItem(k); else st.setItem(k, JSON.stringify(v)); } catch (e) { /* private mode */ } };
  const ls = W.localStorage, ss = W.sessionStorage;
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => '&#' + c.charCodeAt(0) + ';');
  const timed = (url, ms) => { const c = new AbortController(), t = setTimeout(() => c.abort(), ms); return fetch(url, { signal: c.signal, credentials: 'omit', referrerPolicy: 'no-referrer' }).then((r) => { clearTimeout(t); if (!r.ok) throw new Error(r.status); return r.json(); }); };
  const km = (a, b, c, d) => { const r = Math.PI / 180, h = Math.sin((c - a) * r / 2) ** 2 + Math.cos(a * r) * Math.cos(c * r) * Math.sin((d - b) * r / 2) ** 2; return 12742 * Math.asin(Math.sqrt(h)); };
  const P = { sun: 'M12 3v2M12 19v2M5.6 5.6 7 7M17 17l1.4 1.4M3 12h2M19 12h2M5.6 18.4 7 17M17 7l1.4-1.4M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z', moon: 'M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z', cloud: 'M7 18h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 9.5 4.3 4.3 0 0 0 7 18z', rain: 'M7 15h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 6.5 4.3 4.3 0 0 0 7 15zM8 18l-1 3M12 18l-1 3M16 18l-1 3', snow: 'M7 14h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 5.5 4.3 4.3 0 0 0 7 14zM8 18h.01M12 20h.01M16 18h.01M10 22h.01M14 22h.01', storm: 'M7 14h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 5.5 4.3 4.3 0 0 0 7 14zM12 15l-2 4h4l-2 4', fog: 'M4 9h16M4 13h16M6 17h12' };
  const kind = (c, day) => c === 0 || c === 1 ? (day ? 'sun' : 'moon') : c <= 3 ? 'cloud' : c <= 48 ? 'fog' : c <= 67 || (c >= 80 && c <= 82) ? 'rain' : c <= 77 || c === 85 || c === 86 ? 'snow' : c >= 95 ? 'storm' : 'cloud';
  const words = { sun: 'Clear', moon: 'Clear', cloud: 'Cloudy', fog: 'Fog', rain: 'Rain', snow: 'Snow', storm: 'Thunderstorm' };
  const icon = (k) => `<svg class="ic" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="${P[k]}"/></svg>`;
  const deg = (t) => (t < 0 ? '−' : '') + Math.abs(t).toFixed(0) + '°C';

  let boardP = null;
  const board = () => boardP || (boardP = timed(base + 'api/here.json', 6000).catch(() => null));
  const nearestCity = (b, la, lo) => b && b.cities.reduce((best, c) => { const d = km(la, lo, c.la, c.lo); return !best || d < best.d ? { c, d } : best; }, null);
  const tzPlace = (b) => {
    let tz = ''; try { tz = Intl.DateTimeFormat().resolvedOptions().timeZone || ''; } catch (e) { /* none */ }
    const s = b && b.zones[tz], c = s && b.cities.find((x) => x.s === s);
    return c ? { name: c.n, lat: c.la, lon: c.lo, kind: 'zone' } : null;
  };
  const named = (j) => j && (j.city || j.locality || j.principalSubdivision || j.countryName);
  const ipPlace = async () => {
    const hit = get(ss, 'pulse-ip'); if (hit) return hit;
    const j = await timed(BDC + '?localityLanguage=en', 4000);
    if (!(isFinite(j.latitude) && isFinite(j.longitude) && named(j))) throw new Error('no place');
    const p = { name: named(j), lat: j.latitude, lon: j.longitude, kind: 'approx' };
    put(ss, 'pulse-ip', p); return p;
  };
  const precisePlace = () => new Promise((ok, no) => {
    if (!navigator.geolocation) { no(new Error('no geolocation')); return; }
    navigator.geolocation.getCurrentPosition(async (pos) => {
      const la = pos.coords.latitude, lo = pos.coords.longitude;
      let name = '';
      try { name = named(await timed(`${BDC}?latitude=${la}&longitude=${lo}&localityLanguage=en`, 5000)) || ''; } catch (e) { /* keep coordinates */ }
      ok({ name: name || 'Your location', lat: +la.toFixed(3), lon: +lo.toFixed(3), kind: 'precise' });
    }, no, { enableHighAccuracy: true, timeout: 10000, maximumAge: 600000 });
  });
  const place = async (b) => {
    const chosen = get(ls, 'pulse-place'); if (chosen) return chosen;
    try { return await ipPlace(); } catch (e) { return tzPlace(b); }
  };
  const forecast = async (p) => {
    const k = `pulse-wx:${p.lat.toFixed(2)},${p.lon.toFixed(2)}`, hit = get(ss, k);
    if (hit && Date.now() - hit.at < 15 * 60000) return hit.data;
    const data = await timed(`https://api.open-meteo.com/v1/forecast?latitude=${p.lat.toFixed(3)}&longitude=${p.lon.toFixed(3)}&current=temperature_2m,weather_code,is_day&hourly=temperature_2m,precipitation,wind_speed_10m&daily=temperature_2m_max,temperature_2m_min,precipitation_sum,wind_speed_10m_max&timezone=auto&past_days=30&forecast_days=1&past_hours=24&forecast_hours=24`, 6000);
    if (!data.current || !isFinite(data.current.temperature_2m)) throw new Error('no weather');
    put(ss, k, { at: Date.now(), data }); return data;
  };
  // Where we are and what it is like there. Board cities within 30 km use the board.
  const resolve = async () => {
    const b = await board(), p = await place(b);
    if (!p) return null;
    const n = nearestCity(b, p.lat, p.lon);
    if (n && n.d <= 30) return { p, city: n.c, t: n.c.t, k: kind(n.c.c, n.c.d), hi: n.c.hi, lo: n.c.lw };
    try { const f = await forecast(p); return { p, f, t: f.current.temperature_2m, k: kind(f.current.weather_code, f.current.is_day), hi: f.daily.temperature_2m_max.slice(-1)[0], lo: f.daily.temperature_2m_min.slice(-1)[0] }; } catch (e) { return n ? { p: { ...p, name: n.c.n, kind: 'zone' }, city: n.c, t: n.c.t, k: kind(n.c.c, n.c.d), hi: n.c.hi, lo: n.c.lw } : null; }
  };
  const note = (p) => p.kind === 'precise' ? 'precise' : p.kind === 'approx' ? 'approximate' : 'nearest board city';
  const link = (r) => r.city ? `weather/#city-v=${encodeURIComponent(r.city.s)}~temp~48h&go=cityfig` : 'weather/#here=1&go=here';

  // The overview card: one link, plus a pin button for the location settings
  const card = $('[data-lwx]');
  const short = (t) => (t < 0 ? '−' : '') + Math.abs(Math.round(t)) + '°';
  const paint = async () => {
    if (!card) return;
    const r = await resolve().catch(() => null);
    if (!r) {
      $('.lwx-name', card).textContent = 'World weather'; $('.lwx-n', card).textContent = 'A hundred cities, right now'; $('.lwx-v', card).textContent = '';
      D.documentElement.classList.add('lwx-ok'); return;
    }
    card.href = link(r);
    $('.lwx-ic', card).innerHTML = icon(r.k);
    $('.lwx-v', card).textContent = short(r.t);
    $('.lwx-name', card).textContent = r.p.name;
    $('.lwx-n', card).textContent = words[r.k];
    const pin = $('.lwx-pinx', card), how = r.p.kind === 'precise' ? 'Precise, from your device' : r.p.kind === 'approx' ? 'Approximate, from your network' : 'The board city nearest your time zone';
    pin.classList.toggle('precise', r.p.kind === 'precise'); pin.setAttribute('aria-hidden', 'true'); pin.parentElement.title = how;
    card.setAttribute('aria-label', `${r.p.name}, ${deg(r.t)}, ${words[r.k]}. ${how}. Open the weather board`);
    D.documentElement.classList.add('lwx-ok');
  };
  const menu = () => {
    const btn = $('[data-lwx-menu]'); if (!btn) return;
    btn.addEventListener('click', () => {
      const old = $('.psheet'); if (old) { old.remove(); return; }
      const chosen = get(ls, 'pulse-place'), sh = D.createElement('div');
      sh.className = 'psheet'; sh.setAttribute('role', 'dialog'); sh.setAttribute('aria-label', 'Location');
      sh.innerHTML = `<strong>Your place</strong><p>Approximate place from your IP (BigDataCloud). Precise place only if you allow it, used just to fetch the weather (Open-Meteo).</p><div class="acts">${chosen && chosen.kind === 'precise' ? '' : '<button type="button" class="primary" data-act="precise">Use precise location</button>'}${chosen ? '<button type="button" data-act="forget">Forget my place</button>' : ''}<button type="button" data-act="close">Close</button></div>`;
      btn.parentElement.parentElement.appendChild(sh);
      const close = () => { sh.remove(); D.removeEventListener('keydown', onKey); btn.focus(); };
      const onKey = (e) => { if (e.key === 'Escape') close(); };
      D.addEventListener('keydown', onKey);
      sh.addEventListener('click', async (e) => {
        const a = e.target.closest('[data-act]'); if (!a) return;
        if (a.dataset.act === 'close') { close(); return; }
        if (a.dataset.act === 'precise') { a.disabled = true; a.textContent = 'Finding you…'; try { put(ls, 'pulse-place', await precisePlace()); } catch (er) { a.textContent = 'Location not available'; return; } }
        if (a.dataset.act === 'forget') { put(ls, 'pulse-place', null); put(ss, 'pulse-ip', null); }
        close(); await paint(); if (D.documentElement.classList.contains('here')) await here();
      });
      const first = sh.querySelector('button'); if (first) first.focus();
      if (!matchMedia('(prefers-reduced-motion: reduce)').matches) sh.animate([{ opacity: 0, transform: 'translateY(10px)' }, { opacity: 1, transform: 'none' }], { duration: 220, easing: 'cubic-bezier(.2,.8,.2,1)' });
    });
  };

  // The weather page card: the same chart and toggles as "One city, up close"
  const hourLabel = (t) => ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][new Date(t + ':00Z').getUTCDay()] + ' ' + t.slice(11, 16);
  const dayLabel = (t) => ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'][+t.slice(5, 7) - 1] + ' ' + +t.slice(8, 10);
  const here = async () => {
    const box = $('#here'); if (!box) return;
    const r = await resolve().catch(() => null);
    if (!r) { box.innerHTML = '<div class="card"><p class="muted">Your area is not available right now, so here is the whole board.</p></div>'; return; }
    if (!r.f) { box.innerHTML = `<div class="card"><h2 class="ct">Your area: ${esc(r.p.name)}</h2><p class="muted">${esc(r.city.n)} is on the board. <a href="./?up=1#city-v=${encodeURIComponent(r.city.s)}~temp~48h&go=cityfig">See it up close</a>.</p></div>`; return; }
    const f = r.f, hl = f.hourly.time.map(hourLabel), dl = f.daily.time.map(dayLabel);
    const s = (key, name, color, values) => ({ key, name, color, values: values.map((v) => v == null ? null : v) });
    const views = [
      { key: 'temp~48h', label: 'Temperature', chart: { kind: 'line', labels: hl, series: [s('t', 'Temperature', 2, f.hourly.temperature_2m)], unit: '°C', digits: 1 } },
      { key: 'rain~48h', label: 'Rain', chart: { kind: 'bar', labels: hl, series: [s('r', 'Rain', 1, f.hourly.precipitation)], unit: 'mm', digits: 1 } },
      { key: 'wind~48h', label: 'Wind', chart: { kind: 'line', labels: hl, series: [s('w', 'Wind', 6, f.hourly.wind_speed_10m)], unit: 'km/h', digits: 0 } },
      { key: 'temp~30d', label: 'Temperature', chart: { kind: 'line', labels: dl, series: [s('hi', 'High', 2, f.daily.temperature_2m_max), s('lo', 'Low', 1, f.daily.temperature_2m_min)], unit: '°C', digits: 1 } },
      { key: 'rain~30d', label: 'Rain', chart: { kind: 'bar', labels: dl, series: [s('r', 'Rain', 1, f.daily.precipitation_sum)], unit: 'mm', digits: 1 } },
      { key: 'wind~30d', label: 'Wind', chart: { kind: 'line', labels: dl, series: [s('w', 'Strongest wind', 6, f.daily.wind_speed_10m_max)], unit: 'km/h', digits: 0 } },
    ];
    const seg = (part, opts) => `<div class="seg ctl" role="radiogroup" aria-label="${part ? 'Span' : 'Metric'}" data-vpart="${part}" data-for="here">${opts.map(([v, l], i) => `<button type="button" role="radio" aria-checked="${i === 0}" data-v="${v}">${l}</button>`).join('')}</div>`;
    box.innerHTML = `<figure class="card cv-fig" data-group="here"><figcaption class="cv-cap"><h2 class="ct">Your area: ${esc(r.p.name)} <span class="pill acc">${note(r.p)}</span></h2><p><span class="lwx-big">${icon(r.k)}<b>${deg(r.t)}</b></span> ${words[r.k]}. Today ${deg(r.lo)} to ${deg(r.hi)}. Live from Open-Meteo, in local time.</p></figcaption><div class="cv-ctl">${seg(0, [['temp', 'Temperature'], ['rain', 'Rain'], ['wind', 'Wind']])}${seg(1, [['48h', '48 hours'], ['30d', '30 days']])}</div><div class="cv" tabindex="0" role="group" aria-label="Your area, interactive chart. Use arrow keys to read values."></div><table class="sr-only"></table><p class="muted small">Approximate place from your IP (BigDataCloud). Precise place only if you allow it, used just to fetch the weather (Open-Meteo).</p></figure>`;
    W.PulseCharts.add({ id: 'here', views, ranges: [], view: 'temp~48h', range: '' }, $('figure', box));
  };
  menu();
  if (card) paint();
  if (D.documentElement.classList.contains('here')) here();
})();
