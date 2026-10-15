// SSPUR packages: theme, search, copy buttons. Every page works without this script.
(() => {
  const root = document.documentElement;
  const toast = (msg) => {
    const t = document.querySelector(".toast");
    if (!t) return;
    t.textContent = msg;
    t.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => (t.hidden = true), 1800);
  };

  const themeBtn = document.querySelector("[data-theme-toggle]");
  if (themeBtn) {
    themeBtn.addEventListener("click", () => {
      const dark = root.dataset.theme ? root.dataset.theme === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
      root.dataset.theme = dark ? "light" : "dark";
      try {
        localStorage.setItem("sspur-theme", root.dataset.theme);
      } catch (e) {}
    });
  }

  const q = document.getElementById("q");
  const list = document.getElementById("pkgs");
  if (q && list) {
    const items = [...list.children];
    const none = document.getElementById("none");
    const apply = () => {
      const words = q.value.toLowerCase().split(/\s+/).filter(Boolean);
      let shown = 0;
      for (const li of items) {
        const text = li.dataset.text || "";
        const ok = words.every((w) => text.includes(w));
        li.hidden = !ok;
        if (ok) shown++;
      }
      if (none) none.hidden = shown > 0;
      const url = new URL(location.href);
      if (q.value) url.searchParams.set("q", q.value);
      else url.searchParams.delete("q");
      history.replaceState(null, "", url);
    };
    q.value = new URLSearchParams(location.search).get("q") || "";
    q.addEventListener("input", apply);
    if (q.value) apply();
    addEventListener("keydown", (e) => {
      if (e.key === "/" && document.activeElement !== q && !/input|textarea|select/i.test(document.activeElement.tagName)) {
        e.preventDefault();
        q.focus();
      }
    });
  }

  for (const b of document.querySelectorAll("[data-copy]")) {
    b.addEventListener("click", async () => {
      const el = document.getElementById(b.dataset.copy);
      try {
        await navigator.clipboard.writeText(el.textContent);
        toast("Copied");
      } catch (e) {
        getSelection().selectAllChildren(el);
        toast("Selected: press Ctrl+C or Cmd+C");
      }
    });
  }

  // Type "sspur" anywhere outside a text field.
  let typed = "";
  addEventListener("keydown", (e) => {
    if (/input|textarea|select/i.test(document.activeElement.tagName) || e.key.length !== 1) return;
    typed = (typed + e.key.toLowerCase()).slice(-5);
    if (typed === "sspur") {
      const logo = document.querySelector(".logo");
      logo.classList.remove("spin");
      void logo.offsetWidth;
      logo.classList.add("spin");
      toast("Effects in the signature, hashes in the lock.");
    }
  });
})();
