// The playground editor: CodeMirror 6 with an SSPUR mode. tools/build_playground.sh bundles this
// file with its imports into editor.bundle.js.
import { EditorView, ViewPlugin, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, dropCursor, highlightSpecialChars, hoverTooltip, showTooltip, Decoration, WidgetType } from "@codemirror/view";
import { EditorState, Compartment, StateField, StateEffect, Transaction } from "@codemirror/state";
import { StreamLanguage, syntaxHighlighting, HighlightStyle, bracketMatching, indentService, indentUnit, indentOnInput, foldGutter, foldService, foldKeymap } from "@codemirror/language";
import { defaultKeymap, history, historyKeymap, indentWithTab, toggleComment, isolateHistory } from "@codemirror/commands";
import { autocompletion, completionKeymap, closeBrackets, closeBracketsKeymap, snippetCompletion, startCompletion } from "@codemirror/autocomplete";
import { lintGutter, setDiagnostics, lintKeymap } from "@codemirror/lint";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { tags as t, Tag } from "@lezer/highlight";

const KEYWORDS = new Set("fn type test var for in if then else match catch handle do raise return with profile derive new par while rule trait impl store svc queue effect use pub extern kernel static unsafe is resume yield own mut migrate dec cost".split(" "));
const CONTRACTS = new Set(["pre", "post", "where", "ex"]);
const OPERATORS = new Set(["and", "or", "not"]);
const DEF_AFTER = new Set(["fn", "test", "type", "effect", "trait", "rule", "store", "svc"]);
const BUILTIN_TYPES = new Set("Int F64 F32 Bool Str Unit List Opt Res Map Set Heap StrBuf Time Duration Bits HashMap HashSet BigInt Dec Regex Rng Complex Ratio Locale Zone File View FlatMap MdSpan DevBuf Atomic Chan Secret Pii Untrusted Guess Ptr Array Mmio Self I8 I16 I32 U8 U16 U32 U64".split(" "));
const EFFECTS = new Set("log fail div conc fs io time env proc ffi db yield dev unsafe mmio static".split(" "));

const effectTag = Tag.define();
const contractTag = Tag.define();
const holeTag = Tag.define();

function last(stack) {
  return stack[stack.length - 1];
}

// Strings nest code inside `{...}`, so the state is a stack of string and interpolation frames.
const sspur = StreamLanguage.define({
  name: "sspur",
  startState: () => ({ stack: [], effects: false, prev: "", prevKind: "" }),
  copyState: (s) => ({ stack: s.stack.map((f) => ({ ...f })), effects: s.effects, prev: s.prev, prevKind: s.prevKind }),
  token(stream, state) {
    const top = last(state.stack);
    if (stream.sol() && !top) state.effects = false;
    if (top && top.t === "str") {
      if (stream.match(/^\\./)) return "escape";
      if (stream.eat('"')) {
        state.stack.pop();
        return "string";
      }
      if (stream.eat("{")) {
        state.stack.push({ t: "interp", depth: 0 });
        return "interp";
      }
      stream.match(/^[^"\\{]+/);
      return "string";
    }
    if (stream.eatSpace()) return null;
    const kind = tokenIn(stream, state, top);
    state.prevKind = kind || "";
    return kind;
  },
  indent: () => null,
  languageData: {
    commentTokens: { line: "//" },
    closeBrackets: { brackets: ["(", "[", "{", '"'] },
    indentOnInput: /^\s*[|=.)\]}]$/,
  },
  tokenTable: { effect: effectTag, contract: contractTag, hole: holeTag, interp: t.special(t.string), def: t.definition(t.variableName), ctor: t.className, call: t.function(t.variableName), method: t.function(t.propertyName), placeholder: t.special(t.variableName), pipe: t.separator, bang: effectTag },
});

