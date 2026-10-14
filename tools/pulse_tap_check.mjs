// Pulse touch checks: every section chip in the top nav navigates on one tap, in WebKit and
// Chromium mobile emulation, after the chip bar has been scrolled sideways; nothing covers the
// nav; a touch whose click the browser swallows (as iOS and Android do for a tap that stops a
// glide) still activates the control, and a control is never activated twice by one tap.
//   python3 -m http.server -d pulse/out 8795 &
//   PLAYWRIGHT=/path/to/node_modules/playwright/index.mjs BASE=http://localhost:8795/ node tools/pulse_tap_check.mjs
const pw = await import(process.env.PLAYWRIGHT || 'playwright');
const BASE = process.env.BASE || 'http://localhost:8795/';
const devices = [
  { engine: 'webkit', id: 'iphone-14', w: 390, h: 844, scale: 3 },
  { engine: 'webkit', id: 'iphone-se', w: 320, h: 568, scale: 2 },
  { engine: 'chromium', id: 'pixel-7', w: 412, h: 915, scale: 2.6 },
];
const problems = [];
let checks = 0;
const fail = (m) => { problems.push(m); console.log('FAIL ' + m); };

// The chip's centre once it is in view, after scrolling the bar to the far end and back.
const scrollBarTo = (p, i) => p.evaluate(async (i) => {
  const row = document.querySelector('.chips-row'), chip = row.querySelectorAll('.chip')[i];
  row.scrollLeft = chip.offsetLeft > row.scrollWidth / 2 ? 0 : row.scrollWidth;
  await new Promise((r) => setTimeout(r, 120));
  const b0 = chip.getBoundingClientRect(), rb = row.getBoundingClientRect();
  row.scrollLeft += b0.left + b0.width / 2 - (rb.left + rb.width / 2);
  await new Promise((r) => setTimeout(r, 200));
  const b = chip.getBoundingClientRect();
  return { x: b.left + b.width / 2, y: b.top + b.height / 2, href: chip.href, scrolled: row.scrollWidth > row.clientWidth };
}, i);

// What sits on top at each chip's centre, for the chips that are in view.
const covered = (p) => p.evaluate(() => [...document.querySelectorAll('.chips-row .chip')].flatMap((c) => {
  const b = c.getBoundingClientRect(), x = b.left + b.width / 2, y = b.top + b.height / 2;
  if (x < 0 || x > innerWidth || y < 0 || y > innerHeight) return [];
  const top = document.elementFromPoint(x, y);
  return top && (top === c || c.contains(top)) ? [] : [`${c.textContent.trim()} under ${top ? top.tagName.toLowerCase() + '.' + top.className : 'nothing'}`];
}));

// A touch that the browser does not turn into a click.
const swallowedTap = (p, sel) => p.evaluate((sel) => {
  const el = document.querySelector(sel), b = el.getBoundingClientRect(), x = b.left + b.width / 2, y = b.top + b.height / 2;
  const o = { bubbles: true, cancelable: true, composed: true, pointerType: 'touch', isPrimary: true, pointerId: 7, clientX: x, clientY: y };
  el.dispatchEvent(new PointerEvent('pointerdown', o)); el.dispatchEvent(new PointerEvent('pointerup', o));
}, sel);

