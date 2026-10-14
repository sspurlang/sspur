// highlight.js grammar for SSPUR, registered after mdBook's own highlighting runs.
// Blocks marked ```sspur are highlighted, and so are unmarked blocks that start with a
// definition (the docs predate the site and leave SSPUR blocks unmarked).
(function () {
  if (typeof hljs === "undefined") return;

  var KEYWORDS = {
    keyword:
      "fn type test effect handle resume match if then else do for in while return var " +
      "catch raise with where pre post ex pub use store svc ep extern kernel profile " +
      "unsafe yield par own mut static asm migrate new dec and or not",
    literal: "true false none some ok err",
    built_in: "table",
  };

  var EFFECTS = "log fail div conc fs io time env proc net ffi db yield gpu mmio irq";

  hljs.registerLanguage("sspur", function () {
    var braces = { begin: /\{/, end: /\}/, keywords: KEYWORDS };
    var interp = { className: "subst", begin: /\{/, end: /\}/, keywords: KEYWORDS };
    var string = {
      className: "string",
      begin: /"/,
      end: /"/,
      contains: [{ begin: /\\./ }, interp],
    };
    braces.contains = ["self", string, hljs.C_NUMBER_MODE];
    interp.contains = [braces, string, hljs.C_NUMBER_MODE];
    return {
      name: "SSPUR",
      aliases: ["ssp"],
      keywords: KEYWORDS,
      contains: [
        hljs.C_LINE_COMMENT_MODE,
        string,
        {
          // the effect row of a signature: `! log, fail[E], db.read[T]`
          className: "meta",
          begin: /!(?=\s)/,
          end: /$|(?==)/,
          keywords: { built_in: EFFECTS },
          contains: [{ className: "type", begin: /\b[A-Z][A-Za-z0-9_]*/ }],
        },
        {
          className: "title",
          begin: /(?<=\b(?:fn|test|type|effect|store|svc)\s+)[A-Za-z_][A-Za-z0-9_]*/,
        },
        { className: "type", begin: /\b[A-Z][A-Za-z0-9_]*/ },
        hljs.C_NUMBER_MODE,
        { className: "operator", begin: /=>|:=|->|\.\.|==|!=|<=|>=/ },
      ],
    };
  });

  var looksLikeSspur = /^\s*(pub\s+)?(fn|type|test|effect|store|svc|use|extern\s+fn|kernel\s+fn|profile)\s/m;
  var highlight = hljs.highlightElement || hljs.highlightBlock;

  document.querySelectorAll("pre > code").forEach(function (block) {
    var marked = /\blanguage-(sspur|ssp)\b/.test(block.className);
    var unmarked = !/\blanguage-/.test(block.className);
    if (!marked && !(unmarked && looksLikeSspur.test(block.textContent))) return;
    block.textContent = block.textContent;
    block.classList.add("language-sspur");
    highlight.call(hljs, block);
    block.classList.add("hljs");
  });
})();