function tokenIn(stream, state, top) {
  if (stream.match("//")) {
    stream.skipToEnd();
    return "comment";
  }
  if (top && top.t === "interp") {
    if (stream.peek() === "}" && top.depth === 0) {
      stream.next();
      state.stack.pop();
      return "interp";
    }
  }
  if (stream.eat('"')) {
    state.stack.push({ t: "str" });
    return "string";
  }
  if (stream.match(/^(0x[0-9a-fA-F_]+|0b[01_]+|0o[0-7_]+|\d[\d_]*(\.\d[\d_]*)?([eE][+-]?\d+)?)/)) {
    state.prev = "1";
    return "number";
  }
  if (stream.match(/^![^=]/, false) || (stream.peek() === "!" && stream.string.length === stream.pos + 1)) {
    stream.next();
    state.effects = true;
    state.prev = "!";
    return "bang";
  }
  const w = stream.match(/^[A-Za-z_][A-Za-z0-9_]*/);
  if (w) {
    const word = w[0];
    const prev = state.prev;
    state.prev = word;
    const next = stream.string.charAt(stream.pos);
    if (word === "_") return "placeholder";
    if (CONTRACTS.has(word)) {
      state.effects = false;
      return "contract";
    }
    if (state.effects) {
      if (/^[A-Z]/.test(word)) return "typeName";
      if (EFFECTS.has(word) || prev === "!" || prev === "," || prev === ".") return "effect";
    }
    if (word === "true" || word === "false") return "bool";
    if (word === "none") return "atom";
    if (OPERATORS.has(word)) return "operatorKeyword";
    if (KEYWORDS.has(word)) return "keyword";
    if (DEF_AFTER.has(prev)) return "def";
    if (prev === ".") return next === "(" ? "method" : "propertyName";
    if (/^[A-Z]/.test(word)) {
      if (next === "{" || next === "(") return BUILTIN_TYPES.has(word) && next === "(" ? "typeName" : "ctor";
      if (prev === ":" || prev === "->" || prev === "[" || prev === "new" || prev === "for" || BUILTIN_TYPES.has(word)) return "typeName";
      return "ctor";
    }
    if (word === "some" || word === "ok" || word === "err") return "ctor";
    if (next === "(") return "call";
    return "variableName";
  }
  const op = stream.match(/^(=>|->|:=|==|!=|<=|>=|\.\.\.|\.\.|\*\*|[+\-*/%<>=&@;])/);
  if (op) {
    state.prev = op[0];
    if (op[0] === "=") state.effects = false;
    return "operator";
  }
  const ch = stream.next();
  state.prev = ch;
  if (ch === "|") return "pipe";
  if (ch === "?") {
    stream.match(/^[A-Za-z_][A-Za-z0-9_]*/);
    return "hole";
  }
  if (ch === "." || ch === "," || ch === ":") return "punctuation";
  if (top && top.t === "interp") {
    if (ch === "{") top.depth++;
    if (ch === "}") top.depth--;
  }
  if ("()[]{}".includes(ch)) return "bracket";
  return null;
}

// indent rules: begin (tools/playground_test.mjs runs these; crates/sspur-ide/src/indent.rs is
// the same for the language server)
// A new line goes one level deeper after a line that opens a block: `= do`, `then do`, a
// `match` scrutinee, an arm ending in `=>`, a trait or impl header, a bare signature.
function indentFor(text) {
  const line = text.replace(/\/\/.*$/, "").trimEnd();
  const base = text.length - text.trimStart().length;
  const head = line.trim();
  if (head === "") return null;
  if (/(\bdo|\bthen|\belse|=>|=)$/.test(line) || /(^|[=>(]\s*)\b(match|catch|handle)\b/.test(head) && !/\|/.test(head)) return base + 2;
  if (/^(pub\s+)?(trait|impl|svc)\b/.test(head) || /^(pub\s+)?effect\s+\w+(\[[^\]]*\])?$/.test(head)) return base + 2;
  if (/^(pub\s+)?(fn|rule|kernel\s+fn)\b/.test(head) && base === 0) return /^(pub\s+)?rule\b/.test(head) ? 2 : 0;
  return base;
}

// A line ending in a binary operator continues on the next line, one level deeper; so does a
// line starting with `.name` or an operator, which keeps its own indent for the next one.
const codeOf = (s) => s.replace(/\/\/.*$/, "").trimEnd();
const indentOf = (s) => s.length - s.trimStart().length;
const endsOp = (s) => !/=>$/.test(codeOf(s)) && /(\*\*|==|!=|<=|>=|:=|&&|\|\||->|[-+*/%<>,]|(^|[^\w])(and|or|is|with))$/.test(codeOf(s));
const startsOp = (s) => /^(\.[A-Za-z_]|(and|or|is)(?!\w)|[-+*/%<>]|==|!=|&&)/.test(codeOf(s).trimStart());