for (const dev of devices) {
  const browser = await pw[dev.engine].launch();
  const ctx = await browser.newContext({ viewport: { width: dev.w, height: dev.h }, deviceScaleFactor: dev.scale, isMobile: true, hasTouch: true });
  await ctx.route(/bigdatacloud\.net|open-meteo\.com/, (r) => r.abort());
  const p = await ctx.newPage();
  const tag = `${dev.engine} ${dev.id}`;
  for (const start of ['', 'launches/', 'holidays/']) {
    await p.goto(BASE + start);
    const n = await p.$$eval('.chips-row .chip', (cs) => cs.length);
    for (let i = 0; i < n; i++) {
      await p.goto(BASE + start);
      if (i % 2) await p.evaluate(() => window.scrollTo(0, document.body.scrollHeight / 2));
      const c = await scrollBarTo(p, i);
      for (const m of await covered(p)) fail(`${tag} /${start}: ${m}`);
      if (c.href === p.url()) continue;
      await p.touchscreen.tap(c.x, c.y);
      try { await p.waitForURL(c.href, { timeout: 4000 }); } catch (e) { fail(`${tag} /${start}: one tap on chip ${i} did not open ${c.href} (at ${p.url()})`); }
      checks++;
    }
    // The same chips when the browser swallows the click
    await p.goto(BASE + start);
    const c = await scrollBarTo(p, n - 1);
    await swallowedTap(p, `.chips-row .chip:nth-child(${n})`);
    try { await p.waitForURL(c.href, { timeout: 4000 }); } catch (e) { fail(`${tag} /${start}: a swallowed tap on the last chip did not navigate`); }
    checks++;
  }
  // Toggles: one tap flips once, a swallowed tap flips once too
  await p.goto(BASE + 'launches/');
  const flip = async (sel, attr, how) => {
    const before = await p.$eval(sel, (e, a) => e.getAttribute(a), attr);
    if (how === 'tap') await p.tap(sel); else await swallowedTap(p, sel);
    await p.waitForTimeout(600);
    const after = await p.$eval(sel, (e, a) => e.getAttribute(a), attr);
    checks++;
    if (after === before) fail(`${tag}: ${how} on ${sel} did not toggle ${attr} (${before})`);
  };
  await p.$eval('#orbit', (e) => e.scrollIntoView());
  await p.waitForFunction(() => window.PulseOrbit && window.PulseOrbit.count > 0, null, { timeout: 30000 });
  await flip('[data-orb-layers]', 'aria-expanded', 'tap');
  await p.waitForTimeout(400);
  for (const m of await p.evaluate(() => { const b = document.querySelector('[data-og="oneweb"]').getBoundingClientRect(), t = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2); return t && t.closest('[data-og="oneweb"]') ? [] : ['the Layers sheet row is covered by ' + (t && t.className)]; })) fail(`${tag}: ${m}`);
  await flip('[data-og="oneweb"]', 'aria-checked', 'tap');
  await flip('[data-og="oneweb"]', 'aria-checked', 'swallowed');
  await flip('[data-ofam="nav"]', 'aria-checked', 'tap');
  if (!/o-off=[^&]*gps/.test(await p.evaluate(() => decodeURIComponent(location.hash)))) fail(`${tag}: hiding Navigation is not in the URL`);
  await p.tap('[data-orb-close].orb-done');
  await p.waitForTimeout(400);
  await flip('[data-band="0"]', 'aria-checked', 'tap');
  await flip('[data-speed="600"]', 'aria-checked', 'swallowed');
  await p.$eval('[data-secrets]', (e) => e.scrollIntoView({ block: 'center' }));
  await p.tap('[data-secrets]');
  await p.waitForTimeout(800);
  checks++;
  if (!(await p.$('.egg-sheet'))) fail(`${tag}: one tap on the secrets counter did not open the list`);
  else if ((await p.$$('.egg-sheet')).length > 1) fail(`${tag}: one tap opened the secrets list twice`);
  // The view survives a reload
  await p.reload();
  await p.$eval('#orbit', (e) => e.scrollIntoView());
  await p.waitForFunction(() => window.PulseOrbit && window.PulseOrbit.count > 0, null, { timeout: 30000 });
  await p.waitForTimeout(300);
  const kept = await p.evaluate(() => [document.querySelector('[data-ofam="nav"]').getAttribute('aria-checked'), document.querySelector('[data-band="0"]').getAttribute('aria-checked')]);
  checks++;
  if (kept[0] !== 'false' || kept[1] !== 'true') fail(`${tag}: the globe's view did not survive a reload (${kept})`);
  // Holding the logo cycles normal, retro, night sky, normal, every time; the menu never opens
  await p.goto(BASE + 'launches/');
  await p.evaluate(() => localStorage.removeItem('pulse-eggs'));
  // Desktop WebKit has no -webkit-touch-callout and drops it from the CSSOM: read the styles' text
  const hold = () => p.evaluate(async () => {
    const calloutRule = () => { const raw = [...document.querySelectorAll('style')].map((x) => x.textContent).join('\n'); return /\.logo,\.logo \*\{-webkit-touch-callout:none!important/.test(raw) && !/touch-callout:\s*(?!none)/.test(raw) ? 'none' : 'not none'; };
    const logo = document.querySelector('.logo'), b = logo.getBoundingClientRect(), x = b.left + 20, y = b.top + b.height / 2;
    const o = { bubbles: true, cancelable: true, composed: true, pointerType: 'touch', isPrimary: true, pointerId: 9, clientX: x, clientY: y };
    const target = document.elementFromPoint(x, y);
    target.dispatchEvent(new PointerEvent('pointerdown', o));
    await new Promise((r) => setTimeout(r, 560));
    const menu = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y });
    target.dispatchEvent(menu);
    await new Promise((r) => setTimeout(r, 300));
    target.dispatchEvent(new PointerEvent('pointerup', o));
    await new Promise((r) => setTimeout(r, 900));
    const cs = getComputedStyle(logo), top = getComputedStyle(document.querySelector('.top'));
    return { egg: document.documentElement.dataset.egg || 'normal', menuBlocked: menu.defaultPrevented, callout: CSS.supports('-webkit-touch-callout', 'none') ? cs.getPropertyValue('-webkit-touch-callout') : calloutRule(logo), select: cs.webkitUserSelect || cs.userSelect, onTop: target.closest('.logo') !== null, header: +top.zIndex, toasts: document.querySelectorAll('.egg-toast').length };
  });
  const cycle = ['crt', 'sky', 'normal', 'crt', 'sky', 'normal'];
  for (const [k, want] of cycle.entries()) {
    const h = await hold();
    checks++;
    if (h.egg !== want) fail(`${tag}: logo hold ${k + 1} gave ${h.egg}, not ${want}`);
    if (!h.menuBlocked) fail(`${tag}: logo hold ${k + 1} let the context menu through`);
    if (dev.engine === 'webkit' && h.callout !== 'none') fail(`${tag}: logo hold ${k + 1}: -webkit-touch-callout is ${h.callout} in ${h.egg}`);
    if (h.select !== 'none') fail(`${tag}: logo hold ${k + 1}: user-select is ${h.select}`);
    if (!h.onTop) fail(`${tag}: logo hold ${k + 1}: something covers the logo`);
    if (h.egg === 'crt' && h.header <= 60) fail(`${tag}: the retro scanlines cover the header`);
    if (k === 0 && h.toasts !== 1) fail(`${tag}: the first retro find showed ${h.toasts} toasts`);
    if (k > 0 && h.toasts) fail(`${tag}: retro toggle ${k + 1} still shows a toast`);
  }
  for (const want of ['crt']) {
    await hold();
    const lb = await p.$eval('.logo', (e) => { const b = e.getBoundingClientRect(); return { x: b.left + 20, y: b.top + b.height / 2 }; });
    await p.touchscreen.tap(lb.x, lb.y);
    try { await p.waitForURL(BASE, { timeout: 4000 }); } catch (e) { fail(`${tag}: a tap on the logo in ${want} did not go home`); }
    checks++;
  }
  await browser.close();
}
console.log(`${checks} tap checks, ${problems.length} problems`);
process.exit(problems.length ? 1 : 0);
