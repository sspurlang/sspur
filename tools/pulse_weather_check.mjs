// Pulse local weather checks, with the location and weather APIs mocked: a place near a board city
// (Gurugram, next to Delhi) opens the weather board on "Your location: Gurugram" with its own charts,
// from IP and from precise location, with the same reading as the front page; a board city opens on
// itself; "Forget my place" goes back to the IP estimate at once; condition words follow the WMO code.
//   PLAYWRIGHT=/path/to/node_modules/playwright/index.mjs BASE=http://localhost:8795/ node tools/pulse_weather_check.mjs
const pw = await import(process.env.PLAYWRIGHT || 'playwright');
const BASE = process.env.BASE || 'http://localhost:8795/';
const GURUGRAM = { lat: 28.4595, lon: 77.0266 }, BHOPAL = { lat: 23.26, lon: 77.41 }, DELHI = { lat: 28.61, lon: 77.21 };
const problems = [];
let checks = 0;
const check = (ok, m) => { checks++; if (!ok) { problems.push(m); console.log('FAIL ' + m); } };

const mock = async (ctx, ip) => {
  await ctx.route(/bigdatacloud\.net/, (r) => {
    const u = new URL(r.request().url()), la = +u.searchParams.get('latitude'), named = (n) => ({ latitude: la || ip.lat, longitude: +u.searchParams.get('longitude') || ip.lon, city: '', locality: n, countryName: 'India' });
    const at = la ? { lat: la } : ip, name = Math.abs(at.lat - GURUGRAM.lat) < 0.01 ? 'Gurugram' : Math.abs(at.lat - DELHI.lat) < 0.01 ? 'Delhi' : 'Bhopal';
    r.fulfill({ status: 200, contentType: 'application/json', headers: { 'access-control-allow-origin': '*' }, body: JSON.stringify(named(name)) });
  });
  await ctx.route(/open-meteo\.com/, (r) => {
    const u = new URL(r.request().url()), la = +u.searchParams.get('latitude');
    const h = Array.from({ length: 48 }, (_, i) => `2026-10-${13 + Math.floor(i / 24)}T${String(i % 24).padStart(2, '0')}:00`);
    const d = Array.from({ length: 31 }, (_, i) => new Date(Date.UTC(2026, 8, 14 + i)).toISOString().slice(0, 10));
    const t = 20 + la / 4; // a temperature that differs by place
    r.fulfill({ status: 200, contentType: 'application/json', headers: { 'access-control-allow-origin': '*' }, body: JSON.stringify({ current: { time: '2026-10-14T15:15', temperature_2m: t, weather_code: 2, is_day: 1 }, hourly: { time: h, temperature_2m: h.map((_, i) => t - 4 + 4 * Math.sin(i / 4)), precipitation: h.map((_, i) => (i % 9 === 0 ? 1.2 : 0)), wind_speed_10m: h.map((_, i) => 8 + (i % 5)) }, daily: { time: d, temperature_2m_max: d.map((_, i) => t + 3 + (i % 4)), temperature_2m_min: d.map((_, i) => t - 6 + (i % 3)), precipitation_sum: d.map((_, i) => (i % 6) * 0.8), wind_speed_10m_max: d.map((_, i) => 14 + (i % 7)) } }) });
  });
};
const card = (p) => p.evaluate(() => ({ name: document.querySelector('.lwx-name').textContent, v: document.querySelector('.lwx-v').textContent, word: document.querySelector('.lwx-n').textContent, href: document.querySelector('.lwx').getAttribute('href') }));
const board = (p) => p.evaluate(() => { const s = document.querySelector('select[data-vpart="0"][data-for="city"]'), now = document.querySelector('#cityfig .here-now'); return { value: s.value, text: s.selectedOptions[0].textContent, paths: document.querySelectorAll('#cityfig .cv .ln').length, now: now && !now.hidden ? now.textContent : '' }; });
const homeReady = (p, name) => p.waitForFunction((n) => document.querySelector('.lwx-ok') && document.querySelector('.lwx-name').textContent === n, name, { timeout: 10000 }).catch(() => {});
const openBoard = async (p, c) => { await p.click('.lwx'); await p.waitForURL(/weather\//); await p.waitForTimeout(1500); return board(p); };

for (const [engine, dev] of [['webkit', { w: 390, h: 844, mobile: true }], ['chromium', { w: 1440, h: 900, mobile: false }]]) {
  const browser = await pw[engine].launch();
  const tag = `${engine} ${dev.w}`;
  // 1. From IP, in Gurugram
  {
    const ctx = await browser.newContext({ viewport: { width: dev.w, height: dev.h }, isMobile: dev.mobile && engine !== 'firefox', hasTouch: dev.mobile });
    await mock(ctx, GURUGRAM);
    const p = await ctx.newPage();
    await p.goto(BASE); await homeReady(p, 'Gurugram');
    const c = await card(p);
    check(c.name === 'Gurugram', `${tag}: the front page says ${c.name}, not Gurugram`);
    check(c.word === 'Partly cloudy', `${tag}: code 2 reads "${c.word}", not Partly cloudy`);
    check(/city-v=here~/.test(c.href), `${tag}: the card links to ${c.href}`);
    const b = await openBoard(p);
    check(b.value === 'here' && b.text === 'Your location: Gurugram', `${tag}: the board opened on ${b.text} (${b.value})`);
    check(b.paths > 0, `${tag}: no chart for Your location`);
    check(b.now.includes(c.v + 'C') && b.now.includes('Partly cloudy'), `${tag}: the board reads "${b.now}" but the front page ${c.v}`);
    await p.reload(); await p.waitForTimeout(1500);
    const b2 = await board(p);
    check(b2.value === 'here', `${tag}: after a reload the board is on ${b2.text}`);
    await p.selectOption('select[data-vpart="0"][data-for="city"]', 'delhi'); await p.waitForTimeout(300);
    check((await board(p)).now === '', `${tag}: the Your location reading stays up for Delhi`);
    await ctx.close();
  }
  // 2. Precise location in Gurugram, IP in Bhopal; then forget it
  {
    const ctx = await browser.newContext({ viewport: { width: dev.w, height: dev.h }, isMobile: dev.mobile, hasTouch: dev.mobile, permissions: ['geolocation'], geolocation: { latitude: GURUGRAM.lat, longitude: GURUGRAM.lon } });
    await mock(ctx, BHOPAL);
    const p = await ctx.newPage();
    await p.goto(BASE); await homeReady(p, 'Bhopal');
    await p.click('[data-lwx-menu]'); await p.click('[data-act="precise"]');
    await homeReady(p, 'Gurugram');
    const c = await card(p);
    check(c.name === 'Gurugram' && !(await p.$('.psheet')), `${tag}: precise location shows ${c.name}${(await p.$('.psheet')) ? ' with the sheet open' : ''}`);
    const b = await openBoard(p);
    check(b.text === 'Your location: Gurugram' && b.paths > 0, `${tag}: precise: the board opened on ${b.text}`);
    check(b.now.includes(c.v + 'C'), `${tag}: precise: board "${b.now}" against ${c.v}`);
    await p.goto(BASE); await homeReady(p, 'Gurugram');
    await p.click('[data-lwx-menu]');
    const forget = await p.$('[data-act="forget"]');
    check(!!forget, `${tag}: no Forget my place button`);
    if (forget) {
      await forget.click();
      await homeReady(p, 'Bhopal');
      const after = await card(p), stored = await p.evaluate(() => localStorage.getItem('pulse-place'));
      check(after.name === 'Bhopal', `${tag}: after Forget the card says ${after.name}`);
      check(stored === null, `${tag}: Forget left ${stored} in storage`);
      check(!(await p.$('.psheet')), `${tag}: Forget left the sheet open`);
      check(!(await p.evaluate(() => document.documentElement.classList.contains('pulse-lock'))), `${tag}: Forget left the page locked`);
    }
    await ctx.close();
  }
  // 3. A board city is itself
  {
    const ctx = await browser.newContext({ viewport: { width: dev.w, height: dev.h }, isMobile: dev.mobile, hasTouch: dev.mobile });
    await mock(ctx, DELHI);
    const p = await ctx.newPage();
    await p.goto(BASE); await homeReady(p, 'Delhi');
    const c = await card(p);
    check(/city-v=delhi~/.test(c.href), `${tag}: Delhi links to ${c.href}`);
    const b = await openBoard(p);
    check(b.value === 'delhi' && !b.text.startsWith('Your location'), `${tag}: Delhi opened on ${b.text}`);
    await ctx.close();
  }
  await browser.close();
}
console.log(`${checks} weather checks, ${problems.length} problems`);
process.exit(problems.length ? 1 : 0);