// An element of a list broken one per line: it ends in a comma and opens no bracket.
function elementLine(s) {
  const code = codeOf(s).replace(/"(\\.|[^"\\])*"/g, '""');
  let depth = 0;
  for (const c of code) depth += "([{".includes(c) ? 1 : ")]}".includes(c) ? -1 : 0;
  return code.endsWith(",") && depth <= 0;
}

// The indent of a new line typed after lines[i], skipping blank lines above.
function enterIndent(lines, i) {
  const nonblank = (j) => { while (j >= 0 && codeOf(lines[j]).trim() === "") j--; return j; };
  i = nonblank(i);
  if (i < 0) return 0;
  const line = lines[i], base = indentOf(line), prev = nonblank(i - 1);
  if (/[([{]$/.test(codeOf(line))) return base + 2;
  if (elementLine(line)) return base;
  if (endsOp(line)) return startsOp(line) || (prev >= 0 && endsOp(lines[prev])) ? base : base + 2;
  const o = indentFor(line);
  if (o !== base || startsOp(line)) return o;
  if (prev < 0 || !endsOp(lines[prev])) return base;
  let j = prev;
  for (let k = nonblank(j - 1); k >= 0 && endsOp(lines[k]); k = nonblank(k - 1)) j = k;
  return indentOf(lines[j]);
}
// A line that starts with a closing bracket goes back to the indent of the line that opened it.
function closerIndent(lines) {
  let depth = 1;
  for (let i = lines.length - 1; i >= 0; i--) {
    const code = codeOf(lines[i]).replace(/"(\\.|[^"\\])*"/g, '""');
    for (let k = code.length - 1; k >= 0; k--) {
      if (")]}".includes(code[k])) depth++;
      else if ("([{".includes(code[k]) && --depth === 0) return indentOf(lines[i]);
    }
  }
  return null;
}
// indent rules: end

// The line being indented is the one at pos, or the part after a simulated break when Enter is
// pressed; the rules look at up to 40 lines above it.
const sspurIndent = indentService.of((cx, pos) => {
  const cur = cx.lineAt(pos, 1);
  const after = cur.text.slice(pos - cur.from).trim();
  const lines = [];
  let l = cx.lineAt(pos, -1);
  if (l.from === cur.from) l = l.from > 0 ? cx.lineAt(l.from - 1, -1) : null;
  for (let n = 0; l && n < 40; n++) {
    lines.unshift(l.text);
    l = l.from > 0 ? cx.lineAt(l.from - 1, -1) : null;
  }
  const last = lines.findLastIndex((t) => t.trim() !== "");
  if (last < 0) return 0;
  if (after.startsWith("=") && /^(pub\s+)?(fn|test)\b/.test(lines[last])) return 0;
  if (/^\.[A-Za-z_]/.test(after)) return indentOf(lines[last]) + (/^\s*\./.test(lines[last]) ? 0 : 2);
  if (/^[)\]}]/.test(after)) {
    const c = closerIndent(lines);
    if (c !== null) return c;
  }
  return enterIndent(lines, last);
});

const foldByIndent = foldService.of((state, from, to) => {
  const line = state.doc.lineAt(from);
  const ind = (l) => l.text.length - l.text.trimStart().length;
  if (line.text.trim() === "") return null;
  let end = line, n = line.number;
  while (n < state.doc.lines) {
    const next = state.doc.line(n + 1);
    if (next.text.trim() !== "" && ind(next) <= ind(line) && !/^[=|]/.test(next.text.trim())) break;
    n++;
    if (next.text.trim() !== "") end = next;
  }
  return end.number > line.number ? { from: line.to, to: end.to } : null;
});

// format rules: begin (tools/playground_test.mjs runs these)
// Reformatting keeps the cursor after the same number of code characters: spaces, line breaks
// and comments don't count, since the formatter moves the first two and drops the third.
function codeCount(text, end) {
  let n = 0, str = false;
  for (let i = 0; i < end && i < text.length; i++) {
    const c = text[i];
    if (str) {
      n++;
      if (c === "\\") { i++; n++; } else if (c === '"') str = false;
    } else if (c === "/" && text[i + 1] === "/") {
      while (i + 1 < end && text[i + 1] !== "\n") i++;
    } else if (!/\s/.test(c)) {
      n++;
      if (c === '"') str = true;
    }
  }
  return n;
}
function posOfCount(text, n) {
  if (n <= 0) return 0;
  let lo = 0, hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (codeCount(text, mid) < n) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}
