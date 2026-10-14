// SSPUR Pulse: satellites in orbit on a globe. Loaded only when the globe scrolls into view.
// The camera is fixed in space (ECI): the Earth turns at its true rate (GMST), so geostationary
// satellites hang over the same country while low orbits sweep past at their real speed.
(() => {
  'use strict';
  const D = document, W = window, $ = (s, e = D) => e.querySelector(s), $$ = (s, e = D) => [...e.querySelectorAll(s)];
  const box = $('#orbit'); if (!box) return;
  const base = (D.currentScript && D.currentScript.src || '').replace(/assets\/orbit\.js.*$/, '');
  const stage = $('.orb-stage', box), tipEl = $('.orb-tip', box), live = $('.orb-live', box);
  const calm = W.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const RE = 6378.137, rad = Math.PI / 180;

  // State
  let idx = null, sats = [], frames = [], pending = false, speed = calm ? 0 : 1, sim = Date.now(), last = performance.now();
  let yaw = 0, pitch = 20 * rad, zoom = 1, drag = null, pinch = null, visible = false, dirty = true, sel = -1, track = null, info = null;
  const shown = {}, ST = W.PulseState || { get: () => null, set: () => {} };
  let band = -1, allSl = false;
  let countries = null, land = [];

  // Canvases: the globe below, points above
  const globe = D.createElement('canvas'), dots = D.createElement('canvas');
  globe.className = 'orb-globe'; dots.className = 'orb-dots';
  dots.tabIndex = 0; dots.setAttribute('role', 'img'); dots.setAttribute('aria-label', 'Globe of satellites in orbit. Drag or use the arrow keys to turn it, plus and minus to zoom, 0 to reset.');
  stage.querySelector('.orb-wait') && stage.querySelector('.orb-wait').remove();
  stage.append(globe, dots);
  const g2 = globe.getContext('2d'), d2 = dots.getContext('2d');
  let Wpx = 0, Hpx = 0, dpr = 1, img = null, buf = null;
  const resize = () => {
    const r = stage.getBoundingClientRect(); dpr = Math.min(2, W.devicePixelRatio || 1);
    Wpx = Math.max(1, Math.round(r.width * dpr)); Hpx = Math.max(1, Math.round(r.height * dpr));
    for (const c of [globe, dots]) { c.width = Wpx; c.height = Hpx; }
    img = d2.createImageData(Wpx, Hpx); buf = new Uint32Array(img.data.buffer); dirty = true;
  };
  new ResizeObserver(resize).observe(stage); resize();

  // Time and the sun
  const gmst = (t) => { const jd = t / 86400000 + 2440587.5, T = (jd - 2451545) / 36525; let g = 280.46061837 + 360.98564736629 * (jd - 2451545) + 0.000387933 * T * T; return ((g % 360) + 360) % 360 * rad; };
  const sun = (t) => { const n = t / 86400000 + 2440587.5 - 2451545, L = (280.46 + 0.9856474 * n) * rad, g = (357.528 + 0.9856003 * n) * rad, l = L + (1.915 * Math.sin(g) + 0.02 * Math.sin(2 * g)) * rad, e = (23.439 - 4e-7 * n) * rad; return [Math.cos(l), Math.cos(e) * Math.sin(l), Math.sin(e) * Math.sin(l)]; };

  // View: yaw about the pole, then pitch; orthographic, depth toward the viewer
  const view = (x, y, z) => { const c = Math.cos(yaw), s = Math.sin(yaw), x1 = x * c - y * s, y1 = x * s + y * c, cp = Math.cos(pitch), sp = Math.sin(pitch); return [x1, y1 * cp - z * sp, y1 * sp + z * cp]; };
  const scale = () => Math.min(Wpx, Hpx) * 0.36 * zoom;
  const ecf2eci = (lat, lon, t) => { const th = gmst(t) + lon * rad, cl = Math.cos(lat * rad); return [cl * Math.cos(th), cl * Math.sin(th), Math.sin(lat * rad)]; };
  const faceLon = (lon) => { yaw = -Math.PI / 2 - (gmst(sim) + lon * rad); };

  // The globe layer: ocean, graticule, coastlines, night side, atmosphere
  const drawGlobe = () => {
    const S = scale(), cx = Wpx / 2, cy = Hpx / 2, dark = getComputedStyle(D.documentElement).colorScheme === 'dark';
    g2.clearRect(0, 0, Wpx, Hpx);
    const atm = g2.createRadialGradient(cx, cy, S * 0.98, cx, cy, S * 1.12);
    atm.addColorStop(0, 'rgba(110,170,255,0.45)'); atm.addColorStop(1, 'rgba(110,170,255,0)');
    g2.fillStyle = atm; g2.beginPath(); g2.arc(cx, cy, S * 1.12, 0, 7); g2.fill();
    const sea = g2.createRadialGradient(cx - S * 0.3, cy - S * 0.35, S * 0.1, cx, cy, S);
    sea.addColorStop(0, dark ? '#1d3a66' : '#2a5591'); sea.addColorStop(1, dark ? '#0b1730' : '#13284d');
    g2.fillStyle = sea; g2.beginPath(); g2.arc(cx, cy, S, 0, 7); g2.fill();
    const proj = (lat, lon) => { const [x, y, z] = view(...ecf2eci(lat, lon, sim)); return [cx + S * x, cy - S * z, -y]; };
    g2.lineWidth = 1 * dpr; g2.strokeStyle = 'rgba(255,255,255,0.07)';
    for (let lat = -60; lat <= 60; lat += 30) { g2.beginPath(); let pen = false; for (let lon = -180; lon <= 180; lon += 4) { const [x, y, d] = proj(lat, lon); if (d > 0) { if (pen) g2.lineTo(x, y); else g2.moveTo(x, y); pen = true; } else pen = false; } g2.stroke(); }
    for (let lon = -180; lon < 180; lon += 30) { g2.beginPath(); let pen = false; for (let lat = -88; lat <= 88; lat += 4) { const [x, y, d] = proj(lat, lon); if (d > 0) { if (pen) g2.lineTo(x, y); else g2.moveTo(x, y); pen = true; } else pen = false; } g2.stroke(); }
    g2.lineWidth = 1.3 * dpr; g2.strokeStyle = 'rgba(190,225,255,0.75)'; g2.fillStyle = 'rgba(120,170,230,0.16)';
    for (const ring of land) {
      const pts = ring.map(([la, lo]) => proj(la, lo)), all = pts.every((p) => p[2] > 0);
      g2.beginPath(); let pen = false;
      for (const [x, y, d] of pts) { if (d > 0) { if (pen) g2.lineTo(x, y); else g2.moveTo(x, y); pen = true; } else pen = false; }
      if (all) { g2.closePath(); g2.fill(); }
      g2.stroke();
    }
    // Night: the part of the visible disk facing away from the sun (visible means y < 0)
    const sv = view(...sun(sim)), TAU = Math.PI * 2;
    const u = norm(cross(sv, Math.abs(sv[2]) < 0.9 ? [0, 0, 1] : [1, 0, 0])), v = cross(sv, u), ring = [];
    for (let k = 0; k < 360; k += 2) { const a = k * rad; ring.push([u[0] * Math.cos(a) + v[0] * Math.sin(a), u[1] * Math.cos(a) + v[1] * Math.sin(a), u[2] * Math.cos(a) + v[2] * Math.sin(a)]); }
    g2.fillStyle = 'rgba(2,6,20,0.55)'; g2.beginPath();
    const start = ring.findIndex((p, k) => p[1] < 0 && ring[(k + ring.length - 1) % ring.length][1] >= 0);
    if (start < 0) { if (sv[1] > 0) g2.arc(cx, cy, S, 0, TAU); }
    else {
      const vis = []; for (let k = 0; k < ring.length; k++) { const p = ring[(start + k) % ring.length]; if (p[1] < 0) vis.push(p); else break; }
      vis.forEach((p, k) => { const x = cx + S * p[0], y = cy - S * p[2]; if (k) g2.lineTo(x, y); else g2.moveTo(x, y); });
      const e = vis[vis.length - 1], b = vis[0], t0 = Math.atan2(-e[2], e[0]), t1 = Math.atan2(-b[2], b[0]);
      const night = (t) => Math.cos(t) * sv[0] - Math.sin(t) * sv[2] < 0, mid = t0 + ((t1 - t0 + TAU) % TAU) / 2;
      g2.arc(cx, cy, S, t0, t1, !night(mid));
      g2.closePath();
    }
    g2.fill();
    if (track) {
      g2.strokeStyle = 'rgba(255,255,255,0.85)'; g2.lineWidth = 2 * dpr; g2.setLineDash([4 * dpr, 4 * dpr]); g2.beginPath(); let pen = false;
      for (const [la, lo] of track) { const [x, y, d] = proj(la, lo); if (d > 0) { if (pen) g2.lineTo(x, y); else g2.moveTo(x, y); pen = true; } else pen = false; }
      g2.stroke(); g2.setLineDash([]);
    }
  };
  const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const norm = (a) => { const l = Math.hypot(...a); return a.map((x) => x / l); };

  // The points layer, written straight into pixels
  const colors = [];
  const rgba = (hex, a) => { const n = parseInt(hex.slice(1), 16); return (a << 24) | ((n & 255) << 16) | (((n >> 8) & 255) << 8) | (n >> 16); };
  const posAt = () => {
    if (!frames.length) return null;
    if (frames.length === 1 || sim <= frames[0].t) return { a: frames[0].pos, b: frames[0].pos, f: 0 };
    for (let k = 0; k < frames.length - 1; k++) { const A = frames[k], B = frames[k + 1]; if (sim <= B.t) return { a: A.pos, b: B.pos, f: (sim - A.t) / (B.t - A.t || 1) }; }
    const L = frames[frames.length - 1]; return { a: L.pos, b: L.pos, f: 0 };
  };
  const projected = new Float32Array(0);
  let proj2 = projected;
  const drawDots = () => {
    const P = posAt(); buf.fill(0);
    if (!P) { d2.putImageData(img, 0, 0); return; }
    const S = scale() / RE, cx = Wpx / 2, cy = Hpx / 2, R2 = (scale()) ** 2, size = Math.max(2, Math.round(1.5 * dpr));
    if (proj2.length !== sats.length * 3) proj2 = new Float32Array(sats.length * 3);
    const c = Math.cos(yaw), s = Math.sin(yaw), cp = Math.cos(pitch), sp = Math.sin(pitch), f = P.f;
    for (let i = 0; i < sats.length; i++) {
      const o = sats[i]; proj2[3 * i] = NaN;
      const ax = P.a[3 * i]; if (!(ax === ax) || !shown[o.g] || (band >= 0 && o.band !== band) || (o.x && !allSl)) continue;
      const bx = P.b[3 * i]; const x = ax + (bx === bx ? (bx - ax) * f : 0), y = P.a[3 * i + 1] + ((P.b[3 * i + 1] - P.a[3 * i + 1]) || 0) * f, z = P.a[3 * i + 2] + ((P.b[3 * i + 2] - P.a[3 * i + 2]) || 0) * f;
      const x1 = x * c - y * s, y1 = x * s + y * c, y2 = y1 * cp - z * sp, z2 = y1 * sp + z * cp;
      const sx = cx + S * x1, sy = cy - S * z2;
      if (y2 > 0 && (S * x1) ** 2 + (S * z2) ** 2 < R2) continue;
      const px = sx | 0, py = sy | 0;
      if (px < 0 || py < 0 || px >= Wpx - size || py >= Hpx - size) continue;
      const col = i === sel ? 0xffffffff : colors[o.g], sz = o.big ? size + 2 : size;
      if (px >= Wpx - sz || py >= Hpx - sz) continue;
      for (let yy = 0; yy < sz; yy++) for (let xx = 0; xx < sz; xx++) buf[(py + yy) * Wpx + px + xx] = col;
      proj2[3 * i] = sx / dpr; proj2[3 * i + 1] = sy / dpr;
    }
    d2.putImageData(img, 0, 0);
    if (sel >= 0 && proj2[3 * sel] === proj2[3 * sel]) { d2.strokeStyle = '#fff'; d2.lineWidth = 2 * dpr; d2.beginPath(); d2.arc(proj2[3 * sel] * dpr, proj2[3 * sel + 1] * dpr, 7 * dpr, 0, 7); d2.stroke(); }
  };

  // The worker
  const worker = new Worker(base + 'assets/orbit-worker.js');
  worker.onmessage = (e) => {
    const m = e.data;
    if (m.op === 'at') { pending = false; frames.push({ t: m.t, pos: m.pos }); frames.sort((a, b) => a.t - b.t); while (frames.length > 3) frames.shift(); }
    if (m.op === 'info' && m.ok && m.i === sel) { track = m.track; info = m; dirty = true; showInfo(); }
  };
  const cadence = () => speed >= 60 ? 250 : 1000;
  const request = () => {
    if (pending || !sats.length) return;
    const lastT = frames.length ? frames[frames.length - 1].t : sim - 1;
    if (speed === 0) { if (!frames.length || Math.abs(lastT - sim) > 1) { pending = true; worker.postMessage({ op: 'at', t: sim }); } return; }
    if (lastT - sim > cadence() * speed) return;
    pending = true; worker.postMessage({ op: 'at', t: Math.max(sim, lastT) + cadence() * speed });
  };

  // Loading
  const fetchJson = (u) => fetch(u, { credentials: 'omit' }).then((r) => { if (!r.ok) throw new Error(r.status); return r.json(); });
  const loadPart = async (g, file, k) => {
    const j = await fetchJson(base + 'sats/' + file + '.json');
    const gi = idx.groups.findIndex((x) => x.key === j.g);
    const add = j.s.map((s) => { const mm = s[10], a = Math.cbrt(398600.4418 / (mm * 2 * Math.PI / 86400) ** 2) - RE; return { name: s[1], g: gi, mm, alt: a, big: j.g === 'crewed', x: j.g === 'starlink' && k > 0, band: a < 2000 ? 0 : a > 35000 && a < 36600 ? 2 : 1 }; });
    sats.push(...add); worker.postMessage({ op: 'add', sats: j.s }); frames = []; pending = false;
  };
  const loadedParts = new Set();
  const load = async (all) => {
    for (const g of idx.groups) for (const [k, p] of g.parts.entries()) {
      if (loadedParts.has(p.file)) continue;
      if (g.key === 'starlink' && k > 0 && !all) continue;
      loadedParts.add(p.file); await loadPart(g, p.file, k);
    }
  };
  // Without "Draw all", Starlink is a quarter sample: say how many are drawn
  const slNote = () => {
    const sl = idx.groups.find((g) => g.key === 'starlink'), el = $('.orb-sl', box); if (!sl || !el) return;
    const n = sats.filter((s) => idx.groups[s.g].key === 'starlink' && (allSl || !s.x)).length;
    el.textContent = n < sl.count ? `(${n.toLocaleString('en-US')} drawn now)` : '';
  };
  const loadLand = async () => {
    const t = await fetch(base + 'assets/land.path').then((r) => r.text());
    land = t.split('Z').map((p) => p.replace('M', '').trim().split(/\s+/).map(Number)).filter((a) => a.length > 4).map((a) => { const r = []; for (let i = 0; i + 1 < a.length; i += 2) r.push([90 - a[i + 1] / 2, a[i] / 2 - 180]); return r; });
    dirty = true;
  };

  // Tooltip, picking, countries
  const country = (lat, lon) => {
    if (!countries) return '';
    const r = Math.min(179, Math.max(0, Math.floor(90 - lat))), c = Math.min(359, Math.max(0, Math.floor(lon + 180))), want = r * 360 + c;
    let at = 0; for (const run of countries.runs) { if (want < at + run[1]) return run[0] ? countries.names[run[0] - 1] : ''; at += run[1]; }
    return '';
  };
  const showInfo = () => {
    if (sel < 0 || !info) { tipEl.hidden = true; return; }
    const o = sats[sel], g = idx.groups[o.g], over = country(info.lat, info.lon);
    tipEl.innerHTML = `<b>${esc(isIss(sel) ? 'International Space Station' : o.name)}</b><br>${esc(g.name)}<br>${Math.round(info.alt).toLocaleString('en-US')} km up, ${info.speed.toFixed(2)} km/s<br>Over ${esc(over || 'the ocean')}${isIss(sel) ? '<br>Crewed without a break since November 2000, once around the Earth every ' + Math.round(info.period) + ' minutes' : ''}`;
    tipEl.hidden = false;
    live.textContent = `${o.name}, ${g.name}, ${Math.round(info.alt)} kilometres up, over ${over || 'the ocean'}`;
  };
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => '&#' + c.charCodeAt(0) + ';');
  const pick = (cx, cy) => {
    let best = -1, bd = (matchMedia('(pointer: coarse)').matches ? 22 : 12) ** 2;
    const r2 = bd; let big = -1, bigd = r2;
    for (let i = 0; i < sats.length; i++) { const x = proj2[3 * i]; if (!(x === x)) continue; const d = (x - cx) ** 2 + (proj2[3 * i + 1] - cy) ** 2; if (d < bd) { bd = d; best = i; } if (sats[i].big && d < bigd) { bigd = d; big = i; } }
    return big >= 0 ? big : best;
  };
  const isIss = (i) => i >= 0 && /^ISS \(/.test(sats[i].name);
  const flyTo = (i) => {
    const P = posAt(); if (!P) return;
    const x = P.a[3 * i], y = P.a[3 * i + 1], z = P.a[3 * i + 2], r = Math.hypot(x, y, z);
    const y0 = yaw, p0 = pitch, z0 = zoom, y1 = -Math.PI / 2 - Math.atan2(y, x), p1 = Math.asin(z / r), z1 = Math.max(zoom, 1.6);
    const dy = ((y1 - y0 + Math.PI * 3) % (Math.PI * 2)) - Math.PI;
    if (calm) { yaw = y0 + dy; pitch = p1; zoom = z1; dirty = true; return; }
    const t0 = performance.now(), step = (now) => { const k = Math.min(1, (now - t0) / 900), e = 1 - Math.pow(1 - k, 3); yaw = y0 + dy * e; pitch = p0 + (p1 - p0) * e; zoom = z0 + (z1 - z0) * e; dirty = true; if (k < 1) requestAnimationFrame(step); };
    requestAnimationFrame(step);
  };
  const choose = (i) => {
    sel = i; track = null; info = null; dirty = true;
    if (i >= 0) worker.postMessage({ op: 'info', i, t: sim }); else showInfo();
    if (isIss(i)) { flyTo(i); if (W.PulseEgg) W.PulseEgg('iss'); }
  };

  // Input: drag to turn, wheel and pinch to zoom, double tap to reset, keys
  const pts = new Map();
  dots.addEventListener('pointerdown', (e) => { dots.setPointerCapture(e.pointerId); pts.set(e.pointerId, [e.clientX, e.clientY]); drag = { x: e.clientX, y: e.clientY, yaw, pitch, moved: false }; if (pts.size === 2) { const [a, b] = [...pts.values()]; pinch = { d: Math.hypot(a[0] - b[0], a[1] - b[1]), zoom }; } });
  dots.addEventListener('pointermove', (e) => {
    if (!pts.has(e.pointerId)) return; pts.set(e.pointerId, [e.clientX, e.clientY]);
    if (pinch && pts.size === 2) { const [a, b] = [...pts.values()]; zoom = Math.min(8, Math.max(0.15, pinch.zoom * Math.hypot(a[0] - b[0], a[1] - b[1]) / pinch.d)); dirty = true; return; }
    if (!drag) return; const k = 1.6 / (Math.min(Wpx, Hpx) / dpr * zoom);
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y; if (Math.abs(dx) + Math.abs(dy) > 4) drag.moved = true;
    yaw = drag.yaw + dx * k * 2; pitch = Math.max(-1.4, Math.min(1.4, drag.pitch + dy * k * 2)); dirty = true;
  });
  const up = (e) => { pts.delete(e.pointerId); if (pts.size < 2) pinch = null; if (drag && !drag.moved && e.type === 'pointerup') { const r = dots.getBoundingClientRect(); choose(pick(e.clientX - r.left, e.clientY - r.top)); } drag = null; };
  dots.addEventListener('pointerup', up); dots.addEventListener('pointercancel', up);
  dots.addEventListener('wheel', (e) => { e.preventDefault(); zoom = Math.min(8, Math.max(0.15, zoom * Math.exp(-e.deltaY / 500))); dirty = true; }, { passive: false });
  const reset = () => { zoom = 1; pitch = 20 * rad; faceLon(home); dirty = true; };
  dots.addEventListener('dblclick', reset);
  dots.addEventListener('keydown', (e) => {
    const k = e.key, step = 8 * rad;
    if (k === 'ArrowLeft') yaw -= step; else if (k === 'ArrowRight') yaw += step; else if (k === 'ArrowUp') pitch = Math.min(1.4, pitch + step); else if (k === 'ArrowDown') pitch = Math.max(-1.4, pitch - step);
    else if (k === '+' || k === '=') zoom = Math.min(8, zoom * 1.25); else if (k === '-') zoom = Math.max(0.15, zoom / 1.25); else if (k === '0') reset(); else if (k === 'Escape') choose(-1); else return;
    e.preventDefault(); dirty = true;
  });

  // Controls: speed and orbit band on the globe, operators in the legend (a sheet on phones)
  const legend = $('.orb-legend', box), lbtn = $('[data-orb-layers]', box), scrim = $('.orb-scrim', box);
  const gi = (key) => idx.groups.findIndex((x) => x.key === key);
  const persist = () => {
    const off = idx.groups.filter((g, i) => !shown[i]).map((g) => g.key);
    ST.set('o-off', off.length ? off.join(',') : null); ST.set('o-band', band >= 0 ? String(band) : null);
    ST.set('o-speed', speed === (calm ? 0 : 1) ? null : String(speed)); ST.set('o-all', allSl ? '1' : null);
  };
  const sync = () => {
    $$('[data-og]', box).forEach((b) => b.setAttribute('aria-checked', String(!!shown[gi(b.dataset.og)])));
    $$('[data-ofam]', box).forEach((f) => { const on = $$('[data-og]', f.parentElement).map((b) => b.getAttribute('aria-checked') === 'true'); f.setAttribute('aria-checked', on.every(Boolean) ? 'true' : on.some(Boolean) ? 'mixed' : 'false'); });
    $$('[data-band]', box).forEach((b) => b.setAttribute('aria-checked', String(+b.dataset.band === band)));
    $$('.orb-legend .n', box).forEach((n) => { const v = +(band < 0 ? n.dataset.n : n.dataset['n' + band]); n.textContent = v.toLocaleString('en-US'); n.parentElement.classList.toggle('none', !v); });
    const sw = $('[data-orb-all]', box); if (sw) sw.setAttribute('aria-checked', String(allSl));
    if (sel >= 0 && (!shown[sats[sel].g] || (band >= 0 && sats[sel].band !== band) || (sats[sel].x && !allSl))) choose(-1);
    if (lbtn) lbtn.classList.toggle('off', band >= 0 || idx.groups.some((g, i) => !shown[i]));
    slNote(); dirty = true;
  };
  const setSpeed = (v) => { speed = v; frames = frames.filter((f) => f.t <= sim + 1); $$('[data-speed]', box).forEach((b) => b.setAttribute('aria-checked', String(+b.dataset.speed === v))); };
  const setAll = async (on) => {
    const sw = $('[data-orb-all]', box); allSl = on; sync();
    if (on && sw) { sw.setAttribute('aria-busy', 'true'); await load(true); sw.removeAttribute('aria-busy'); sync(); }
  };
  box.addEventListener('click', (e) => {
    const t = e.target.closest('button,[data-orb-close]'); if (!t || !idx) return;
    if (t.dataset.speed != null) { const v = +t.dataset.speed; if (v === 1 && speed !== 1) sim = Date.now(); setSpeed(v); persist(); return; }
    if (t.dataset.band != null) { band = +t.dataset.band; sync(); persist(); return; }
    if (t.dataset.og) { const i = gi(t.dataset.og); shown[i] = !shown[i]; sync(); persist(); return; }
    if (t.dataset.ofam) { const on = t.getAttribute('aria-checked') !== 'true'; $$('[data-og]', t.parentElement).forEach((b) => { shown[gi(b.dataset.og)] = on; }); sync(); persist(); return; }
    if (t.hasAttribute('data-orb-all')) { if (!t.hasAttribute('aria-busy')) setAll(!allSl).then(persist); return; }
    if (t.hasAttribute('data-orb-layers')) { if (legend.classList.contains('open')) closeSheet(); else openSheet(); return; }
    if (t.hasAttribute('data-orb-close')) closeSheet();
  });
  // The Layers sheet on phones: a modal bottom sheet with the same legend
  const onSheetKey = (e) => {
    if (e.key === 'Escape') { e.preventDefault(); closeSheet(); return; }
    if (e.key !== 'Tab') return;
    const bs = $$('button', legend).filter((b) => b.offsetParent), i = bs.indexOf(D.activeElement);
    if (e.shiftKey && i <= 0) { e.preventDefault(); bs[bs.length - 1].focus(); } else if (!e.shiftKey && i === bs.length - 1) { e.preventDefault(); bs[0].focus(); }
  };
  const openSheet = () => {
    legend.classList.add('open'); legend.setAttribute('role', 'dialog'); legend.setAttribute('aria-modal', 'true'); legend.setAttribute('aria-label', 'Layers');
    scrim.hidden = false; lbtn.setAttribute('aria-expanded', 'true'); D.addEventListener('keydown', onSheetKey);
    requestAnimationFrame(() => { const f = $('.orb-fh', legend); if (f) f.focus({ preventScroll: true }); });
  };
  const closeSheet = () => {
    if (!legend.classList.contains('open')) return;
    legend.classList.remove('open'); legend.setAttribute('role', 'group'); legend.removeAttribute('aria-modal'); legend.setAttribute('aria-label', 'Operators');
    scrim.hidden = true; lbtn.setAttribute('aria-expanded', 'false'); D.removeEventListener('keydown', onSheetKey);
    if (lbtn.offsetParent) lbtn.focus({ preventScroll: true });
  };
  W.matchMedia('(min-width: 700px)').addEventListener('change', (m) => { if (m.matches) closeSheet(); });

  // The loop: pause when hidden or off-screen
  let home = 78, fpsN = 0, fpsT = performance.now(), fps = 0;
  try { const tz = Intl.DateTimeFormat().resolvedOptions().timeZone || ''; home = /^America/.test(tz) ? -90 : /^Europe|^Africa/.test(tz) ? 15 : /^Australia|^Pacific/.test(tz) ? 140 : /^Asia\/(Tokyo|Seoul|Shanghai|Hong|Taipei|Manila)/.test(tz) ? 125 : 78; } catch (e) { /* India first */ }
  let lastGlobe = 0;
  const frame = (now) => {
    const dt = now - last; last = now;
    if (visible && !D.hidden) {
      sim += dt * speed;
      request();
      const rot = speed * dt / 240000; // degrees of Earth rotation this frame
      if (dirty || now - lastGlobe > (speed > 1 ? 0 : 1000) || rot > 0.02) { drawGlobe(); lastGlobe = now; dirty = false; }
      drawDots();
      if (sel >= 0 && info && speed) { if ((fpsN & 15) === 0) worker.postMessage({ op: 'info', i: sel, t: sim }); }
      fpsN++; if (now - fpsT > 1000) { fps = fpsN * 1000 / (now - fpsT); fpsN = 0; fpsT = now; }
    }
    requestAnimationFrame(frame);
  };
  new IntersectionObserver((es) => { visible = es[0].isIntersecting; last = performance.now(); }, { rootMargin: '100px' }).observe(stage);

  // Test hooks for the Playwright checks
  W.PulseOrbit = {
    get fps() { return fps; }, get sim() { return sim; }, get count() { return sats.length; }, setSpeed,
    one: (i, t) => new Promise((ok) => { const h = (e) => { if (e.data.op === 'one' && e.data.i === i && e.data.t === t) { worker.removeEventListener('message', h); ok(e.data); } }; worker.addEventListener('message', h); worker.postMessage({ op: 'one', i, t }); }),
    groupOf: (i) => idx.groups[sats[i].g].key,
    find: (name) => sats.findIndex((s) => s.name === name),
    where: (i) => new Promise((ok) => { const h = (e) => { if (e.data.op === 'info' && e.data.i === i) { worker.removeEventListener('message', h); ok(e.data); } }; worker.addEventListener('message', h); worker.postMessage({ op: 'info', i, t: sim }); }),
    screen: (i) => (proj2[3 * i] === proj2[3 * i] ? [proj2[3 * i], proj2[3 * i + 1]] : null),
    face: (i) => { const P = posAt(); if (!P) return; const x = P.a[3 * i], y = P.a[3 * i + 1], z = P.a[3 * i + 2]; yaw = -Math.PI / 2 - Math.atan2(y, x); pitch = Math.asin(z / Math.hypot(x, y, z)); dirty = true; },
    view: () => ({ yaw, pitch, zoom }),
    visibleCount: () => { let n = 0; for (let i = 0; i < sats.length; i++) if (proj2[3 * i] === proj2[3 * i]) n++; return n; },
  };

  (async () => {
    try {
      idx = await fetchJson(base + 'api/orbit.json');
      idx.groups.forEach((g, i) => { shown[i] = true; colors[i] = rgba(g.color, 255); });
      faceLon(home);
      // Views from the URL: o-off=starlink,geo&o-band=0&o-speed=600&o-all=1
      const off = (ST.get('o-off') || '').split(','), b0 = +ST.get('o-band'), s0 = ST.get('o-speed');
      idx.groups.forEach((g, i) => { if (off.includes(g.key)) shown[i] = false; });
      if ([0, 1, 2].includes(b0) && ST.get('o-band') !== null) band = b0;
      if (['0', '1', '60', '600'].includes(s0)) speed = +s0;
      allSl = ST.get('o-all') === '1';
      await Promise.all([loadLand(), load(allSl)]);
      sync();
      fetchJson(base + 'assets/countries.json').then((c) => { const runs = c.grid.split(',').map((x) => { const [v, n] = x.split('x'); return [+v, n ? +n : 1]; }); countries = { names: c.names, runs }; }).catch(() => {});
      setSpeed(speed);
      box.classList.add('orb-on');
      requestAnimationFrame(frame);
    } catch (e) {
      stage.insertAdjacentHTML('beforeend', '<p class="orb-fail muted">The globe could not load its data right now.</p>');
    }
  })();
})();
