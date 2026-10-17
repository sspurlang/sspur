// The 404 page as a compiler error: the visitor's path, a caret under it, and a quick fix that
// points at the closest real page (by edit distance over known pages and registry packages).
(function () {
  var path = decodeURIComponent(location.pathname || "/");
  if (path.length > 60) path = path.slice(0, 57) + "...";
  var known = ["/sspur-packages/", "/sspur-packages/publish/"];
  function dist(a, b) {
    var m = a.length, n = b.length, d = [], i, j;
    for (i = 0; i <= m; i++) d[i] = [i];
    for (j = 0; j <= n; j++) d[0][j] = j;
    for (i = 1; i <= m; i++) for (j = 1; j <= n; j++)
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    return d[m][n];
  }
  function norm(s) { return s.toLowerCase().replace(/\.html$/, "").replace(/\/index$/, "").replace(/\/+$/, ""); }
  var alias = { docs: "/sspur/", doc: "/sspur/", documentation: "/sspur/", registry: "/sspur-packages/", packages: "/sspur-packages/",
    install: "/sspur/get-started/installation.html", publish: "/sspur-packages/publish/", playground: "/sspur/play/", github: "https://github.com/sspurlang" };
  function best(list) {
    var p = norm(path), last = p.split("/").pop() || "", top = "/sspur-packages/", score = 1e9;
    if (alias[p.replace(/^\//, "")]) return alias[p.replace(/^\//, "")];
    list.forEach(function (k) {
      var n = norm(k), tail = n.split("/").pop() || "";
      var s = dist(p, n);
      // A typed beginning of a page's name, or a near miss of it, is a strong hint.
      if (last.length >= 3 && tail.indexOf(last) === 0) s = Math.min(s, 1);
      else if (last.length >= 3 && tail) s = Math.min(s, dist(last, tail) + 1);
      if (s < score) { score = s; top = k; }
    });
    return p && score <= Math.max(2, Math.floor(last.length * 0.4)) ? top : "/sspur-packages/";
  }
  function show(fix) {
    document.querySelectorAll(".lost-p").forEach(function (e) { e.textContent = path; });
    document.getElementById("lost-path").textContent = path;
    document.getElementById("lost-caret").textContent = new Array(Math.max(1, Math.min(path.length, 40)) + 1).join("^");
    document.getElementById("lost-help").textContent = fix === "/sspur-packages/" ? "   = help: search the packages: /sspur-packages/" : "   = help: did you mean " + fix + " ?";
    var a = document.getElementById("lost-fix");
    a.href = fix;
    a.textContent = fix === "/sspur-packages/" ? "Browse all packages" : "Apply the fix";
    var home = document.getElementById("lost-home");
    if (home) home.style.display = fix === "/sspur-packages/" ? "none" : "";
  }
  show(best(known));
  // Package pages too: the registry's own list of packages, same origin.
  fetch("/sspur-packages/search.json").then(function (r) { return r.ok ? r.json() : []; }).then(function (cards) {
    var pkgs = (cards || []).map(function (c) { return "/sspur-packages/packages/" + c.name + "/"; });
    show(best(known.concat(pkgs)));
  }).catch(function () {});
  // The error appears line by line, as a terminal prints it.
  var term = document.getElementById("lost-term");
  if (!matchMedia("(prefers-reduced-motion: reduce)").matches && term) {
    term.classList.add("typing");
    var lines = term.innerHTML.split("\n");
    term.innerHTML = lines.map(function (l, i) { return '<span class="line" style="animation-delay:' + (i * 110) + 'ms">' + l + "</span>"; }).join("\n");
  }
})();