// Where `pos` in `before` is in `after`: after the same code, and before the same code when it
// was right before some.
function mapPos(before, pos, after) {
  let p = posOfCount(after, codeCount(before, pos));
  if (pos < before.length && !/\s/.test(before[pos])) while (p < after.length && /\s/.test(after[p])) p++;
  return p;
}
// The top-level definitions that the range from..to touches, whole: from the line that starts
// the first to the last line of code of the last.
const DEF_START = /^(pub\s+)?(fn|type|res\s+type|test|effect|trait|impl|store|svc|static|use|profile|rule|extern|kernel)\b/;
function defRange(text, from, to) {
  const starts = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") starts.push(i + 1);
  const lineOf = (pos) => { let lo = 0, hi = starts.length - 1; while (lo < hi) { const m = (lo + hi + 1) >> 1; if (starts[m] <= pos) lo = m; else hi = m - 1; } return lo; };
  const line = (i) => text.slice(starts[i], i + 1 < starts.length ? starts[i + 1] - 1 : text.length);
  let a = lineOf(from), b = lineOf(Math.max(from, to - (to > from && text[to - 1] === "\n" ? 1 : 0)));
  while (a > 0 && !DEF_START.test(line(a))) a--;
  let e = b + 1;
  while (e < starts.length && !DEF_START.test(line(e))) e++;
  e--;
  while (e > a && line(e).trim() === "") e--;
  return { from: starts[a], to: e + 1 < starts.length ? starts[e + 1] - 1 : text.length };
}
// The smallest edit that turns a into b: what is between their common prefix and suffix.
function diffRange(a, b) {
  let p = 0;
  while (p < a.length && p < b.length && a[p] === b[p]) p++;
  let q = 0;
  while (q < a.length - p && q < b.length - p && a[a.length - 1 - q] === b[b.length - 1 - q]) q++;
  return { from: p, to: a.length - q, insert: b.slice(p, b.length - q) };
}
// format rules: end

// A thin line at the formatter's width, unless the editor wraps lines (on a phone), where a
// column would mean nothing.
function ruler(col) {
  return ViewPlugin.fromClass(class {
    constructor(view) {
      this.dom = document.createElement("div");
      this.dom.className = "cm-sspur-ruler";
      this.dom.setAttribute("aria-hidden", "true");
      view.scrollDOM.appendChild(this.dom);
      this.place(view);
    }
    update(u) {
      // Fonts can load after the first measure, changing the character width.
      if (u.geometryChanged || u.heightChanged || u.docChanged || u.viewportChanged) this.place(u.view);
    }
    place(view) {
      view.requestMeasure({
        read: () => {
          const line = view.contentDOM.querySelector(".cm-line");
          const pad = line ? parseFloat(getComputedStyle(line).paddingLeft) || 0 : 6;
          const left = view.contentDOM.offsetLeft + pad + col * view.defaultCharacterWidth;
          return { left, show: !view.lineWrapping, height: Math.max(view.contentHeight, view.scrollDOM.clientHeight) };
        },
        write: (m) => {
          this.dom.style.left = m.left + "px";
          this.dom.style.height = m.height + "px";
          this.dom.hidden = !m.show;
        },
      });
    }
    destroy() {
      this.dom.remove();
    }
  });
}

// A message at a position for a moment: why Reformat left the code as it was.
const setFlash = StateEffect.define();
const flashField = StateField.define({
  create: () => null,
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setFlash)) return e.value;
    return v && tr.docChanged ? null : v;
  },
  provide: (f) =>
    showTooltip.from(f, (v) =>
      v
        ? {
            pos: v.pos,
            above: true,
            create() {
              const dom = document.createElement("div");
              dom.className = "cm-sspur-flash";
              dom.setAttribute("role", "status");
              dom.textContent = v.text;
              return { dom };
            },
          }
        : null,
    ),
});

const palette = {
  light: { keyword: "#a626a4", def: "#1c5fb8", type: "#0b7285", ctor: "#b4590b", effect: "#8a3ffc", string: "#2b8a3e", number: "#c2410c", comment: "#8b949e", op: "#57606a", interp: "#cf222e", contract: "#9a6700", prop: "#24292f", call: "#1c5fb8", hole: "#cf222e" },
  dark: { keyword: "#f0a8f5", def: "#8ab4f8", type: "#66d9e8", ctor: "#ffb86b", effect: "#c4a2ff", string: "#a5d6a7", number: "#f6a878", comment: "#7d8590", op: "#a0a8b0", interp: "#ff7b72", contract: "#e3b341", prop: "#d6dde4", call: "#8ab4f8", hole: "#ff7b72" },
};

