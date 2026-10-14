// Adds Pulse and Playground links to the header and a Run button to complete SSPUR examples.
(function () {
  const root = typeof path_to_root === "string" ? path_to_root : "/sspur/";
  const play = root + "play/";

  const right = document.querySelector(".right-buttons");
  if (right && !document.getElementById("sspur-play-link")) {
    const a = document.createElement("a");
    a.id = "sspur-play-link";
    a.href = play;
    a.className = "sspur-play-link";
    a.title = "Open the SSPUR playground";
    a.textContent = "Playground";
    right.prepend(a);
    const pulse = document.createElement("a");
    pulse.href = root + "pulse/";
    pulse.className = "sspur-play-link";
    pulse.title = "SSPUR Pulse: a live dashboard computed by SSPUR programs";
    pulse.textContent = "Pulse";
    right.prepend(pulse);
  }

  if (typeof CompressionStream === "undefined") return;
  const b64url = (bytes) => {
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  };
  const encode = async (text) => {
    const stream = new Blob([new TextEncoder().encode(text)]).stream().pipeThrough(new CompressionStream("deflate-raw"));
    return b64url(new Uint8Array(await new Response(stream).arrayBuffer()));
  };
  const hasMain = /^(pub\s+)?fn\s+main\b/m;
  const blocks = [...document.querySelectorAll("pre > code.language-sspur")];
  if (!blocks.length) return;
  const squash = (t) => t.replace(/\s+/g, " ").trim();
  fetch(play + "snippets.json")
    .then((r) => (r.ok ? r.json() : {}))
    .catch(() => ({}))
    .then((files) => {
      const all = Object.values(files).map((src) => [src, squash(src)]);
      for (const code of blocks) {
        const shown = code.textContent;
        const want = squash(shown);
        // Prefer the smallest snippet file that contains the block, so excerpts run with their context.
        const hit = all.filter(([, flat]) => want && flat.includes(want)).sort((a, b) => a[0].length - b[0].length)[0];
        const src = hit ? hit[0] : shown;
        if (!hit && !hasMain.test(src) && !/^test\s/m.test(src)) continue;
        const pre = code.parentElement;
        const a = document.createElement("a");
        a.className = "sspur-run";
        a.textContent = hasMain.test(src) ? "Run" : "Try it";
        a.title = hit && squash(hit[0]) !== want ? "Open the full example in the playground" : "Open this example in the playground";
        a.target = "_blank";
        a.rel = "noopener";
        a.href = play;
        encode(src).then((c) => { a.href = play + "#code=" + c; });
        pre.classList.add("sspur-has-run");
        pre.appendChild(a);
      }
    });
})();
