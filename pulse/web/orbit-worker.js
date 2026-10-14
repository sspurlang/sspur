// SSPUR Pulse: SGP4 for every satellite on the globe, off the main thread.
// Elements arrive as compact arrays [id, name, yy, day, bstar, inc, raan, ecc*1e7, argp, ma, mm]
// and become two-line element sets for satellite.js (MIT).
/* global satellite */
importScripts('vendor/satellite.min.js');
const recs = [];
const put = (a, at, s) => { for (let i = 0; i < s.length; i++) a[at + i] = s[i]; };
const fix = (x, w, d) => x.toFixed(d).padStart(w, ' ');
const bexp = (x) => {
  if (!x) return [' ', '00000', '+0'];
  const e = Math.floor(Math.log10(Math.abs(x))) + 1;
  let m = Math.round(Math.abs(x) / Math.pow(10, e) * 1e5);
  let ee = e;
  if (m >= 1e5) { m = Math.round(m / 10); ee += 1; }
  return [x < 0 ? '-' : ' ', String(m).padStart(5, '0'), (ee < 0 ? '-' : '+') + Math.min(9, Math.abs(ee))];
};
const tle = (s) => {
  const [id, , yy, day, bstar, inc, raan, ecc, argp, ma, mm] = s;
  const l1 = Array(69).fill(' '), l2 = Array(69).fill(' ');
  put(l1, 0, '1 '); put(l1, 2, String(id % 100000).padStart(5, '0')); put(l1, 7, 'U');
  put(l1, 18, String(yy).padStart(2, '0')); put(l1, 20, day.toFixed(8).padStart(12, '0'));
  put(l1, 33, ' .00000000'); put(l1, 44, '00000'); put(l1, 50, '-0');
  const [bs, bm, be] = bexp(bstar); put(l1, 53, bs); put(l1, 54, bm); put(l1, 59, be);
  put(l2, 0, '2 '); put(l2, 2, String(id % 100000).padStart(5, '0'));
  put(l2, 8, fix(inc, 8, 4)); put(l2, 17, fix(raan, 8, 4)); put(l2, 26, String(ecc).padStart(7, '0'));
  put(l2, 34, fix(argp, 8, 4)); put(l2, 43, fix(ma, 8, 4)); put(l2, 52, fix(mm, 11, 8));
  return [l1.join(''), l2.join('')];
};
self.onmessage = (e) => {
  const m = e.data;
  if (m.op === 'add') {
    for (const s of m.sats) {
      let rec = null;
      try { const [a, b] = tle(s); rec = satellite.twoline2satrec(a, b); if (rec.error) rec = null; } catch (er) { rec = null; }
      recs.push(rec);
    }
    self.postMessage({ op: 'added', n: recs.length });
  } else if (m.op === 'at') {
    const d = new Date(m.t), pos = new Float32Array(recs.length * 3);
    for (let i = 0; i < recs.length; i++) {
      const r = recs[i]; let p = null;
      if (r) { const pv = satellite.propagate(r, d); p = pv && pv.position; }
      if (p && isFinite(p.x)) { pos[3 * i] = p.x; pos[3 * i + 1] = p.y; pos[3 * i + 2] = p.z; } else pos[3 * i] = NaN;
    }
    self.postMessage({ op: 'at', t: m.t, pos }, [pos.buffer]);
  } else if (m.op === 'one') {
    const r = recs[m.i], pv = r && satellite.propagate(r, new Date(m.t));
    self.postMessage({ op: 'one', i: m.i, t: m.t, p: pv && pv.position ? [pv.position.x, pv.position.y, pv.position.z] : null, gmst: satellite.gstime(new Date(m.t)), mm: r ? r.no * 1440 / (2 * Math.PI) : 0 });
  } else if (m.op === 'info') {
    const r = recs[m.i]; if (!r) { self.postMessage({ op: 'info', i: m.i, ok: false }); return; }
    const d = new Date(m.t), pv = satellite.propagate(r, d), g = satellite.gstime(d);
    const geo = satellite.eciToGeodetic(pv.position, g), v = pv.velocity;
    const period = 2 * Math.PI / r.no, track = [];
    for (let k = -50; k <= 50; k++) {
      const dt = new Date(m.t + k * period / 100 * 60000), q = satellite.propagate(r, dt);
      if (!q.position) continue;
      const gg = satellite.eciToGeodetic(q.position, satellite.gstime(dt));
      track.push([satellite.degreesLat(gg.latitude), satellite.degreesLong(gg.longitude)]);
    }
    self.postMessage({ op: 'info', i: m.i, ok: true, lat: satellite.degreesLat(geo.latitude), lon: satellite.degreesLong(geo.longitude), alt: geo.height, speed: Math.hypot(v.x, v.y, v.z), period, track });
  }
};