function highlight(p) {
  return HighlightStyle.define([
    { tag: t.keyword, color: p.keyword },
    { tag: t.operatorKeyword, color: p.keyword },
    { tag: t.definition(t.variableName), color: p.def, fontWeight: "600" },
    { tag: t.typeName, color: p.type },
    { tag: t.className, color: p.ctor },
    { tag: effectTag, color: p.effect, fontStyle: "italic" },
    { tag: contractTag, color: p.contract, fontWeight: "600" },
    { tag: t.string, color: p.string },
    { tag: t.escape, color: p.interp },
    { tag: t.special(t.string), color: p.interp, fontWeight: "600" },
    { tag: [t.number, t.bool, t.atom], color: p.number },
    { tag: t.comment, color: p.comment, fontStyle: "italic" },
    { tag: [t.operator, t.separator, t.punctuation], color: p.op },
    { tag: t.special(t.variableName), color: p.keyword, fontWeight: "600" },
    { tag: t.function(t.variableName), color: p.call },
    { tag: t.function(t.propertyName), color: p.call },
    { tag: t.propertyName, color: p.prop },
    { tag: holeTag, color: p.hole, fontWeight: "700" },
  ]);
}

function theme(dark) {
  return EditorView.theme(
    {
      "&": { height: "100%", fontSize: "var(--editor-font, 14px)", backgroundColor: "var(--editor-bg)", color: "var(--fg)" },
      ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.55" },
      ".cm-content": { caretColor: "var(--accent)" },
      ".cm-gutters": { backgroundColor: "var(--editor-bg)", color: "var(--muted)", border: "none" },
      ".cm-activeLine": { backgroundColor: "var(--active-line)" },
      ".cm-activeLineGutter": { backgroundColor: "var(--active-line)" },
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--selection) !important" },
      ".cm-tooltip": { backgroundColor: "var(--panel-bg)", border: "1px solid var(--border)", borderRadius: "6px", color: "var(--fg)" },
      ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--accent)", color: "#fff" },
      ".cm-completionDetail": { fontStyle: "normal", opacity: 0.75, marginLeft: "0.8em" },
      ".cm-completionInfo": { maxWidth: "28em", fontFamily: "var(--sans)", fontSize: "13px", padding: "6px 9px" },
      ".cm-sspur-hover": { fontFamily: "var(--mono)", fontSize: "13px", padding: "5px 9px", maxWidth: "40em", whiteSpace: "pre-wrap" },
      ".cm-sspur-hover .doc": { fontFamily: "var(--sans)", marginTop: "4px", opacity: 0.85 },
      ".cm-sspur-sig": { fontFamily: "var(--mono)", fontSize: "13px", padding: "4px 9px", maxWidth: "44em", whiteSpace: "pre-wrap" },
      ".cm-sspur-sig .active": { fontWeight: "bold", textDecoration: "underline", textUnderlineOffset: "3px" },
      ".cm-sspur-sig .doc": { fontFamily: "var(--sans)", marginTop: "3px", opacity: 0.85 },
      ".cm-sspur-hint": { color: "var(--muted)", fontSize: "0.9em", opacity: 0.85 },
      ".cm-diagnostic": { fontFamily: "var(--sans)", fontSize: "13px" },
      ".cm-sspur-ruler": { position: "absolute", top: 0, width: "1px", background: "var(--ruler)", pointerEvents: "none", zIndex: -1 },
      ".cm-sspur-flash": { fontFamily: "var(--sans)", fontSize: "13px", padding: "5px 10px", color: "var(--err)", maxWidth: "36em" },
      ".cm-diagnosticAction": { backgroundColor: "var(--accent)", borderRadius: "4px" },
      ".cm-matchingBracket": { backgroundColor: "var(--match)", outline: "none" },
      ".cm-foldGutter .cm-gutterElement": { opacity: 0, transition: "opacity 0.2s" },
      ".cm-gutters:hover .cm-foldGutter .cm-gutterElement": { opacity: 1 },
    },
    { dark },
  );
}

