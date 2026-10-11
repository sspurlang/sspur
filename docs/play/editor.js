// The playground editor: CodeMirror 6 with an SSPUR mode. tools/build_playground.sh bundles this
// file with its imports into editor.bundle.js.
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, dropCursor, highlightSpecialChars, hoverTooltip } from "@codemirror/view";
import { EditorState, Compartment } from "@codemirror/state";
import { StreamLanguage, syntaxHighlighting, HighlightStyle, bracketMatching, indentService, indentUnit, foldGutter, foldService, foldKeymap } from "@codemirror/language";
import { defaultKeymap, history, historyKeymap, indentWithTab, toggleComment } from "@codemirror/commands";
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
    indentOnInput: /^\s*[|=.]$/,
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

// The indent of a new line typed after lines[i], skipping blank lines above.
function enterIndent(lines, i) {
  const nonblank = (j) => { while (j >= 0 && codeOf(lines[j]).trim() === "") j--; return j; };
  i = nonblank(i);
  if (i < 0) return 0;
  const line = lines[i], base = indentOf(line), prev = nonblank(i - 1);
  if (/[([{]$/.test(codeOf(line))) return base + 2;
  if (endsOp(line)) return startsOp(line) || (prev >= 0 && endsOp(lines[prev])) ? base : base + 2;
  const o = indentFor(line);
  if (o !== base || startsOp(line)) return o;
  if (prev < 0 || !endsOp(lines[prev])) return base;
  let j = prev;
  for (let k = nonblank(j - 1); k >= 0 && endsOp(lines[k]); k = nonblank(k - 1)) j = k;
  return indentOf(lines[j]);
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
      ".cm-diagnostic": { fontFamily: "var(--sans)", fontSize: "13px" },
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

const SEVERITY = { error: "error", warning: "warning", hole: "info" };

export function createEditor({ parent, doc, dark, api, onChange, commands }) {
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
        foldByIndent,
        bracketMatching(),
        closeBrackets(),
        autocompletion({ override: [completionSource(api)], activateOnTypingDelay: 120, icons: true }),
        highlightActiveLine(),
        highlightSelectionMatches(),
        hoverSource(api),
        lintGutter(),
        themeConf.of(styled(dark)),
        keymap.of([
          { key: "Mod-Enter", run: () => (commands.run(), true), preventDefault: true },
          { key: "Mod-Shift-Enter", run: () => (commands.test(), true), preventDefault: true },
          { key: "Shift-Alt-f", run: () => (commands.format(), true), preventDefault: true },
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
    goto(pos) {
      view.dispatch({ selection: { anchor: pos }, scrollIntoView: true });
      view.focus();
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
