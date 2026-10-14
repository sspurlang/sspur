// SSPUR Pulse: progressive enhancement. Pages are complete without this file.
(() => {
  'use strict';
  const D = document, R = D.documentElement, W = window;
  const $ = (s, e = D) => e.querySelector(s), $$ = (s, e = D) => [...e.querySelectorAll(s)];
  const calm = () => W.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const base = (D.currentScript && D.currentScript.src || '').replace(/assets\/pulse\.js.*$/, '');
  const tip = $('.tip');
  const store = (k, v) => { try { if (v === undefined) return localStorage.getItem(k); if (v === null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch (e) { return null; } };

  // Keep the current section's chip in view in the scrolling nav
  const row = $('.chips-row'), on = $('.chip.on');
  if (row && on && row.scrollWidth > row.clientWidth) row.scrollLeft = Math.max(0, on.getBoundingClientRect().left - row.getBoundingClientRect().left - (row.clientWidth - on.offsetWidth) / 2);

  // Theme
  const themeBtn = $('.theme');
  if (themeBtn) themeBtn.addEventListener('click', () => {
    const dark = R.dataset.theme ? R.dataset.theme === 'dark' : W.matchMedia('(prefers-color-scheme: dark)').matches;
    R.dataset.theme = dark ? 'light' : 'dark';
    store('pulse-theme', R.dataset.theme);
  });

  // Relative times, countdowns and clocks
  const plural = (n, w) => n + ' ' + w + (n === 1 ? '' : 's');
  const ago = (ms) => { const s = Math.round(ms / 1000); return s < 90 ? 'just now' : s < 5400 ? plural(Math.round(s / 60), 'minute') + ' ago' : s < 129600 ? plural(Math.round(s / 3600), 'hour') + ' ago' : plural(Math.round(s / 86400), 'day') + ' ago'; };
  const pad = (n) => String(n).padStart(2, '0');
  const tick = () => {
    const now = Date.now();
    $$('time[data-rel]').forEach((t) => { t.textContent = ago(now - Date.parse(t.getAttribute('datetime'))); });
    $$('[data-countdown]').forEach((e) => {
      let s = Math.round((Date.parse(e.dataset.countdown) - now) / 1000);
      const neg = s < 0; s = Math.abs(s);
      const d = Math.floor(s / 86400), h = Math.floor(s % 86400 / 3600), m = Math.floor(s % 3600 / 60), x = s % 60;
      e.textContent = (neg ? 'T+' : 'T−') + (d ? d + 'd ' : '') + pad(h) + ':' + pad(m) + ':' + pad(x);
      if (neg && e.dataset.done !== e.dataset.countdown) { e.dataset.done = e.dataset.countdown; if (s < 5) egg('liftoff', e); }
    });
    $$('[data-clock]').forEach((e) => { const t = new Date(now); e.textContent = pad(t.getHours()) + ':' + pad(t.getMinutes()) + ':' + pad(t.getSeconds()); });
    $$('[data-tz]').forEach((e) => { try { e.textContent = Intl.DateTimeFormat().resolvedOptions().timeZone.replace(/_/g, ' '); } catch (er) { /* keep */ } });
  };
  tick();
  setInterval(tick, 1000);

  // View state in the URL hash, remembered per page
  const key = 'pulse:' + location.pathname;
  const state = new URLSearchParams(location.hash.slice(1) || store(key) || '');
  const save = () => { const s = state.toString(); store(key, s || null); history.replaceState(null, '', s ? '#' + s : location.pathname + location.search); };

  // Tooltip
  const showTip = (html, x, y) => {
    tip.innerHTML = html; tip.hidden = false;
    const r = tip.getBoundingClientRect(), vw = R.clientWidth;
    let left = Math.min(Math.max(8, x - r.width / 2), vw - r.width - 8), top = y - r.height - 14;
    if (top < 8) top = y + 18;
    tip.style.transform = `translate(${Math.round(left)}px,${Math.round(top)}px)`;
  };
  const hideTip = () => { tip.hidden = true; };
  D.addEventListener('pointerdown', (e) => { if (!e.target.closest('.cv,.map,[data-tip]')) { hideTip(); $$('.cv.on').forEach(clear); } }, { passive: true });

  // Number formatting, shared with the SSPUR renderer
  const compact = (v) => { const a = Math.abs(v); return a >= 999500 ? (v / 1e6).toFixed(a >= 9950000 ? 0 : 1) + 'M' : a >= 9950 ? (v / 1e3).toFixed(0) + 'K' : a >= 1000 ? (v / 1e3).toFixed(1) + 'K' : Number.isInteger(v) ? String(v) : v.toFixed(1); };
  const tickLabel = (v, d) => Math.abs(v) >= 1000 ? compact(v) : Number.isInteger(+v.toFixed(9)) ? String(Math.round(v)) : v.toFixed(Math.max(1, d));
  const valueLabel = (v, d) => Math.abs(v) >= 10000 ? compact(v) : d === 0 ? Math.round(v).toLocaleString('en-US') : v.toFixed(d);
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => '&#' + c.charCodeAt(0) + ';');

  // Charts
  const groups = {};
  try { JSON.parse(($('#pulse-charts') || {}).textContent || '[]').forEach((g) => { groups[g.id] = g; }); } catch (e) { /* server render stays */ }
  const niceUnit = (raw) => { const m = Math.pow(10, Math.floor(Math.log10(raw))), n = raw / m; return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 2.5 ? 2.5 : n <= 5 ? 5 : 10) * m; };
  const ticks = (lo, hi, count, least = 0) => {
    const span = hi - lo <= 0 ? Math.max(1, Math.abs(hi)) : hi - lo, step = Math.max(least, niceUnit(span / count)), top = hi <= lo ? lo + span : hi;
    const a = Math.floor(lo / step), b = Math.ceil(top / step), out = [];
    for (let i = a; i <= Math.max(b, a + 1); i++) out.push(+(i * step).toFixed(10));
    return out;
  };
  const isRank = (c) => c.kind === 'line' && c.unit === 'rank';
  const ypct = (c, v, lo, hi) => isRank(c) ? 100 - pct(v, lo, hi) : pct(v, lo, hi);
  const domain = (c) => {
    if (isRank(c)) { const vs = c.series.flatMap((s) => s.values.filter((v) => v != null)), t = ticks(0, vs.length ? Math.max(...vs) : 1, 4, c.digits === 0 ? 1 : 0); return [1, Math.max(2, t[t.length - 1])]; }
    const vs = c.kind === 'stack' ? c.labels.map((_, i) => c.series.reduce((s, x) => s + (x.values[i] || 0), 0)) : c.series.flatMap((s) => s.values.filter((v) => v != null));
    const hi = vs.length ? Math.max(...vs) : 1, lo = c.kind === 'line' ? Math.min(0, ...(vs.length ? vs : [0])) : 0;
    const t = ticks(lo, hi, 4, c.digits === 0 ? 1 : 0);
    return [t[0], t[t.length - 1]];
  };
  const pct = (v, lo, hi) => hi === lo ? 0 : (v - lo) / (hi - lo) * 100;
  const xAt = (c, i) => c.kind === 'line' ? (c.labels.length <= 1 ? 50 : i * 100 / (c.labels.length - 1)) : (i + 0.5) * 100 / c.labels.length;
  const xpos = (i, n) => n <= 1 ? 500 : i * 1000 / (n - 1);
  const linePath = (c, vs, lo, hi) => {
    const n = vs.length; let pen = false, d = '';
    vs.forEach((v, i) => { if (v == null) { pen = false; return; } d += (pen ? 'L' : 'M') + xpos(i, n).toFixed(1) + ' ' + (1000 - ypct(c, v, lo, hi) * 10).toFixed(1); pen = true; });
    return d;
  };
  const areaPath = (vs, lo, hi) => {
    const n = vs.length; let pen = false, last = 0, d = '';
    vs.forEach((v, i) => {
      if (v == null) { if (pen) d += 'L' + last.toFixed(1) + ' 1000Z'; pen = false; return; }
      const x = xpos(i, n).toFixed(1), y = (1000 - pct(v, lo, hi) * 10).toFixed(1);
      d += pen ? 'L' + x + ' ' + y : 'M' + x + ' 1000L' + x + ' ' + y; last = xpos(i, n); pen = true;
    });
    return pen ? d + 'L' + last.toFixed(1) + ' 1000Z' : d;
  };
  const xLabels = (c) => {
    const n = c.labels.length, wide = n <= 12 ? 1 : Math.max(1, Math.floor((n + 5) / 6)), narrow = n <= 7 ? 1 : n <= 12 ? 2 : Math.max(1, Math.floor((n + 2) / 3));
    return c.labels.map((l, i) => (n - 1 - i) % wide ? '' : `<span${(n - 1 - i) % narrow ? ' class="w"' : ''} style="left:${xAt(c, i).toFixed(2)}%">${esc(l)}</span>`).join('');
  };
  const hbar = (c) => {
    const s = c.series[0], hi = Math.max(1e-6, ...s.values.map((v) => v || 0));
    return `<div class="cv-hb">${c.labels.map((l, i) => { const v = s.values[i] || 0; return `<div class="hb"><span class="hb-l">${esc(l)}</span><span class="hb-t"><i class="k${s.color}" style="transform:scaleX(${(Math.max(0, v) / hi).toFixed(4)})"></i></span><span class="hb-v">${valueLabel(v, c.digits)}</span></div>`; }).join('')}</div>`;
  };
  const plot = (c) => {
    if (c.kind === 'hbar') return hbar(c);
    const [lo, hi] = domain(c);
    const inv = isRank(c), ts = inv ? [1].concat(ticks(0, hi, 4, c.digits === 0 ? 1 : 0).filter((t) => t > 1 && t <= hi)) : ticks(lo, hi, 4, c.digits === 0 ? 1 : 0).filter((t) => t >= lo && t <= hi);
    const ys = ts.map((t) => `<span style="bottom:${ypct(c, t, lo, hi).toFixed(2)}%"><b>${inv ? '#' : ''}${tickLabel(t, c.digits)}</b></span>`).join('');
    let marks;
    if (c.kind === 'line') {
      const paths = c.series.map((s) => (c.series.length === 1 && !inv ? `<path class="ar k${s.color}" d="${areaPath(s.values, lo, hi)}"/>` : '') + `<path class="ln k${s.color}" d="${linePath(c, s.values, lo, hi)}" vector-effect="non-scaling-stroke"/>`).join('');
      let labs = '';
      if (c.series.length > 1) {
        const ends = c.series.map((s) => { const v = s.values.filter((x) => x != null).pop(); return v == null ? null : { s, y: 100 - ypct(c, v, lo, hi) }; }).filter(Boolean).sort((a, b) => a.y - b.y);
        for (let i = 1; i < ends.length; i++) if (ends[i].y - ends[i - 1].y < 9) ends[i].y = ends[i - 1].y + 9;
        labs = `<div class="cv-dl" aria-hidden="true">${ends.map((e) => `<span class="k${e.s.color}" style="top:${e.y.toFixed(2)}%">${esc(e.s.name)}</span>`).join('')}</div>`;
      }
      marks = `<svg class="cv-svg" viewBox="0 0 1000 1000" preserveAspectRatio="none" aria-hidden="true">${paths}</svg>${labs}`;
    } else {
      marks = `<div class="cv-bars" aria-hidden="true">${c.labels.map((_, i) => `<div class="cv-col">${c.series.map((s) => { const v = s.values[i]; return v > 0 ? `<i class="k${s.color}" style="height:${pct(v, lo, hi).toFixed(2)}%"></i>` : ''; }).join('')}</div>`).join('')}</div>`;
    }
    return `<div class="cv-plot${c.kind === 'line' && c.series.length > 1 ? ' gut' : ''}"><div class="cv-y" aria-hidden="true">${ys}</div><div class="cv-area">${marks}</div></div><div class="cv-x" aria-hidden="true">${xLabels(c)}</div>`;
  };
  const current = (g, fig) => {
    const v = g.views.find((x) => x.key === g.view) || g.views[0], r = g.ranges.find((x) => x.key === g.range);
    let c = v.chart, n = r ? r.n : 0;
    if (n > 0 && n < c.labels.length) c = { ...c, labels: c.labels.slice(-n), series: c.series.map((s) => ({ ...s, values: s.values.slice(-n) })) };
    const off = (state.get(g.id + '-off') || '').split(',');
    const shown = c.series.filter((s) => !off.includes(s.key));
    return { ...c, all: c.series, series: shown.length ? shown : c.series, fig };
  };
  const totals = (cv) => $$('.cv-col', cv).map((col) => $$('i', col).reduce((s, i) => s + parseFloat(i.style.height || 0), 0));
  const scales = (cv) => $$('.hb-t i', cv).map((i) => parseFloat((i.style.transform.match(/[\d.]+/) || [0])[0]));
  const draw = (fig, animate) => {
    const g = groups[fig.dataset.group]; if (!g) return;
    const c = current(g, fig), cv = $('.cv', fig);
    const before = totals(cv), oldSvg = $('.cv-svg', cv), oldHb = scales(cv);
    cv.innerHTML = plot(c); cv._c = c;
    const table = $('table.sr-only', fig);
    if (table) table.innerHTML = `<caption>${esc($('.cv-cap h2, .cv-cap h3', fig).textContent)}</caption><thead><tr><th scope="col">Point</th>${c.series.map((s) => `<th scope="col">${esc(s.name)}</th>`).join('')}</tr></thead><tbody>${c.labels.map((l, i) => `<tr><th scope="row">${esc(l)}</th>${c.series.map((s) => `<td>${s.values[i] == null ? '' : valueLabel(s.values[i], c.digits)}</td>`).join('')}</tr>`).join('')}</tbody>`;
    const chips = $('.schips', fig);
    if (chips) $$('button', chips).forEach((b) => b.setAttribute('aria-pressed', String(c.series.some((s) => s.key === b.dataset.s))));
    if (!animate || calm()) return;
    if (c.kind === 'hbar') {
      const now = scales(cv);
      $$('.hb-t i', cv).forEach((i, k) => i.animate([{ transform: `scaleX(${oldHb.length === now.length ? oldHb[k] : 0})` }, { transform: `scaleX(${now[k]})` }], { duration: 450, easing: 'cubic-bezier(.2,.8,.2,1)' }));
      return;
    }
    const after = totals(cv);
    if (after.length && after.length === before.length) {
      $$('.cv-col', cv).forEach((col, i) => { if (after[i] > 0 && before[i] !== after[i]) col.animate([{ transform: `scaleY(${Math.max(0.02, before[i] / after[i])})` }, { transform: 'none' }], { duration: 420, easing: 'cubic-bezier(.2,.8,.2,1)' }); });
    } else {
      const plotEl = $('.cv-plot', cv);
      plotEl.animate([{ opacity: 0.25, transform: 'translateY(6px)' }, { opacity: 1, transform: 'none' }], { duration: 320, easing: 'cubic-bezier(.2,.8,.2,1)' });
      if (oldSvg) { oldSvg.classList.add('old'); oldSvg.style.position = 'absolute'; $('.cv-area', cv).appendChild(oldSvg); oldSvg.animate([{ opacity: 0.6 }, { opacity: 0 }], { duration: 300 }).onfinish = () => oldSvg.remove(); }
    }
  };
  const clear = (cv) => { cv.classList.remove('on'); $$('.cv-hair,.cv-pt', cv).forEach((e) => e.remove()); const b = $('.cv-bars', cv) || $('.cv-hb', cv); if (b) { b.classList.remove('hov'); $$('.hl', b).forEach((e) => e.classList.remove('hl')); } hideTip(); };
  const focusPoint = (cv, i) => {
    const c = cv._c; if (!c) return;
    i = Math.max(0, Math.min(c.labels.length - 1, i)); cv._i = i;
    if (c.kind === 'hbar') {
      const box = $('.cv-hb', cv), rows = $$('.hb', box); box.classList.add('hov'); rows.forEach((r, j) => r.classList.toggle('hl', j === i));
      cv.classList.add('on');
      const b = $('.hb-t', rows[i]).getBoundingClientRect(), s = c.series[0];
      showTip(`<b>${esc(c.labels[i])}</b><br><i class="k${s.color}"></i><b>${valueLabel(s.values[i] || 0, c.digits)}</b> ${esc(c.unit)}`, b.left + b.width * Math.min(1, (s.values[i] || 0) / Math.max(1e-6, ...s.values.map((v) => v || 0))), b.top);
      return;
    }
    const plotEl = $('.cv-area', cv), r = plotEl.getBoundingClientRect(), [lo, hi] = domain(c);
    $$('.cv-hair,.cv-pt', cv).forEach((e) => e.remove());
    const x = xAt(c, i) / 100 * r.width;
    cv.classList.add('on');
    if (c.kind === 'line') {
      const hair = D.createElement('i'); hair.className = 'cv-hair'; hair.style.transform = `translateX(${x}px)`; plotEl.appendChild(hair);
      c.series.forEach((s) => { const v = s.values[i]; if (v == null) return; const p = D.createElement('i'); p.className = 'cv-pt k' + s.color; p.style.transform = `translate(${x}px,${(100 - ypct(c, v, lo, hi)) / 100 * r.height}px)`; plotEl.appendChild(p); });
    } else {
      const bars = $('.cv-bars', cv); bars.classList.add('hov'); $$('.cv-col', bars).forEach((col, j) => col.classList.toggle('hl', j === i));
    }
    const rows = c.series.map((s) => s.values[i] == null ? '' : `<br><i class="k${s.color}"></i>${c.series.length > 1 ? esc(s.name) + ': ' : ''}<b>${valueLabel(s.values[i], c.digits)}</b> ${esc(c.unit)}`).join('');
    const total = c.kind === 'stack' && c.series.length > 1 ? `<br>Total: <b>${valueLabel(c.series.reduce((a, s) => a + (s.values[i] || 0), 0), c.digits)}</b>` : '';
    showTip(`<b>${esc(c.labels[i])}</b>${rows}${total}`, r.left + x, r.top + 10);
  };
  const indexAt = (cv, clientX, clientY) => {
    if (cv._c.kind === 'hbar') { const rows = $$('.hb', cv); let k = rows.findIndex((r) => r.getBoundingClientRect().bottom >= clientY); return k < 0 ? rows.length - 1 : k; }
    const c = cv._c, r = $('.cv-area', cv).getBoundingClientRect(), f = (clientX - r.left) / r.width, n = c.labels.length;
    return Math.max(0, Math.min(n - 1, c.kind === 'line' ? Math.round(f * (n - 1)) : Math.floor(f * n)));
  };
  $$('figure[data-group]').forEach((fig) => {
    const g = groups[fig.dataset.group], cv = $('.cv', fig); if (!g) return;
    const v0 = g.view, r0 = g.range;
    if (state.get(g.id + '-v') && g.views.some((v) => v.key === state.get(g.id + '-v'))) g.view = state.get(g.id + '-v');
    if (state.get(g.id + '-r') && g.ranges.some((v) => v.key === state.get(g.id + '-r'))) g.range = state.get(g.id + '-r');
    $$('.seg[data-ctl]', fig).forEach((seg) => {
      const which = seg.dataset.ctl, cur = which === 'view' ? g.view : g.range;
      $$('button', seg).forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === cur)));
      seg.addEventListener('click', (e) => {
        const b = e.target.closest('button'); if (!b) return;
        $$('button', seg).forEach((x) => x.setAttribute('aria-checked', String(x === b)));
        if (which === 'view') { g.view = b.dataset.v; state.set(g.id + '-v', g.view); } else { g.range = b.dataset.v; state.set(g.id + '-r', g.range); }
        save(); draw(fig, true);
      });
    });
    $$(`[data-vpart][data-for="${g.id}"]`).forEach((ctl) => {
      const part = +ctl.dataset.vpart, isSel = ctl.tagName === 'SELECT';
      const sync = () => { const v = g.view.split('~')[part]; if (isSel) ctl.value = v; else $$('button', ctl).forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === v))); };
      const set = (v) => { const ps = g.view.split('~'); ps[part] = v; const k = ps.join('~'); if (!g.views.some((x) => x.key === k)) return; g.view = k; state.set(g.id + '-v', k); save(); sync(); draw(fig, true); };
      sync();
      if (isSel) ctl.addEventListener('change', () => set(ctl.value));
      else ctl.addEventListener('click', (e) => { const b = e.target.closest('button'); if (b) set(b.dataset.v); });
      fig.addEventListener('pulse:view', sync);
    });
    const chips = $('.schips', fig);
    if (chips) chips.addEventListener('click', (e) => {
      const b = e.target.closest('button'); if (!b) return;
      const off = new Set((state.get(g.id + '-off') || '').split(',').filter(Boolean));
      if (off.has(b.dataset.s)) off.delete(b.dataset.s); else off.add(b.dataset.s);
      const all = $$('button', chips).map((x) => x.dataset.s);
      if (all.every((k) => off.has(k))) off.delete(b.dataset.s);
      if (off.size) state.set(g.id + '-off', [...off].join(',')); else state.delete(g.id + '-off');
      save(); draw(fig, true);
    });
    // The server already drew the default view; redraw only a remembered one.
    if (g.view !== v0 || g.range !== r0 || state.get(g.id + '-off')) draw(fig, false); else cv._c = current(g, fig);
    cv.addEventListener('pointermove', (e) => focusPoint(cv, indexAt(cv, e.clientX, e.clientY)));
    cv.addEventListener('pointerdown', (e) => focusPoint(cv, indexAt(cv, e.clientX, e.clientY)));
    cv.addEventListener('pointerleave', (e) => { if (e.pointerType === 'mouse') clear(cv); });
    cv.addEventListener('keydown', (e) => {
      const n = cv._c.labels.length, i = cv._i == null ? n - 1 : cv._i;
      const to = { ArrowLeft: i - 1, ArrowRight: i + 1, Home: 0, End: n - 1, ArrowDown: i - 1, ArrowUp: i + 1 }[e.key];
      if (to != null) { e.preventDefault(); focusPoint(cv, cv._i == null ? n - 1 : to); } else if (e.key === 'Escape') clear(cv);
    });
    cv.addEventListener('blur', () => { clear(cv); cv._i = null; });
  });

  // Maps: nearest point under the pointer, arrow keys between points
  $$('.map').forEach((m) => {
    const svg = $('svg', m);
    const pts = () => $$('circle:not(.off)', m);
    const pick = (c) => {
      $$('circle.hl', m).forEach((x) => x.classList.remove('hl'));
      if (!c) { hideTip(); return; }
      c.classList.add('hl'); m._c = c;
      const r = c.getBoundingClientRect();
      showTip(esc(c.dataset.tip).replace(/ \| /g, '<br>'), r.left + r.width / 2, r.top);
    };
    const near = (e) => {
      const r = svg.getBoundingClientRect(), vb = svg.viewBox.baseVal, k = vb.width / r.width;
      const x = vb.x + (e.clientX - r.left) * k, y = vb.y + (e.clientY - r.top) * k;
      let best = null, bd = (e.pointerType === 'touch' ? 26 : 16) * k;
      pts().forEach((c) => { const d = Math.hypot(c.cx.baseVal.value - x, c.cy.baseVal.value - y) - c.r.baseVal.value; if (d < bd) { bd = d; best = c; } });
      return best;
    };
    m.addEventListener('pointermove', (e) => { if (e.pointerType === 'mouse') pick(near(e)); });
    m.addEventListener('pointerleave', (e) => { if (e.pointerType === 'mouse') pick(null); });
    m.addEventListener('click', (e) => { const c = near(e); pick(c); if (c && c.classList.contains('top1')) egg('shake', m); });
    m.addEventListener('keydown', (e) => {
      const list = pts(); if (!list.length) return;
      const i = list.indexOf(m._c), d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
      if (d) { e.preventDefault(); pick(list[(i + d + list.length) % list.length]); } else if (e.key === 'Escape') pick(null);
      else if (e.key === 'Enter' && m._c && m._c.classList.contains('top1')) egg('shake', m);
    });
    m.addEventListener('blur', () => pick(null));
  });

  // Anything else with a data-tip: hover, tap, focus
  D.addEventListener('pointerover', (e) => { const t = e.target.closest('[data-tip]:not(circle)'); if (t && e.pointerType === 'mouse') { const r = t.getBoundingClientRect(); showTip(esc(t.dataset.tip).replace(/ \| /g, '<br>'), r.left + r.width / 2, r.top); } });
  D.addEventListener('pointerout', (e) => { if (e.target.closest('[data-tip]:not(circle)') && e.pointerType === 'mouse') hideTip(); });
  D.addEventListener('focusin', (e) => { const t = e.target.closest('[data-tip]:not(circle)'); if (t) { const r = t.getBoundingClientRect(); showTip(esc(t.dataset.tip).replace(/ \| /g, '<br>'), r.left + r.width / 2, r.top); } });
  D.addEventListener('click', (e) => { const t = e.target.closest('[data-tip]:not(circle)'); if (t) { const r = t.getBoundingClientRect(); showTip(esc(t.dataset.tip).replace(/ \| /g, '<br>'), r.left + r.width / 2, r.top); } });

  // Filters, pickers and sorts over rows, map dots and cells
  const applyFilter = (name, v, targets, mode) => {
    targets.forEach((t) => {
      const els = t.matches('[data-tags]') ? [t] : $$('[data-tags]', t);
      t.classList.toggle('picking', mode === 'hl' && v !== 'all');
      els.forEach((el) => { const hit = v === 'all' || (' ' + el.dataset.tags + ' ').includes(' ' + v + ' '); if (mode === 'hl') el.classList.toggle('hl', hit && v !== 'all'); else el.classList.toggle('off', !hit); });
      const empty = t.querySelector('.empty'); if (empty) empty.hidden = els.some((x) => !x.classList.contains('off'));
    });
  };
  $$('[data-filter]').forEach((box) => {
    const name = box.dataset.filter, targets = box.dataset.target.split(',').map((s) => $(s)).filter(Boolean), mode = box.dataset.mode;
    const set = (v, push) => {
      if (box.tagName === 'SELECT') box.value = v; else $$('button', box).forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === v)));
      applyFilter(name, v, targets, mode);
      if (push) { if (v === 'all') state.delete('f-' + name); else state.set('f-' + name, v); save(); }
      box.dispatchEvent(new CustomEvent('pulse:filter', { bubbles: true, detail: { name, v } }));
    };
    let init = state.get('f-' + name);
    if (!init && box.tagName !== 'SELECT') { const on = $('button[aria-pressed="true"]', box); if (on && on.dataset.v !== 'all') init = on.dataset.v; }
    if (init && (box.tagName === 'SELECT' ? [...box.options].some((o) => o.value === init) : $(`button[data-v="${CSS.escape(init)}"]`, box))) set(init, false);
    if (box.tagName === 'SELECT') box.addEventListener('change', () => set(box.value, true));
    else box.addEventListener('click', (e) => { const b = e.target.closest('button'); if (b) set(b.dataset.v, true); });
  });
  $$('[data-sort]').forEach((seg) => {
    const list = $(seg.dataset.sort); if (!list) return;
    const set = (v, push) => {
      $$('button', seg).forEach((b) => b.setAttribute('aria-checked', String(b.dataset.v === v)));
      const [k, dir] = v.split(':'); let rows = $$(':scope > [data-tags]', list); if (!rows.length) rows = $$(':scope > li:not(.empty)', list);
      const first = new Map(rows.map((r) => [r, r.getBoundingClientRect().top]));
      rows.sort((a, b) => (parseFloat(b.dataset['k' + k]) - parseFloat(a.dataset['k' + k])) * (dir === 'asc' ? -1 : 1)).forEach((r) => list.appendChild(r));
      const empty = $(':scope > .empty', list); if (empty) list.appendChild(empty);
      rows.forEach((r, i) => { const rk = r.querySelector(':scope > .rk'); if (rk && /^\d+$/.test(rk.textContent.trim())) rk.textContent = i + 1; });
      if (!calm()) rows.slice(0, 40).forEach((r) => { const dy = first.get(r) - r.getBoundingClientRect().top; if (dy) r.animate([{ transform: `translateY(${dy}px)` }, { transform: 'none' }], { duration: 380, easing: 'cubic-bezier(.2,.8,.2,1)' }); });
      if (push) { state.set('s-' + seg.dataset.sort.slice(1), v); save(); }
    };
    const init = state.get('s-' + seg.dataset.sort.slice(1));
    if (init && $(`button[data-v="${CSS.escape(init)}"]`, seg)) set(init, false);
    seg.addEventListener('click', (e) => { const b = e.target.closest('button'); if (b) set(b.dataset.v, true); });
  });

  // Motion: count up once, reveal below-the-fold cards, draw charts in
  if (!calm() && 'IntersectionObserver' in W) {
    const vh = W.innerHeight;
    const io = new IntersectionObserver((es) => es.forEach((en) => {
      if (!en.isIntersecting) return;
      const el = en.target; io.unobserve(el);
      if (el.classList.contains('rv')) { el.style.transitionDelay = (el._d || 0) + 'ms'; el.classList.add('in'); }
      if (el.matches('.cv')) el.classList.add('draw');
      if (el.matches('.num[data-count]')) countUp(el);
    }), { rootMargin: '0px 0px -8% 0px' });
    let d = 0;
    $$('.card,.tile,.stat').forEach((el) => { if (el.getBoundingClientRect().top > vh) { el.classList.add('rv'); el._d = (d++ % 4) * 60; io.observe(el); } });
    $$('.cv').forEach((el) => { if (el.getBoundingClientRect().top > vh) io.observe(el); });
    $$('.num[data-count]').forEach((el) => io.observe(el));
  }
  function countUp(el) {
    const to = parseFloat(el.dataset.count), txt = el.textContent;
    if (!isFinite(to) || to === 0) return;
    const dec = (txt.split('.')[1] || '').replace(/\D.*$/, '').length, suffix = txt.replace(/^[^\d]*[\d,.]+/, ''), lead = txt.match(/^[^\d]*/)[0];
    el.style.minWidth = el.getBoundingClientRect().width + 'px';
    const t0 = performance.now();
    const step = (t) => {
      const k = Math.min(1, (t - t0) / 650), e = 1 - Math.pow(1 - k, 3), v = to * e;
      el.textContent = k < 1 ? lead + (dec ? v.toFixed(dec) : Math.round(v).toLocaleString('en-US')) + suffix : txt;
      if (k < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }

  // Easter eggs, loaded on demand
  let eggs = null;
  function egg(name, arg) {
    if (!eggs) eggs = new Promise((ok) => { const s = D.createElement('script'); s.src = base + 'assets/eggs.js'; s.onload = () => ok(W.PulseEggs); s.onerror = () => ok(null); D.head.appendChild(s); });
    eggs.then((E) => E && E[name] && E[name](arg));
  }
  W.PulseEgg = egg;
  const seq = ['ArrowUp', 'ArrowUp', 'ArrowDown', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'ArrowLeft', 'ArrowRight', 'b', 'a'];
  let ki = 0, typed = '';
  D.addEventListener('keydown', (e) => {
    ki = e.key === seq[ki] || e.key.toLowerCase() === seq[ki] ? ki + 1 : (e.key === seq[0] ? 1 : 0);
    if (ki === seq.length) { ki = 0; egg('retro'); }
    if (e.target.closest('input,textarea,select')) return;
    typed = (typed + (e.key.length === 1 ? e.key.toLowerCase() : '')).slice(-5);
    if (typed === 'sspur') { typed = ''; egg('source'); }
  });
  const logo = $('.logo');
  if (logo) {
    let t = 0;
    logo.addEventListener('pointerdown', () => { t = setTimeout(() => { t = -1; egg('retro'); }, 650); });
    const end = (e) => { if (t === -1 && e.type === 'click') e.preventDefault(); clearTimeout(t); if (e.type === 'click') t = 0; };
    ['pointerup', 'pointerleave', 'pointercancel', 'click'].forEach((ev) => logo.addEventListener(ev, end));
    logo.addEventListener('contextmenu', (e) => e.preventDefault());
  }
  D.addEventListener('pulse:hot', (e) => egg('weather', e.detail));
  D.addEventListener('click', (e) => { const h = e.target.closest('[data-hot]'); if (h) egg('weather', { kind: h.dataset.hot, el: h.closest('.card') || h }); });
  $$('[data-pickview]').forEach((el) => el.addEventListener('click', (e) => {
    const pv = el.dataset.pickview, gid = pv.slice(0, pv.indexOf(':')), v = pv.slice(pv.indexOf(':') + 1), sel = $(`select[data-vpart="0"][data-for="${gid}"]`);
    if (sel && [...sel.options].some((o) => o.value === v)) { sel.value = v; sel.dispatchEvent(new Event('change')); }
  }));
  if (W.console) console.log('%cSSPUR Pulse%c\nEvery number here was computed by SSPUR programs, every 6 hours, for free.\nRead them: https://github.com/utkarshavardhana/sspur/tree/main/pulse\nTry the up up down down code, or type "sspur".', 'font:800 20px system-ui;color:#d9381e', 'font:13px system-ui');
})();