// Templates for the common shapes, offered next to the compiler's names.
const SNIPPETS = [
  ["fn", "fn ${name}(${x}: ${Int}) -> ${Int}\n= ${x}", "function"],
  ["fn do", "fn ${name}(${x}: ${Int}) -> ${Unit} ! log\n= do\n  ${}", "function with a block body"],
  ["main", "fn main() -> Unit ! log\n= do\n  log(\"${hello}\")", "entry point"],
  ["type record", "type ${Name} = {${field}: ${Int}}", "record type"],
  ["type sum", "type ${Name} = ${A} | ${B}{${x}: ${Int}}", "sum type"],
  ["match", "match ${e}\n  | ${pat} => ${}\n  | _ => ${}", "match arms"],
  ["if is", "if ${e} is ${some(v)} then ${v} else ${d}", "one-pattern test"],
  ["is", "${e} is ${some(_)}", "pattern test, a Bool"],
  ["guard", "${some(v)} = ${e} else ${d}", "bind a pattern, or end the block with d"],
  ["if then else", "if ${c} then ${a} else ${b}", "conditional"],
  ["test", "test ${name} = ${f(1) == 1}", "test"],
  ["for", "for ${x} in ${xs}\n  ${}", "loop"],
  ["do", "do\n  ${}", "block"],
  ["catch", "catch ${e}\n  | ${Err} => ${}", "handle failures"],
  ["handle", "handle ${e}\n  | ${op}() => resume(${})", "effect handler"],
  ["effect", "effect ${ask}() -> ${Int}", "effect declaration"],
  ["trait", "trait ${Name}\n  fn ${m}(x: Self) -> ${Str}", "trait"],
  ["impl", "impl ${Trait} for ${Type}\n  fn ${m}(x: ${Type}) -> ${Str} = ${}", "trait implementation"],
];

const kindBoost = { variable: 2, function: 1 };

function completionSource(api) {
  const snippets = SNIPPETS.map(([label, tpl, detail]) => snippetCompletion(tpl, { label, detail, type: "keyword", boost: -1 }));
  return async (ctx) => {
    const word = ctx.matchBefore(/[A-Za-z_][A-Za-z0-9_]*/);
    const dot = ctx.matchBefore(/\.[A-Za-z0-9_]*/);
    if (!ctx.explicit && !word && !dot) return null;
    const res = await api.complete(ctx.state.doc.toString(), ctx.pos);
    if (!res || ctx.aborted) return null;
    const options = res.items.map((it) => {
      const o = { label: it.label, type: it.type, detail: it.detail || undefined, boost: (it.boost || 0) + (kindBoost[it.type] || 0) };
      if (it.info) o.info = it.info;
      return it.apply ? snippetCompletion(it.apply, o) : o;
    });
    const line = ctx.state.doc.lineAt(ctx.pos);
    const before = line.text.slice(0, ctx.pos - line.from);
    const isMember = /\.[A-Za-z0-9_]*$/.test(before);
    if (!isMember && !/!\s*[\w, \[\].]*$/.test(before)) {
      options.push(...snippets);
      for (const k of api.keywords()) options.push({ label: k, type: "keyword", boost: -2 });
    }
    return { from: res.from, options, validFor: /^[A-Za-z0-9_]*$/ };
  };
}

function hoverSource(api) {
  return hoverTooltip(async (view, pos) => {
    const h = await api.hover(view.state.doc.toString(), pos);
    if (!h) return null;
    return {
      pos: h.from,
      end: h.to,
      above: true,
      create() {
        const dom = document.createElement("div");
        dom.className = "cm-sspur-hover";
        dom.textContent = h.text;
        if (h.doc) {
          const d = document.createElement("div");
          d.className = "doc";
          d.textContent = h.doc;
          dom.appendChild(d);
        }
        return { dom };
      },
    };
  }, { hoverTime: 350 });
}

// Signature help: typing `(` or `,` asks for the signature of the call at the cursor; the tooltip
// follows the cursor while it is open and closes outside a call, on `)` after the last argument,
// or on Escape.
const setSignature = StateEffect.define();
const signatureField = StateField.define({
  create: () => null,
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setSignature)) return e.value;
    if (v && tr.docChanged) return { ...v, pos: tr.changes.mapPos(v.pos) };
    return v;
  },
  provide: (f) =>
    showTooltip.from(f, (v) =>
      v
        ? {
            pos: v.pos,
            above: true,
            create() {
              const dom = document.createElement("div");
              dom.className = "cm-sspur-sig";
              const label = v.sig.label;
              const r = v.sig.ranges[v.sig.active];
              if (r) {
                dom.append(label.slice(0, r[0]));
                const a = document.createElement("span");
                a.className = "active";
                a.textContent = label.slice(r[0], r[1]);
                dom.append(a, label.slice(r[1]));
              } else dom.textContent = label;
              if (v.sig.doc) {
                const d = document.createElement("div");
                d.className = "doc";
                d.textContent = v.sig.doc;
                dom.appendChild(d);
              }
              return { dom };
            },
          }
        : null,
    ),
});

