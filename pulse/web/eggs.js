// SSPUR Pulse easter eggs, loaded only when someone finds one.
(() => {
  'use strict';
  const D = document, R = D.documentElement;
  const calm = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  const modal = () => window.PulseModal || { open() {}, close() {} };
  const css = `
[data-egg=crt] body{background:#031207;color:#7dffa8;text-shadow:0 0 6px rgba(80,255,140,.55)}
[data-egg=crt]{--bg:#031207;--card:#05200e;--card2:#08301a;--ink:#a8ffc6;--ink2:#7fe7a2;--ink3:#5fc483;--line:rgba(125,255,168,.18);--grid:rgba(125,255,168,.14);--land:#0a3a1d;--acc:#b6ff5c;--soft:#0f3a12;--k1:#7dffa8;--k2:#b6ff5c;--k3:#5cffd9;--k4:#e6ff5c;--k5:#5cd1ff;--k6:#a8ff7d;--k7:#ffd95c;--display:ui-monospace,Menlo,monospace}
[data-egg=crt] .top{z-index:61}
[data-egg=crt] body::after{content:"";position:fixed;inset:0;pointer-events:none;z-index:60;background:repeating-linear-gradient(0deg,rgba(0,0,0,.28) 0 1px,transparent 1px 3px);mix-blend-mode:multiply}
[data-egg=sky]{--bg:#060a1f;--card:rgba(18,26,60,.78);--card2:rgba(40,52,100,.6);--ink:#eef1ff;--ink2:#c6cdf2;--ink3:#9aa5d8;--line:rgba(255,255,255,.12);--grid:rgba(255,255,255,.1);--land:#1b2552;--acc:#ffd36b;--soft:#2a2a55;color-scheme:dark}
[data-egg=sky] body{background:radial-gradient(1200px 600px at 70% -10%,#1d2a6b,transparent),#060a1f}
.egg-stars{position:fixed;inset:0;pointer-events:none;z-index:0}
.egg-stars i{position:absolute;width:2px;height:2px;border-radius:50%;background:#fff;opacity:.7}
[data-egg=sky] main,[data-egg=sky] .foot{position:relative;z-index:1}
.egg-toast{position:fixed;left:50%;bottom:calc(14px + env(safe-area-inset-bottom));transform:translateX(-50%);z-index:70;background:var(--ink);color:var(--bg);padding:7px 14px;border-radius:999px;font-size:13px;font-weight:600;line-height:1.3;box-shadow:0 6px 18px rgba(0,0,0,.22);max-width:calc(100vw - 32px);width:max-content;text-align:center;pointer-events:none}
.egg-src{position:fixed;inset:auto 10px calc(10px + env(safe-area-inset-bottom)) 10px;z-index:75;max-width:640px;max-height:min(40vh,420px);margin:0 auto;display:flex;flex-direction:column;background:var(--card);color:var(--ink);border-radius:20px;box-shadow:0 18px 50px rgba(0,0,0,.3);padding:10px 14px 12px;font-size:.92rem;touch-action:pan-y}
@media (min-width:700px){.egg-src{max-height:min(56vh,560px)}}
.egg-src::before{content:"";flex:none;align-self:center;width:36px;height:5px;border-radius:3px;margin:0 0 8px;background:color-mix(in srgb,var(--ink) 18%,transparent)}
.egg-bar{display:flex;align-items:center;gap:6px;flex:none}.egg-bar strong{flex:1;min-width:0;font-size:.92rem}
.egg-src pre{flex:1 1 auto;min-height:0;margin:8px 0 0;overflow:auto;background:var(--card2);border-radius:12px;padding:10px 12px;font:12px/1.5 ui-monospace,Menlo,monospace;white-space:pre}
.egg-src button{font:inherit;font-weight:600;min-height:44px;padding:0 14px;border-radius:12px;border:0;background:var(--card2);color:var(--ink);cursor:pointer}
.egg-src a{display:inline-flex;align-items:center;min-height:44px;padding:0 6px;font-weight:600}
.egg-list{overflow:auto;min-height:0}
.egg-fx{position:absolute;inset:0;overflow:hidden;pointer-events:none;border-radius:inherit;z-index:3}
.egg-fx i{position:absolute;top:-10px;width:8px;height:8px;border-radius:50%;background:#fff;box-shadow:0 0 6px rgba(160,200,255,.9)}
.egg-fx b{position:absolute;right:-60px;top:-60px;width:180px;height:180px;border-radius:50%;background:radial-gradient(circle,rgba(255,200,60,.55),rgba(255,140,0,0) 70%)}
.egg-rocket{position:fixed;z-index:70;width:34px;height:34px;pointer-events:none}
.egg-list{list-style:none;margin:10px 0;padding:0}.egg-list li{padding:7px 0;border-top:1px solid var(--line);color:var(--ink3)}.egg-list li.got{color:var(--ink);font-weight:600}.egg-list span{display:inline-block;width:1.4em;color:var(--acc)}
.egg-sheet{max-height:min(66vh,560px)}.egg-sheet p{display:flex;gap:8px;flex:none;margin-top:8px}.egg-reset{background:var(--card2)!important;color:var(--ink)!important}`;
  const style = D.createElement('style'); style.textContent = css; D.head.appendChild(style);
  // Only the first find of each secret says anything: one small pill at the bottom edge.
  const toast = (msg, first, total) => {
    if (!first) return;
    const old = D.querySelector('.egg-toast'); if (old) old.remove();
    const t = D.createElement('div'); t.className = 'egg-toast'; t.setAttribute('role', 'status'); t.textContent = `${msg} · ${first} of ${total}`; D.body.appendChild(t);
    if (!calm()) t.animate([{ opacity: 0, transform: 'translate(-50%,6px)' }, { opacity: 1, transform: 'translate(-50%,0)' }], { duration: 200, easing: 'cubic-bezier(.2,.8,.2,1)' });
    setTimeout(() => t.remove(), 2400);
  };
  // Sheets close on a swipe down from anywhere but a scrolled code block
  const swipe = (box, close) => {
    let y0 = null;
    box.addEventListener('touchstart', (e) => { const pre = e.target.closest('pre,.egg-list'); y0 = pre && pre.scrollTop > 0 ? null : e.touches[0].clientY; }, { passive: true });
    box.addEventListener('touchmove', (e) => { if (y0 == null) return; const dy = Math.max(0, e.touches[0].clientY - y0); box.style.transform = dy ? `translateY(${dy}px)` : ''; }, { passive: true });
    box.addEventListener('touchend', (e) => { if (y0 == null) return; const dy = e.changedTouches[0].clientY - y0; y0 = null; if (dy > 60) close(); else box.style.transform = ''; }, { passive: true });
  };
  const stars = () => {
    let s = D.querySelector('.egg-stars');
    if (s) return s;
    s = D.createElement('div'); s.className = 'egg-stars'; s.setAttribute('aria-hidden', 'true');
    let seed = 7;
    const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
    for (let i = 0; i < 90; i++) { const st = D.createElement('i'); st.style.left = (rnd() * 100) + '%'; st.style.top = (rnd() * 100) + '%'; st.style.opacity = (0.3 + rnd() * 0.7).toFixed(2); s.appendChild(st); if (!calm() && i % 3 === 0) st.animate([{ opacity: 0.2 }, { opacity: 1 }, { opacity: 0.2 }], { duration: 2000 + rnd() * 3000, iterations: Infinity }); }
    D.body.prepend(s); return s;
  };
  window.PulseEggs = {
    iss(info, first, total) { toast('Found: the space station', first, total); },
    lost(_, first, total) { if (first) toast('Found: the lost page. Look where the ISS was', first, total); },
    sheet(btn, _f, _t, api) {
      const names = { retro: 'Retro screens', source: 'The source', shake: 'Aftershock', liftoff: 'Liftoff', weather: 'Weather of its own', iss: 'The space station', lost: 'Lost in orbit' };
      const hints = { retro: 'The logo remembers old computers.', source: 'Knock three times on what\'s fresh.', shake: 'The biggest quake is sensitive.', liftoff: 'Some numbers count down to something.', weather: 'Very cold and very hot places have weather of their own.', iss: 'Someone is always awake up there.', lost: 'Even lost pages know where the ISS is.' };
      const old = D.querySelector('.egg-sheet'); if (old) old.remove();
      const f = api.found();
      const box = D.createElement('div'); box.className = 'egg-src egg-sheet'; box.setAttribute('role', 'dialog'); box.setAttribute('aria-modal', 'true'); box.setAttribute('aria-labelledby', 'egg-sheet-h');
      box.innerHTML = `<strong id="egg-sheet-h">${f.length} of ${api.secrets.length} secrets found</strong><ul class="egg-list">${api.secrets.map((k) => f.includes(k) ? `<li class="got"><span aria-hidden="true">★</span> ${names[k]}</li>` : `<li><span aria-hidden="true">☆</span> ${hints[k]}</li>`).join('')}</ul><p><button type="button" class="egg-reset">Reset</button> <button type="button" class="egg-close">Close</button></p>`;
      D.body.appendChild(box);
      const close = () => { box.remove(); modal().close(); D.removeEventListener('keydown', onKey); if (btn) btn.focus(); };
      modal().open(close); swipe(box, close);
      const onKey = (e) => { if (e.key === 'Escape') close(); if (e.key === 'Tab') { const bs = [...box.querySelectorAll('button')]; const i = bs.indexOf(D.activeElement); if (e.shiftKey && i <= 0) { e.preventDefault(); bs[bs.length - 1].focus(); } else if (!e.shiftKey && i === bs.length - 1) { e.preventDefault(); bs[0].focus(); } } };
      box.querySelector('.egg-close').addEventListener('click', close);
      box.querySelector('.egg-reset').addEventListener('click', () => { api.store('pulse-eggs', null); api.showFound(); close(); });
      D.addEventListener('keydown', onKey);
      box.querySelector('.egg-close').focus();
      if (!calm()) box.animate([{ opacity: 0, transform: 'translateY(20px)' }, { opacity: 1, transform: 'none' }], { duration: 280, easing: 'cubic-bezier(.2,.8,.2,1)' });
    },
    retro(_, first, total) {
      const next = { undefined: 'crt', crt: 'sky', sky: undefined }[R.dataset.egg];
      if (next) R.dataset.egg = next; else delete R.dataset.egg;
      const s = D.querySelector('.egg-stars');
      if (next === 'sky') stars(); else if (s) s.remove();
      toast('Found: retro mode. Hold again for night sky', first, total);
    },
    source(_, first, total) {
      if (first) toast('Found: the source', first, total);
      const old = D.querySelector('.egg-src'); if (old) { old.remove(); modal().close(); return; }
      const tpl = D.getElementById('sspur-src'); if (!tpl) return;
      const box = D.createElement('div'); box.className = 'egg-src'; box.setAttribute('role', 'dialog'); box.setAttribute('aria-label', 'SSPUR source');
      const path = location.pathname.replace(/\/$/, '').split('/').pop();
      box.innerHTML = '<div class="egg-bar"><strong>One of the SSPUR functions behind this page</strong><a href="https://github.com/utkarshavardhana/sspur/tree/main/pulse/' + (/^[a-z]+$/.test(path) && path !== 'pulse' ? path : '') + '">All of it</a><button type="button">Close</button></div><pre></pre>';
      box.querySelector('pre').textContent = tpl.innerHTML.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, '&');
      D.body.appendChild(box);
      const close = () => { box.remove(); modal().close(); D.removeEventListener('keydown', onKey); };
      modal().open(close); swipe(box, close);
      const onKey = (e) => { if (e.key === 'Escape') close(); };
      box.querySelector('button').addEventListener('click', close); D.addEventListener('keydown', onKey);
      box.querySelector('button').focus();
      if (!calm()) box.animate([{ opacity: 0, transform: 'translateY(20px)' }, { opacity: 1, transform: 'none' }], { duration: 300, easing: 'cubic-bezier(.2,.8,.2,1)' });
    },
    shake(map, first, total) {
      toast('Found: the big one', first, total);
      if (calm()) return;
      map.animate([0, -6, 5, -4, 3, -2, 1, 0].map((x, i) => ({ transform: `translate(${x}px,${i % 2 ? 1 : -1}px)` })), { duration: 600, easing: 'ease-out' });
    },
    liftoff(el, first, total) {
      const r = el.getBoundingClientRect();
      const rk = D.createElementNS('http://www.w3.org/2000/svg', 'svg'); rk.setAttribute('viewBox', '0 0 24 24'); rk.setAttribute('class', 'egg-rocket'); rk.setAttribute('aria-hidden', 'true');
      rk.innerHTML = '<path d="M12 2.5c3.2 2.3 4.8 6 4.2 11.2L14 16h-4l-2.2-2.3C7.2 8.5 8.8 4.8 12 2.5z" fill="currentColor"/><path d="M10 16.5l2 5 2-5z" fill="#ff7a2f"/>';
      rk.style.left = (r.left + r.width / 2 - 17) + 'px'; rk.style.top = (r.top - 40) + 'px'; rk.style.color = getComputedStyle(el).color;
      D.body.appendChild(rk);
      toast('Found: liftoff', first, total);
      if (calm()) { setTimeout(() => rk.remove(), 1500); return; }
      rk.animate([{ transform: 'translateY(0) scale(1)', opacity: 1 }, { transform: 'translateY(-14px) scale(1.05)', opacity: 1, offset: 0.2 }, { transform: `translateY(${-r.top - 120}px) scale(.7)`, opacity: 0 }], { duration: 1800, easing: 'cubic-bezier(.5,0,.8,.4)' }).onfinish = () => rk.remove();
    },
    weather(d, first, total) {
      if (!d || !d.el) return;
      toast(d.kind === 'sun' ? 'Found: hot enough for its own sun' : 'Found: cold enough for snow', first, total);
      const host = d.el; host.style.position = host.style.position || 'relative';
      const old = host.querySelector('.egg-fx'); if (old) old.remove();
      const fx = D.createElement('div'); fx.className = 'egg-fx'; fx.setAttribute('aria-hidden', 'true');
      if (d.kind === 'sun') fx.innerHTML = '<b></b>';
      else for (let i = 0; i < 26; i++) { const f = D.createElement('i'); f.style.left = (i * 37 % 100) + '%'; f.style.transform = `scale(${0.5 + (i * 13 % 10) / 12})`; fx.appendChild(f); }
      host.appendChild(fx);
      if (calm()) { setTimeout(() => fx.remove(), 2500); return; }
      if (d.kind === 'sun') fx.firstChild.animate([{ transform: 'scale(.6)', opacity: 0 }, { transform: 'scale(1.1)', opacity: 1 }, { transform: 'scale(1)', opacity: 0 }], { duration: 2600, easing: 'ease-out' }).onfinish = () => fx.remove();
      else { const h = host.getBoundingClientRect().height + 20; [...fx.children].forEach((f, i) => f.animate([{ transform: `translate(0,0) ${f.style.transform}` }, { transform: `translate(${(i % 5) * 6 - 12}px,${h}px) ${f.style.transform}` }], { duration: 1800 + (i * 97 % 900), delay: i * 60, easing: 'linear' })); setTimeout(() => fx.remove(), 4400); }
    }
  };
})();