function signatureHelp(api) {
  let asked = 0;
  const ask = async (view) => {
    const mine = ++asked;
    const text = view.state.doc.toString();
    const head = view.state.selection.main.head;
    const sig = api.signature ? await api.signature(text, head) : null;
    if (mine !== asked) return;
    // Typing went on while we waited (a fast typist, or a paste): ask again for where the cursor is now.
    if (view.state.doc.toString() !== text || view.state.selection.main.head !== head) return ask(view);
    view.dispatch({ effects: setSignature.of(sig ? { pos: head, sig } : null) });
  };
  return [
    signatureField,
    EditorView.updateListener.of((u) => {
      const open = u.state.field(signatureField) !== null;
      let typed = false;
      for (const tr of u.transactions) {
        if (!tr.isUserEvent("input.type")) continue;
        tr.changes.iterChanges((_a, _b, _c, _d, ins) => {
          if (/[(,)]$/.test(ins.toString())) typed = true;
        });
      }
      if (typed || (open && (u.docChanged || u.selectionSet))) ask(u.view);
    }),
    keymap.of([{ key: "Escape", run: (v) => (v.state.field(signatureField) ? (v.dispatch({ effects: setSignature.of(null) }), true) : false) }]),
  ];
}

// Inlay hints, when the page turns them on: the types of bindings and the names of literal
// arguments, as widgets from the last check.
class HintWidget extends WidgetType {
  constructor(label) {
    super();
    this.label = label;
  }
  eq(o) {
    return o.label === this.label;
  }
  toDOM() {
    const s = document.createElement("span");
    s.className = "cm-sspur-hint";
    s.textContent = this.label;
    return s;
  }
}
const setHints = StateEffect.define();
const hintsField = StateField.define({
  create: () => Decoration.none,
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setHints)) return e.value;
    return tr.docChanged ? v.map(tr.changes) : v;
  },
  provide: (f) => EditorView.decorations.from(f),
});

const SEVERITY = { error: "error", warning: "warning", hole: "info" };

export function createEditor({ parent, doc, dark, api, onChange, commands, width = 100 }) {
  const themeConf = new Compartment();
  const styled = (d) => [theme(d), syntaxHighlighting(highlight(d ? palette.dark : palette.light))];
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        ...(matchMedia("(max-width: 720px)").matches ? [EditorView.lineWrapping] : []),
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        history(),
        foldGutter(),
        drawSelection(),
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        indentUnit.of("  "),
        EditorState.tabSize.of(2),
        sspur,
        sspurIndent,
        indentOnInput(),
        foldByIndent,
        bracketMatching(),
        closeBrackets(),
        autocompletion({ override: [completionSource(api)], activateOnTypingDelay: 120, icons: true }),
        highlightActiveLine(),
        highlightSelectionMatches(),
        hoverSource(api),
        signatureHelp(api),
        hintsField,
        flashField,
        ruler(width),
        lintGutter(),
        themeConf.of(styled(dark)),
        keymap.of([
          { key: "Mod-Enter", run: () => (commands.run(), true), preventDefault: true },
          { key: "Mod-Shift-Enter", run: () => (commands.test(), true), preventDefault: true },
          { key: "Shift-Alt-f", run: () => (commands.format(), true), preventDefault: true },
          { key: "Mod-Alt-l", run: () => (commands.format(), true), preventDefault: true },
          { key: "Mod-s", run: () => (commands.share(), true), preventDefault: true },
          { key: "Mod-/", run: toggleComment },
          { key: "Ctrl-Space", run: startCompletion },
          ...closeBracketsKeymap,
          ...completionKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...foldKeymap,
          ...lintKeymap,
          ...defaultKeymap,
          indentWithTab,
        ]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) onChange(u.state.doc.toString());
        }),
        // On a Mac, Option + Shift + F types a character, so the keymap never sees it.
        EditorView.domEventHandlers({
          keydown(e) {
            if (!(e.altKey && e.shiftKey && e.code === "KeyF" && !e.ctrlKey && !e.metaKey)) return false;
            e.preventDefault();
            commands.format();
            return true;
          },
        }),
      ],
    }),
  });
  return {
    view,
    text: () => view.state.doc.toString(),
    setText(text, keepCursor) {
      const head = Math.min(view.state.selection.main.head, text.length);
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text }, selection: keepCursor ? { anchor: head } : { anchor: 0 } });
    },
    setDark(d) {
      view.dispatch({ effects: themeConf.reconfigure(styled(d)) });
    },
    // Reformat Code: the whole text, or the top-level definitions the selection touches, through
    // `fmt` (text -> {ok, text} or {ok: false, diags}). One undo step brings the old text back;
    // the cursor stays at the same code and on the same line of the screen. Code that doesn't
    // parse is left alone, with the reason shown where it is.
    async reformat(fmt) {
      const st = view.state, text = st.doc.toString(), sel = st.selection.main;
      const r = sel.empty ? { from: 0, to: text.length } : defRange(text, sel.from, sel.to);
      const part = text.slice(r.from, r.to);
      const res = await fmt(part);
      if (view.state.doc.toString() !== text) return false;
      if (!res.ok) {
        const d = (res.diags || [])[0];
        const pos = Math.min(r.from + (d ? d.from : 0), text.length);
        view.dispatch({ effects: setFlash.of({ pos, text: "Can't reformat: " + (d ? `${d.line}:${d.col} ${d.message}` : "the code doesn't parse") }) });
        const shown = view.state.field(flashField);
        setTimeout(() => { if (view.state.field(flashField) === shown) view.dispatch({ effects: setFlash.of(null) }); }, 2600);
        return false;
      }
      const out = r.to < text.length || !part.endsWith("\n") ? res.text.replace(/\n$/, "") : res.text;
      if (out === part) return true;
      const d = diffRange(part, out);
      const changes = st.changes({ from: r.from + d.from, to: r.from + d.to, insert: d.insert });
      const map = (pos) => (pos < r.from || pos > r.to ? changes.mapPos(pos) : r.from + mapPos(part, pos - r.from, out));
      const top = view.coordsAtPos(sel.head);
      const y = top ? Math.max(0, top.top - view.scrollDOM.getBoundingClientRect().top) : 0;
      const head = map(sel.head);
      view.dispatch({
        changes,
        selection: { anchor: map(sel.anchor), head },
        effects: EditorView.scrollIntoView(head, { y: "start", yMargin: y }),
        annotations: [Transaction.userEvent.of("format"), isolateHistory.of("full")],
      });
      return true;
    },
    goto(pos) {
      view.dispatch({ selection: { anchor: pos }, scrollIntoView: true });
      view.focus();
    },
    // Inlay hints from a check of `text`, as `[{at, label, kind}]`; an empty list clears them.
    setHints(text, hints) {
      if (view.state.doc.toString() !== text) return;
      const len = view.state.doc.length;
      const marks = hints
        .filter((h) => h.at <= len)
        .map((h) => Decoration.widget({ widget: new HintWidget(h.kind === "param" ? h.label + " " : h.label), side: h.kind === "param" ? -1 : 1 }).range(h.at));
      view.dispatch({ effects: setHints.of(Decoration.set(marks, true)) });
    },
    // Diagnostics from a check of `text`; a fix is applied only while the text is unchanged.
    setDiagnostics(text, diags) {
      if (view.state.doc.toString() !== text) return;
      const len = view.state.doc.length;
      const list = diags.map((d) => {
        let from = Math.min(d.from, len), to = Math.min(Math.max(d.to, d.from), len);
        if (from === to && to < len) to = from + 1;
        const message = d.code + " " + d.message + (d.hint ? "\nhint: " + d.hint : "");
        const actions = (d.fixes || []).map((f) => ({
          name: f.title,
          apply(v) {
            if (v.state.doc.toString() !== text) return;
            v.dispatch({ changes: { from: f.from, to: f.to, insert: f.insert } });
          },
        }));
        return { from, to, severity: SEVERITY[d.severity] || "info", message, actions };
      });
      view.dispatch(setDiagnostics(view.state, list));
    },
  };
}
