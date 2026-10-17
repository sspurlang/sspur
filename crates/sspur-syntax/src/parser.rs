use crate::ast::*;
use crate::lexer::{lex, Tok, Token, SOFT_KEYWORDS};
use crate::SyntaxError;

type PResult<T> = Result<T, SyntaxError>;

pub fn parse(src: &str) -> PResult<Module> {
    parse_noted(src).map(|(m, _)| m)
}

/// Like `parse`, and also returns one note per foreign spelling the parser accepted
/// (`&&`, `elif`, `let`, ...); the module holds the canonical form.
pub fn parse_noted(src: &str) -> PResult<(Module, Vec<String>)> {
    parse_tokens(lex(src)?)
}

/// `parse_noted` of tokens already lexed.
pub(crate) fn parse_tokens(toks: Vec<Token>) -> PResult<(Module, Vec<String>)> {
    let mut p = Parser::new(toks);
    let mut m = p.module()?;
    normalize(&mut m);
    Ok((m, p.notes))
}

const BUILTIN_TYPE_NAMES: &[&str] = &["Int", "I8", "I16", "I32", "U8", "U16", "U32", "U64", "F32", "F64", "Bool", "Str", "Unit", "List", "Opt", "Res", "Map", "Secret", "Pii", "Untrusted", "Guess"];

fn normalize(m: &mut Module) {
    let names: Vec<String> = m.defs.iter().filter_map(|d| if let Def::Type(t) = d { Some(t.name.clone()) } else { None }).collect();
    let imported: Vec<String> = m.defs.iter().filter_map(|d| if let Def::Use(u) = d { Some(u.names.clone()) } else { None }).flatten().collect();
    for d in &mut m.defs {
        let Def::Type(t) = d else { continue };
        if let TypeBody::Alias(Ty::Named { name, args, .. }, None) = &t.body {
            let known = names.contains(name) || BUILTIN_TYPE_NAMES.contains(&name.as_str()) || t.params.iter().any(|p| &p.name == name) || name.contains('.') || imported.contains(name);
            if args.is_empty() && !known {
                t.body = TypeBody::Sum(vec![Variant { name: name.clone(), fields: None }]);
            }
        }
    }
}

pub fn parse_expr(src: &str) -> PResult<Expr> {
    let toks = lex(src)?;
    let mut p = Parser::new(toks);
    let e = p.expr_seq()?;
    p.skip_newlines();
    p.expect_eof()?;
    Ok(e)
}

enum IfCond {
    Plain(Expr),
    /// The first test, its guard, and the span of the whole condition.
    Is(Expr, Pat, Option<Expr>, Span),
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    line_indent: u32,
    in_refine: bool,
    notes: Vec<String>,
}

fn is_upper(s: &str) -> bool {
    s.rsplit('.').next().and_then(|t| t.chars().next()).is_some_and(|c| c.is_ascii_uppercase())
}

impl Parser {
    fn new(toks: Vec<Token>) -> Self {
        Parser { toks, pos: 0, line_indent: 0, in_refine: false, notes: Vec::new() }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Sym(x) if *x == s)
    }

    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Kw(x) if *x == k) || (SOFT_KEYWORDS.contains(&k) && matches!(self.peek(), Tok::Ident(x) if x == k))
    }

    fn eat_sym(&mut self, s: &str) -> bool {
        if self.is_sym(s) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn note(&mut self, n: &str) {
        if !self.notes.iter().any(|x| x == n) {
            self.notes.push(n.to_string());
        }
    }

    /// `&&` or `||` as two adjacent symbol tokens.
    fn doubled(&self, s: &str) -> bool {
        self.is_sym(s) && matches!(self.peek_at(1), Tok::Sym(x) if *x == s) && self.toks.get(self.pos + 1).is_some_and(|t| t.span.start == self.span().end)
    }

    fn err<T>(&self, code: &'static str, msg: impl Into<String>) -> PResult<T> {
        Err(SyntaxError::new(code, msg.into(), self.span()))
    }

    fn describe(&self) -> String {
        match self.peek() {
            Tok::Ident(s) => format!("'{s}'"),
            Tok::Int(n) => format!("'{n}'"),
            Tok::Float(n) => format!("'{n}'"),
            Tok::Str(_) | Tok::Interp(_) => "string".into(),
            Tok::Hash(h) => format!("'#{h}'"),
            Tok::Kw(k) => format!("'{k}'"),
            Tok::Sym(s) => format!("'{s}'"),
            Tok::Newline(_) => "end of line".into(),
            Tok::Eof => "end of input".into(),
        }
    }

    fn expect_sym(&mut self, s: &str) -> PResult<Span> {
        if self.is_sym(s) {
            Ok(self.bump().span)
        } else {
            self.err("E_PARSE_EXPECTED", format!("expected '{s}', found {}", self.describe()))
        }
    }

    fn expect_kw(&mut self, k: &str) -> PResult<Span> {
        if self.is_kw(k) {
            Ok(self.bump().span)
        } else {
            self.err("E_PARSE_EXPECTED", format!("expected '{k}', found {}", self.describe()))
        }
    }

    fn expect_ident(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            _ => self.err("E_PARSE_EXPECTED", format!("expected identifier, found {}", self.describe())),
        }
    }

    fn expect_eof(&mut self) -> PResult<()> {
        if matches!(self.peek(), Tok::Eof) {
            Ok(())
        } else {
            self.err("E_PARSE_TRAILING", format!("unexpected {}", self.describe()))
        }
    }

    fn skip_newlines(&mut self) {
        while let Tok::Newline(c) = *self.peek() {
            self.line_indent = c;
            self.bump();
        }
    }

    fn newline_then(&self, pred: impl Fn(&Tok) -> bool) -> Option<u32> {
        match self.peek() {
            Tok::Newline(c) if pred(self.peek_at(1)) => Some(*c),
            _ => None,
        }
    }

    fn module(&mut self) -> PResult<Module> {
        self.skip_newlines();
        let mut profile = None;
        if self.eat_kw("profile") {
            profile = Some(self.expect_ident()?);
        }
        let mut defs: Vec<Def> = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek(), Tok::Eof) {
                break;
            }
            let d = self.def()?;
            if let Def::Use(u) = &d
                && let Some(Def::Use(prev)) = defs.iter_mut().find(|p| p.name() == u.key) {
                    for n in &u.names {
                        if !prev.names.contains(n) {
                            prev.names.push(n.clone());
                        }
                    }
                    continue;
                }
            defs.push(d);
            match self.peek() {
                Tok::Newline(_) | Tok::Eof => {}
                _ => return self.err("E_PARSE_TRAILING", format!("unexpected {} after definition", self.describe())),
            }
        }
        Ok(Module { profile, defs, own: None })
    }

    fn def(&mut self) -> PResult<Def> {
        let start = self.span();
        if matches!(self.peek(), Tok::Ident(w) if w == "pub") && !matches!(self.peek_at(1), Tok::Sym(_) | Tok::Newline(_) | Tok::Eof) {
            self.bump();
            let mut d = self.def()?;
            match &mut d {
                Def::Type(t) => (t.public, t.span) = (true, start.to(t.span)),
                Def::Fn(f) => (f.public, f.span) = (true, start.to(f.span)),
                Def::Effect(e) => (e.public, e.span) = (true, start.to(e.span)),
                Def::Trait(t) => (t.public, t.span) = (true, start.to(t.span)),
                Def::Impl(i) => (i.public, i.span) = (true, start.to(i.span)),
                _ => return Err(SyntaxError::new("E_PARSE_PUB", "only fn, type, effect, trait and impl definitions can be pub".into(), start)),
            }
            return Ok(d);
        }
        if matches!(self.peek(), Tok::Ident(w) if w == "use") && matches!(self.peek_at(1), Tok::Ident(_)) {
            return self.use_def();
        }
        match self.peek() {
            Tok::Kw("type") => self.type_def().map(Def::Type),
            Tok::Ident(r) if r == "res" && matches!(self.peek_at(1), Tok::Kw("type")) => {
                self.bump();
                let mut t = self.type_def()?;
                t.res = true;
                t.span = start.to(t.span);
                Ok(Def::Type(t))
            }
            Tok::Kw("fn") => self.fn_def().map(Def::Fn),
            Tok::Ident(w) if w == "rule" => self.rule_def().map(Def::Fn),
            Tok::Ident(w) if w == "extern" && matches!(self.peek_at(1), Tok::Kw("fn")) => self.extern_def().map(Def::Fn),
            Tok::Ident(w) if w == "kernel" && matches!(self.peek_at(1), Tok::Kw("fn")) => {
                self.bump();
                let mut f = self.fn_def_as(true)?;
                if f.kernel.is_none() {
                    return self.err("E_PARSE_KERNEL", "a kernel fn needs '@grid(threads, group)' after its signature");
                }
                f.span = start.to(f.span);
                Ok(Def::Fn(f))
            }
            Tok::Kw("test") => {
                self.bump();
                let name = self.expect_ident()?;
                self.expect_sym("=")?;
                let body = self.expr_seq()?;
                Ok(Def::Test(TestDef { name, span: start.to(self.prev_span()), body }))
            }
            Tok::Ident(w) if w == "static" && matches!(self.peek_at(1), Tok::Ident(_)) && matches!(self.peek_at(2), Tok::Sym(":")) => {
                self.bump();
                let name = self.expect_ident()?;
                self.expect_sym(":")?;
                let ty = self.ty()?;
                self.expect_sym("=")?;
                let init = self.expr()?;
                Ok(Def::Static(StaticDef { name, ty, init, span: start.to(self.prev_span()) }))
            }
            Tok::Ident(w) if w == "effect" => self.effect_def().map(Def::Effect),
            Tok::Ident(w) if w == "store" => self.store_def().map(Def::Store),
            Tok::Ident(w) if w == "svc" => self.svc_def().map(Def::Svc),
            Tok::Ident(w) if w == "trait" && matches!(self.peek_at(1), Tok::Ident(_)) => self.trait_def().map(Def::Trait),
            Tok::Ident(w) if w == "impl" && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Sym("[")) => self.impl_def().map(Def::Impl),
            Tok::Ident(k) if matches!(k.as_str(), "trait" | "impl" | "queue") => {
                let k = k.clone();
                self.err("E_UNSUPPORTED", format!("'{k}' definitions are not supported by this compiler version yet"))
            }
            _ => self.err("E_PARSE_DEF", format!("expected a definition, found {}", self.describe())),
        }
    }

    fn use_def(&mut self) -> PResult<Def> {
        let start = self.bump().span;
        if let Tok::Ident(q) = self.peek().clone()
            && let Some((p, n)) = q.rsplit_once('.') {
                return Err(SyntaxError::new("E_PARSE_USE", format!("write 'use {p}' or 'use {p}.{{{n}}}'"), self.span()));
            }
        let mut pkg = self.expect_ident()?;
        // A public module of a package: `use pkg.models.{Doc}`.
        while self.is_sym(".") && matches!(self.peek_at(1), Tok::Ident(_)) && matches!(self.peek_at(2), Tok::Sym(".")) {
            self.bump();
            pkg.push('.');
            pkg.push_str(&self.expect_ident()?);
        }
        let mut names = Vec::new();
        if self.eat_sym(".") {
            if self.eat_sym("{") {
                while !self.is_sym("}") {
                    names.push(self.expect_ident()?);
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym("}")?;
            } else {
                names.push(self.expect_ident()?);
            }
        }
        Ok(Def::Use(UseDef::new(pkg, names, start.to(self.prev_span()))))
    }

    fn member_fns(&mut self, what: &str, default_ok: bool) -> PResult<Vec<(FnDef, bool)>> {
        let mut out = Vec::new();
        while let Some(c) = self.newline_then(|t| matches!(t, Tok::Kw("fn"))) {
            if c == 0 {
                break;
            }
            self.line_indent = c;
            self.bump();
            let start = self.span();
            let mut f = self.fn_header(start)?;
            let body = self.is_sym("=") || self.newline_then(|t| matches!(t, Tok::Sym("="))).is_some();
            if body {
                if self.newline_then(|t| matches!(t, Tok::Sym("="))).is_some() {
                    let Tok::Newline(c) = *self.peek() else { unreachable!() };
                    self.line_indent = c;
                    self.bump();
                }
                self.expect_sym("=")?;
                f.body = self.block_or_seq()?;
                f.span = start.to(self.prev_span());
            } else if !default_ok {
                return self.err("E_PARSE_IMPL", format!("{what} method '{}' needs a body: 'fn {}(...) -> T = expr'", f.name, f.name));
            }
            self.line_indent = c;
            out.push((f, body));
        }
        if let Some(c) = self.newline_then(|_| true)
            && c > 0
            && !matches!(self.peek_at(1), Tok::Eof | Tok::Newline(_))
        {
            self.bump();
            return self.err("E_PARSE_IMPL", format!("expected an indented 'fn' line in the {what}, found {}", self.describe()));
        }
        self.line_indent = 0;
        Ok(out)
    }

    fn trait_def(&mut self) -> PResult<TraitDef> {
        let start = self.bump().span;
        let name = self.expect_ident()?;
        let params = self.tparams()?;
        let methods = self.member_fns("trait", true)?.into_iter().map(|(sig, default)| TraitMethod { sig, default }).collect();
        Ok(TraitDef { name, params, methods, public: false, span: start.to(self.prev_span()) })
    }

    fn impl_def(&mut self) -> PResult<ImplDef> {
        let start = self.bump().span;
        let tparams = self.tparams()?;
        let trait_name = self.expect_ident()?;
        let mut trait_args = Vec::new();
        if self.eat_sym("[") {
            while !self.is_sym("]") {
                trait_args.push(self.ty()?);
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.expect_sym("]")?;
        }
        if !self.eat_kw("for") {
            return self.err("E_PARSE_IMPL", format!("expected 'for' after 'impl {trait_name}': write 'impl {trait_name} for Type'"));
        }
        let target = self.ty()?;
        let fns = self.member_fns("impl", false)?.into_iter().map(|(f, _)| f).collect();
        let mut i = ImplDef { key: String::new(), tparams, trait_name, trait_args, target, fns, public: false, span: start.to(self.prev_span()) };
        i.refresh_key();
        Ok(i)
    }

    fn store_def(&mut self) -> PResult<StoreDef> {
        let start = self.expect_kw("store")?;
        let name = self.expect_ident()?;
        self.expect_sym("=")?;
        let kind = self.expect_ident()?;
        if kind != "table" {
            return self.err("E_UNSUPPORTED", format!("store kind '{kind}' is not supported yet; use table[K, V]"));
        }
        self.expect_sym("[")?;
        let key = self.ty()?;
        self.expect_sym(",")?;
        let val = self.ty()?;
        self.expect_sym("]")?;
        Ok(StoreDef { name, kind, key, val, span: start.to(self.prev_span()) })
    }

    fn svc_def(&mut self) -> PResult<SvcDef> {
        let start = self.expect_kw("svc")?;
        let name = self.expect_ident()?;
        #[cfg(not(target_family = "wasm"))]
        let params = if self.is_sym("(") { self.params()? } else { vec![] };
        let mut eps = Vec::new();
        while let Some(c) = self.newline_then(|t| matches!(t, Tok::Ident(n) if n == "ep")) {
            if c == 0 {
                break;
            }
            self.bump();
            let es = self.span();
            self.bump();
            let method = if self.eat_kw("post") { "post".to_string() } else { self.expect_ident()? };
            let Some(path) = raw_text(self.peek()) else {
                return self.err("E_PARSE_EP", format!("expected a path string after 'ep {method}', found {}", self.describe()));
            };
            self.bump();
            self.expect_sym("=")?;
            let handler = self.expect_ident()?;
            eps.push(Endpoint { method, path, handler, span: es.to(self.prev_span()) });
        }
        if eps.is_empty() {
            return self.err("E_PARSE_SVC", "a svc needs at least one endpoint: indented lines 'ep get \"/path/{id}\" = handler'");
        }
        Ok(SvcDef {
            name,
            #[cfg(not(target_family = "wasm"))]
            params,
            eps,
            span: start.to(self.prev_span()),
        })
    }

    fn effect_def(&mut self) -> PResult<EffectDef> {
        let start = self.expect_kw("effect")?;
        let name = self.expect_ident()?;
        let params = self.tparams()?;
        let mut ops = Vec::new();
        if self.is_sym("(") {
            ops.push(self.op_sig(name.clone())?);
        } else {
            while let Some(c) = self.newline_then(|t| matches!(t, Tok::Ident(_))) {
                if c == 0 {
                    break;
                }
                self.bump();
                let op = self.expect_ident()?;
                ops.push(self.op_sig(op)?);
            }
            if ops.is_empty() {
                return self.err("E_PARSE_EFFECT", "an effect needs at least one operation: 'effect name(x: T) -> R', or operations on indented lines");
            }
        }
        Ok(EffectDef { name, params, ops, public: false, span: start.to(self.prev_span()) })
    }

    fn op_sig(&mut self, name: String) -> PResult<OpSig> {
        let params = self.params()?;
        let ret = if self.eat_sym("->") { Some(self.ty()?) } else { None };
        Ok(OpSig { name, params, ret })
    }

    fn params(&mut self) -> PResult<Vec<Param>> {
        self.expect_sym("(")?;
        let mut params = Vec::new();
        while !self.is_sym(")") {
            let pname = self.expect_ident()?;
            self.expect_sym(":")?;
            let ty = self.ty()?;
            let refine = if self.eat_kw("where") { Some(self.refine_expr()?) } else { None };
            params.push(Param { name: pname, ty, refine });
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_sym(")")?;
        Ok(params)
    }

    fn tparams(&mut self) -> PResult<Vec<TParam>> {
        let mut out = Vec::new();
        if !self.eat_sym("[") {
            return Ok(out);
        }
        loop {
            let name = self.expect_ident()?;
            let mut kind = None;
            let mut refine = None;
            let mut bounds = Vec::new();
            if self.eat_sym(":") {
                let first = self.ty()?;
                let is_kind = matches!(&first, Ty::Named { name, args, .. } if args.is_empty() && matches!(name.as_str(), "Int" | "I8" | "I16" | "I32" | "U8" | "U16" | "U32" | "U64" | "Bool"));
                if is_kind && !self.is_sym("+") {
                    kind = Some(first);
                    if self.eat_kw("where") {
                        refine = Some(self.refine_expr()?);
                    }
                } else {
                    bounds.push(first);
                    while self.eat_sym("+") {
                        bounds.push(self.ty()?);
                    }
                }
            }
            out.push(TParam { name, kind, refine, bounds });
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_sym("]")?;
        Ok(out)
    }

    /// A `pre` or `post` condition: `_` in it is a placeholder lambda, as in any call argument.
    fn clause(&mut self) -> PResult<Expr> {
        let saved = self.in_refine;
        self.in_refine = false;
        let e = self.binary(1);
        self.in_refine = saved;
        e
    }

    fn refine_expr(&mut self) -> PResult<Expr> {
        let saved = self.in_refine;
        self.in_refine = true;
        let e = self.binary(1);
        self.in_refine = saved;
        e
    }

    fn type_def(&mut self) -> PResult<TypeDef> {
        let start = self.expect_kw("type")?;
        let name = self.expect_ident()?;
        let params = self.tparams()?;
        self.expect_sym("=")?;
        let body = self.type_body()?;
        let mut derives = Vec::new();
        if self.eat_kw("derive") {
            loop {
                derives.push(self.expect_ident()?);
                if !self.eat_sym(",") {
                    break;
                }
            }
        }
        let drop = if matches!(self.peek(), Tok::Ident(d) if d == "drop") && matches!(self.peek_at(1), Tok::Ident(_)) {
            self.bump();
            Some(self.expect_ident()?)
        } else {
            None
        };
        Ok(TypeDef { name, params, body, derives, res: false, drop, public: false, span: start.to(self.prev_span()) })
    }

    fn type_body(&mut self) -> PResult<TypeBody> {
        // `type S =` and then the variants each on a line starting with `|`, as in OCaml or F#.
        if self.is_sym("|") && matches!(self.peek_at(1), Tok::Ident(s) if is_upper(s)) {
            self.bump();
        }
        if self.is_sym("{") {
            return Ok(TypeBody::Record(self.fields()?));
        }
        if self.eat_kw("new") {
            return Ok(TypeBody::New(self.ty()?));
        }
        let first_is_variant = matches!(self.peek(), Tok::Ident(s) if is_upper(s)) && matches!(self.peek_at(1), Tok::Sym("{"));
        let first = if first_is_variant {
            None
        } else {
            Some(self.ty()?)
        };
        let has_bar = self.is_sym("|") || self.newline_then(|t| matches!(t, Tok::Sym("|"))).is_some();
        if let (Some(ty), false) = (&first, has_bar) {
            let refine = if self.eat_kw("where") { Some(self.refine_expr()?) } else { None };
            return Ok(TypeBody::Alias(ty.clone(), refine));
        }
        let mut variants = Vec::new();
        match first {
            Some(Ty::Named { name, args, .. }) if args.is_empty() && is_upper(&name) => {
                variants.push(Variant { name, fields: None })
            }
            Some(_) => return self.err("E_PARSE_VARIANT", "sum variants must be bare constructor names"),
            None => variants.push(self.variant()?),
        }
        loop {
            if self.newline_then(|t| matches!(t, Tok::Sym("|"))).is_some() {
                self.bump();
            }
            if !self.eat_sym("|") {
                break;
            }
            variants.push(self.variant()?);
        }
        Ok(TypeBody::Sum(variants))
    }

    fn variant(&mut self) -> PResult<Variant> {
        let name = self.expect_ident()?;
        if !is_upper(&name) {
            return self.err("E_PARSE_VARIANT", "variant names must start with an uppercase letter");
        }
        let fields = if self.is_sym("{") { Some(self.fields()?) } else { None };
        Ok(Variant { name, fields })
    }

    fn fields(&mut self) -> PResult<Vec<Field>> {
        self.expect_sym("{")?;
        let mut out = Vec::new();
        while !self.is_sym("}") {
            let name = self.expect_ident()?;
            self.expect_sym(":")?;
            let ty = self.ty()?;
            let refine = if self.eat_kw("where") { Some(self.refine_expr()?) } else { None };
            out.push(Field { name, ty, refine });
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_sym("}")?;
        Ok(out)
    }

    fn ty(&mut self) -> PResult<Ty> {
        if self.is_sym("&") {
            let span = self.bump().span;
            let name = if matches!(self.peek(), Tok::Ident(m) if m == "mut") && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Sym("(" | "[")) {
                self.bump();
                "&mut"
            } else {
                "&"
            };
            let inner = self.ty()?;
            return Ok(Ty::Named { name: name.into(), args: vec![inner], span: span.to(self.prev_span()) });
        }
        if matches!(self.peek(), Tok::Ident(o) if o == "own") && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Sym("(")) {
            let span = self.bump().span;
            let inner = self.ty()?;
            return Ok(Ty::Named { name: "own".into(), args: vec![inner], span: span.to(self.prev_span()) });
        }
        if self.is_sym("[") {
            let span = self.bump().span;
            let inner = self.ty()?;
            self.expect_sym("]")?;
            return Ok(Ty::Named { name: "[]".into(), args: vec![inner], span: span.to(self.prev_span()) });
        }
        let base = if self.eat_sym("(") {
            let mut items = Vec::new();
            while !self.is_sym(")") {
                items.push(self.ty()?);
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.expect_sym(")")?;
            if self.is_sym("->") {
                return self.fn_ty(items);
            }
            match items.len() {
                0 => Ty::Named { name: "Unit".into(), args: vec![], span: self.prev_span() },
                1 => items.pop().unwrap(),
                _ => Ty::Tuple(items),
            }
        } else {
            let span = self.span();
            let name = self.expect_ident()?;
            let mut args = Vec::new();
            if self.eat_sym("[") {
                loop {
                    if let Tok::Int(n) = self.peek().clone() {
                        let sp = self.bump().span;
                        args.push(Ty::Named { name: n.to_string(), args: vec![], span: sp });
                        if !self.eat_sym(",") {
                            break;
                        }
                        continue;
                    }
                    args.push(self.ty()?);
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym("]")?;
            }
            Ty::Named { name, args, span: span.to(self.prev_span()) }
        };
        if self.is_sym("->") {
            return self.fn_ty(vec![base]);
        }
        Ok(base)
    }

    fn fn_ty(&mut self, params: Vec<Ty>) -> PResult<Ty> {
        self.expect_sym("->")?;
        let ret = self.ty()?;
        let effects = if self.eat_sym("!") { self.effects()? } else { vec![] };
        Ok(Ty::Fn { params, ret: Box::new(ret), effects })
    }

    fn effects(&mut self) -> PResult<Vec<Effect>> {
        let mut out = Vec::new();
        loop {
            let start = self.span();
            let mut name = self.expect_ident()?;
            while self.is_sym(".") && matches!(self.peek_at(1), Tok::Ident(_)) {
                self.bump();
                name.push('.');
                name.push_str(&self.expect_ident()?);
            }
            let mut args = Vec::new();
            if self.eat_sym("[") {
                loop {
                    args.push(self.ty()?);
                    if !self.eat_sym("|") && !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym("]")?;
            }
            out.push(Effect { name, args, span: start.to(self.prev_span()) });
            let continues = self.is_sym(",") && matches!(self.peek_at(1), Tok::Ident(_)) && !matches!(self.peek_at(2), Tok::Sym(":"));
            if !continues && self.is_sym("!") && matches!(self.peek_at(1), Tok::Ident(_)) {
                self.note("! a ! b -> ! a, b");
            } else if !continues {
                break;
            }
            self.bump();
        }
        Ok(out)
    }

    fn rule_def(&mut self) -> PResult<FnDef> {
        let start = self.span();
        self.toks[self.pos].tok = Tok::Kw("fn");
        let mut f = self.fn_header(start)?;
        let mut rows = Vec::new();
        let body_start = self.span();
        loop {
            if self.newline_then(|t| matches!(t, Tok::Sym("|"))).is_some() {
                self.bump();
            }
            if !self.eat_sym("|") {
                break;
            }
            let mut cells = Vec::new();
            loop {
                cells.push(self.cell()?);
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.expect_sym("=>")?;
            let out = self.expr()?;
            rows.push(TableRow { cells, out });
        }
        if rows.is_empty() {
            return self.err("E_PARSE_RULE", "a rule needs at least one '| cells => result' row");
        }
        f.body = Expr::new(ExprKind::Table(rows), body_start.to(self.prev_span()));
        f.span = start.to(self.prev_span());
        Ok(f)
    }

    fn cell(&mut self) -> PResult<Cell> {
        if self.is_sym("_") && matches!(self.peek_at(1), Tok::Sym("," | "=>")) {
            self.bump();
            return Ok(Cell::Any);
        }
        let looks_like_pat = match self.peek() {
            Tok::Ident(n) => is_upper(n) || n == "none" || ((n == "some" || n == "ok" || n == "err") && matches!(self.peek_at(1), Tok::Sym("("))),
            Tok::Int(_) | Tok::Str(_) | Tok::Kw("true" | "false") => true,
            _ => false,
        };
        if looks_like_pat {
            let saved = self.pos;
            if let Ok(p) = self.pat()
                && matches!(self.peek(), Tok::Sym("," | "=>")) {
                    return Ok(Cell::Pat(p));
                }
            self.pos = saved;
        }
        Ok(Cell::Cond(self.binary(1)?))
    }

    fn extern_def(&mut self) -> PResult<FnDef> {
        let start = self.span();
        self.bump();
        let mut f = self.fn_header(start)?;
        if f.kernel.is_some() {
            return self.err("E_PARSE_KERNEL", "'@grid' belongs on a 'kernel fn'");
        }
        if !f.pres.is_empty() || !f.posts.is_empty() || !f.examples.is_empty() {
            return self.err("E_PARSE_EXTERN", "extern functions have no contracts or examples");
        }
        let mut ext = Extern { lib: None, symbol: f.name.clone() };
        for key in ["from", "as"] {
            if matches!(self.peek(), Tok::Ident(w) if w == key) {
                self.bump();
                let Some(v) = raw_text(self.peek()) else { return self.err("E_PARSE_EXTERN", format!("expected a string after '{key}'")) };
                self.bump();
                if key == "from" { ext.lib = Some(v) } else { ext.symbol = v }
            }
        }
        f.ext = Some(ext);
        f.span = start.to(self.prev_span());
        Ok(f)
    }

    fn fn_def(&mut self) -> PResult<FnDef> {
        self.fn_def_as(false)
    }

    fn fn_def_as(&mut self, kernel: bool) -> PResult<FnDef> {
        let start = self.span();
        let mut f = self.fn_header(start)?;
        if f.kernel.is_some() && !kernel {
            return self.err("E_PARSE_KERNEL", "'@grid' belongs on a 'kernel fn'");
        }
        if self.newline_then(|t| matches!(t, Tok::Sym("="))).is_some() {
            let Tok::Newline(c) = *self.peek() else { unreachable!() };
            self.line_indent = c;
            self.bump();
        }
        self.expect_sym("=")?;
        f.body = if kernel && (self.assign_ahead() || self.place_assign_ahead()) { self.branch()? } else { self.block_or_seq()? };
        f.span = start.to(self.prev_span());
        Ok(f)
    }

    fn fn_header(&mut self, start: Span) -> PResult<FnDef> {
        self.expect_kw("fn")?;
        let name = self.expect_ident()?;
        let tparams = self.tparams()?;
        let params = self.params()?;
        let ret = if self.eat_sym("->") { Some(self.ty()?) } else { None };
        let effects = if self.eat_sym("!") { self.effects()? } else { vec![] };
        let kernel = if self.eat_sym("@") {
            let at = self.prev_span();
            let two = matches!(self.peek(), Tok::Ident(g) if g == "grid2");
            if !two && !matches!(self.peek(), Tok::Ident(g) if g == "grid") {
                return self.err("E_PARSE_KERNEL", "expected 'grid' or 'grid2' after '@'");
            }
            self.bump();
            self.expect_sym("(")?;
            let grid = self.expr()?;
            self.expect_sym(",")?;
            let mut h = None;
            if two {
                h = Some(self.expr()?);
                self.expect_sym(",")?;
            }
            let group = self.expr()?;
            let mut y = None;
            if let Some(h) = h {
                self.expect_sym(",")?;
                y = Some((h, self.expr()?));
            }
            self.expect_sym(")")?;
            let _ = at;
            Some(Box::new(KernelSpec { grid, group, y }))
        } else {
            None
        };
        let sig_span = start.to(self.prev_span());
        let mut pres = Vec::new();
        let mut posts = Vec::new();
        let mut examples = Vec::new();
        let mut trusted = None;
        let mut interrupt = None;
        loop {
            let unsafe_ahead = |p: &Self, i: usize| matches!(p.peek_at(i), Tok::Ident(u) if u == "unsafe") && matches!(p.peek_at(i + 1), Tok::Str(_) | Tok::Interp(_));
            let irq_ahead = |p: &Self, i: usize| matches!(p.peek_at(i), Tok::Ident(u) if u == "interrupt") && matches!(p.peek_at(i + 1), Tok::Int(_) | Tok::Ident(_));
            if self.newline_then(|t| matches!(t, Tok::Ident(w) if matches!(w.as_str(), "pre" | "post" | "ex"))).is_some() || (matches!(self.peek(), Tok::Newline(_)) && (unsafe_ahead(self, 1) || irq_ahead(self, 1))) {
                self.bump();
            }
            if unsafe_ahead(self, 0) {
                self.bump();
                trusted = raw_text(&self.bump().tok);
            } else if irq_ahead(self, 0) {
                self.bump();
                interrupt = Some(match self.bump().tok {
                    Tok::Int(n) => n.to_string(),
                    Tok::Ident(v) => v,
                    _ => unreachable!(),
                });
            } else if self.eat_kw("pre") {
                pres.push(self.clause()?);
            } else if self.eat_kw("post") {
                posts.push(self.clause()?);
            } else if self.eat_kw("ex") {
                examples.push(self.binary(1)?);
            } else {
                break;
            }
        }
        let body = Expr::new(ExprKind::Unit, self.prev_span());
        Ok(FnDef { name, tparams, params, ret, effects, pres, posts, examples, trusted, interrupt, body, span: start.to(self.prev_span()), sig_span, ext: None, kernel, public: false })
    }

    fn expr_seq(&mut self) -> PResult<Expr> {
        let first = self.expr()?;
        if !self.is_sym(";") {
            return Ok(first);
        }
        let start = first.span;
        let mut stmts = vec![Stmt::Expr(first)];
        while self.eat_sym(";") {
            stmts.extend(self.stmt()?);
        }
        Ok(Expr::new(ExprKind::Block(fold_guards(stmts)), start.to(self.prev_span())))
    }

    fn lambda_ahead(&self) -> bool {
        match self.peek() {
            Tok::Ident(_) | Tok::Sym("_") => matches!(self.peek_at(1), Tok::Sym("=>")),
            Tok::Sym("(") => {
                if matches!((self.peek_at(1), self.peek_at(2)), (Tok::Sym(")"), Tok::Sym("=>"))) {
                    return true;
                }
                let mut i = 1;
                loop {
                    match self.peek_at(i) {
                        Tok::Ident(_) | Tok::Sym("_") => {}
                        _ => return false,
                    }
                    match self.peek_at(i + 1) {
                        Tok::Sym(",") => i += 2,
                        Tok::Sym(")") => return matches!(self.peek_at(i + 2), Tok::Sym("=>")),
                        _ => return false,
                    }
                }
            }
            _ => false,
        }
    }

    fn expr(&mut self) -> PResult<Expr> {
        let start = self.span();
        if self.lambda_ahead() {
            let mut params = Vec::new();
            if self.eat_sym("(") {
                while !self.is_sym(")") {
                    params.push(self.lambda_param()?);
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym(")")?;
            } else {
                params.push(self.lambda_param()?);
            }
            self.expect_sym("=>")?;
            let mut body = if self.is_sym("{") && matches!(self.peek_at(1), Tok::Newline(_)) {
                self.bump();
                let b = self.block()?;
                self.expect_sym("}")?;
                self.note("x => { block } -> x => do block");
                b
            } else {
                self.block_or_expr()?
            };
            if tail_return(&mut body) {
                self.note("return e at the end of a lambda -> e");
            }
            if params.iter().any(|p| p == "_") && free_placeholder(&body) {
                return Err(SyntaxError::new("E_PARSE_LAMBDA", "'_ =>' ignores its argument, so '_' in the body can't be it; name the argument: 'x => ...'".into(), body.span));
            }
            return Ok(Expr::new(ExprKind::Lambda { params, body: Box::new(body), implicit: false }, start.to(self.prev_span())));
        }
        match self.peek() {
            Tok::Kw("if") => {
                let if_indent = self.line_indent;
                self.bump();
                self.if_rest(start, if_indent)
            }
            Tok::Kw(k @ ("match" | "catch")) => {
                let is_match = *k == "match";
                let indent = self.line_indent;
                self.bump();
                let scrut = self.expr()?;
                let arms = self.arms(indent)?;
                if arms.is_empty() {
                    return self.err("E_PARSE_ARMS", "expected at least one '|' arm");
                }
                let kind = if is_match {
                    ExprKind::Match(Box::new(scrut), arms, MatchForm::Arms)
                } else {
                    ExprKind::Catch(Box::new(scrut), arms)
                };
                Ok(Expr::new(kind, start.to(self.prev_span())))
            }
            Tok::Kw("handle") => {
                let indent = self.line_indent;
                self.bump();
                let body = self.expr()?;
                let arms = self.handle_arms(indent)?;
                if arms.is_empty() {
                    return self.err("E_PARSE_ARMS", "expected at least one '| op(x) => ...' arm");
                }
                Ok(Expr::new(ExprKind::Handle(Box::new(body), arms), start.to(self.prev_span())))
            }
            Tok::Kw("do") => {
                self.bump();
                self.block()
            }
            Tok::Kw("raise") => {
                self.bump();
                let e = self.expr()?;
                Ok(Expr::new(ExprKind::Raise(Box::new(e)), start.to(self.prev_span())))
            }
            Tok::Kw("return") => {
                self.bump();
                let e = self.expr()?;
                Ok(Expr::new(ExprKind::Return(Box::new(e)), start.to(self.prev_span())))
            }
            _ => {
                let save = self.pos;
                let base = self.binary(1)?;
                if matches!(self.peek(), Tok::Ident(w) if w == "is") {
                    return self.is_test(base, save, start);
                }
                Ok(base)
            }
        }
    }

    /// `base with a := 1, b.c := 2`: `with` binds tighter than comparisons, so
    /// `p == q with a := 1` compares `p` with the updated `q`.
    fn with_updates(&mut self, base: Expr) -> PResult<Expr> {
        let start = base.span;
        self.bump();
        let mut updates = Vec::new();
        loop {
            let first = self.expect_ident()?;
            let mut path = vec![PathSeg::Field(first)];
            loop {
                if self.eat_sym(".") {
                    path.push(PathSeg::Field(self.expect_ident()?));
                } else if self.eat_sym("[") {
                    let i = self.expr()?;
                    self.expect_sym("]")?;
                    path.push(PathSeg::Index(i));
                } else {
                    break;
                }
            }
            self.expect_sym(":=")?;
            let value = self.expr()?;
            updates.push((path, value));
            if !(self.is_sym(",") && self.path_assign_ahead(1)) {
                break;
            }
            self.bump();
        }
        Ok(Expr::new(ExprKind::With(Box::new(base), updates), start.to(self.prev_span())))
    }

    /// The condition after `if`: an expression, or `e is p` / `e is p and rest` (and the
    /// foreign `let p = e`).
/// A lambda's parameter: a name, or `_` for one it ignores.
    fn lambda_param(&mut self) -> PResult<String> {
        if self.eat_sym("_") {
            return Ok("_".into());
        }
        self.expect_ident()
    }

        fn if_cond(&mut self) -> PResult<IfCond> {
        if matches!(self.peek(), Tok::Ident(w) if w == "let") && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Int(_) | Tok::Str(_) | Tok::Kw("true" | "false") | Tok::Sym("(" | "[" | "_" | "-")) {
            self.bump();
            let p = self.pat()?;
            self.expect_sym("=")?;
            let scrut = self.expr()?;
            self.note("if let p = e -> if e is p");
            return Ok(IfCond::Is(scrut, p, None, Span::default()));
        }
        let c = self.expr()?;
        Ok(match c.kind {
            ExprKind::Match(s, arms, MatchForm::Is) => {
                let a = arms.into_iter().next().unwrap();
                IfCond::Is(*s, a.pat, a.guard, c.span)
            }
            kind => IfCond::Plain(Expr::new(kind, c.span)),
        })
    }

    /// `e is p`, after `e`: a `Bool`, with `and rest` a guard that sees the names `p` binds
    /// and with `or rest` an `or`. `is` tests everything before it, so `a and b is p` is
    /// `E_PARSE_IS` unless `a and b` is parenthesized.
    fn is_test(&mut self, subject: Expr, save: usize, start: Span) -> PResult<Expr> {
        let loose = match &subject.kind {
            ExprKind::Binary(op, ..) => op.prec() < 4,
            ExprKind::Unary(UnOp::Not, _) | ExprKind::If(..) | ExprKind::Match(..) | ExprKind::Catch(..) | ExprKind::Handle(..) | ExprKind::Lambda { implicit: false, .. } => true,
            _ => false,
        };
        if loose && !self.parenthesized(save, self.pos) {
            return self.err("E_PARSE_IS", "'is' would test the whole condition before it");
        }
        let at = self.bump().span;
        let pat = self.pat()?;
        let op = self.binop();
        if matches!(self.peek(), Tok::Ident(w) if w == "is") || self.is_sym("..") || op.is_some_and(|o| !matches!(o, BinOp::And | BinOp::Or)) {
            return self.err("E_PARSE_IS", "a pattern test ends at its pattern, so it can't be an operand here");
        }
        let flag = |b: bool| Expr::new(ExprKind::Bool(b), point(at.start));
        let mut guard = None;
        if let Some(o) = op {
            if self.is_sym("&") || self.is_sym("|") {
                self.bump();
                self.note(if o == BinOp::And { "&& -> and" } else { "|| -> or" });
            }
            self.bump();
            let rest = self.expr()?;
            if o == BinOp::Or {
                let test = Expr::new(ExprKind::Match(Box::new(subject), vec![Arm { pat, guard: None, body: flag(true) }, Arm { pat: Pat::Wild, guard: None, body: flag(false) }], MatchForm::Is), start.to(at));
                let span = start.to(self.prev_span());
                return Ok(Expr::new(ExprKind::Binary(BinOp::Or, Box::new(test), Box::new(rest)), span));
            }
            guard = Some(rest);
        }
        let arms = vec![Arm { pat, guard, body: flag(true) }, Arm { pat: Pat::Wild, guard: None, body: flag(false) }];
        Ok(Expr::new(ExprKind::Match(Box::new(subject), arms, MatchForm::Is), start.to(self.prev_span())))
    }

    /// Whether the tokens `from..to` are one parenthesized group.
    fn parenthesized(&self, from: usize, to: usize) -> bool {
        if to < from + 2 || !matches!(self.toks[from].tok, Tok::Sym("(")) {
            return false;
        }
        let mut depth = 0;
        for (i, t) in self.toks[from..to].iter().enumerate() {
            match t.tok {
                Tok::Sym("(") => depth += 1,
                Tok::Sym(")") => {
                    depth -= 1;
                    if depth == 0 && from + i + 1 != to {
                        return false;
                    }
                }
                _ => {}
            }
        }
        depth == 0
    }

    fn if_rest(&mut self, start: Span, if_indent: u32) -> PResult<Expr> {
        let c = self.if_cond()?;
        self.expect_kw("then")?;
        let t = self.branch()?;
        let is_elif = |t: &Tok| matches!(t, Tok::Ident(w) if w == "elif");
        if let Some(col) = self.newline_then(|t| matches!(t, Tok::Kw("else")) || is_elif(t))
            && col >= if_indent {
                self.line_indent = col;
                self.bump();
            }
        let e = if self.eat_kw("else") {
            Some(Box::new(self.branch()?))
        } else if is_elif(self.peek()) {
            self.note("elif -> else if");
            let s2 = self.bump().span;
            let indent = self.line_indent;
            Some(Box::new(self.if_rest(s2, indent)?))
        } else {
            None
        };
        self.line_indent = if_indent;
        let span = start.to(self.prev_span());
        Ok(match c {
            IfCond::Plain(c) => Expr::new(ExprKind::If(Box::new(c), Box::new(t), e), span),
            IfCond::Is(scrut, pat, guard, cspan) => {
                let other = e.map_or_else(|| Expr::new(ExprKind::Unit, span), |e| *e);
                if_is(scrut, pat, guard, cspan, t, other, span)
            }
        })
    }

    fn path_assign_ahead(&self, mut i: usize) -> bool {
        if !matches!(self.peek_at(i), Tok::Ident(_)) {
            return false;
        }
        i += 1;
        loop {
            match self.peek_at(i) {
                Tok::Sym(":=") => return true,
                Tok::Sym(".") if matches!(self.peek_at(i + 1), Tok::Ident(_)) => i += 2,
                Tok::Sym("[") => {
                    let mut depth = 0;
                    loop {
                        match self.peek_at(i) {
                            Tok::Sym("[") => depth += 1,
                            Tok::Sym("]") => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            Tok::Eof => return false,
                            _ => {}
                        }
                        i += 1;
                    }
                    i += 1;
                }
                _ => return false,
            }
        }
    }

    fn place_assign_ahead(&self) -> bool {
        if !matches!(self.peek(), Tok::Ident(_)) {
            return false;
        }
        let mut i = 1;
        let mut segs = 0;
        loop {
            match self.peek_at(i) {
                Tok::Sym(".") if matches!(self.peek_at(i + 1), Tok::Ident(_)) => i += 2,
                Tok::Sym("[") => {
                    let mut depth = 0;
                    loop {
                        match self.peek_at(i) {
                            Tok::Sym("[") => depth += 1,
                            Tok::Sym("]") => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            Tok::Eof | Tok::Newline(_) => return false,
                            _ => {}
                        }
                        i += 1;
                    }
                    i += 1;
                }
                Tok::Sym(":=") => return segs > 0,
                _ => return false,
            }
            segs += 1;
        }
    }

    fn place_assign(&mut self) -> PResult<Stmt> {
        let span = self.span();
        let name = self.expect_ident()?;
        let mut path = Vec::new();
        loop {
            if self.eat_sym(".") {
                path.push(PathSeg::Field(self.expect_ident()?));
            } else if self.eat_sym("[") {
                path.push(PathSeg::Index(self.expr()?));
                self.expect_sym("]")?;
            } else {
                break;
            }
        }
        self.expect_sym(":=")?;
        let value = self.expr()?;
        let full = span.to(self.prev_span());
        let base = Expr::new(ExprKind::Name(name.clone()), span);
        let with = Expr::new(ExprKind::With(Box::new(base), vec![(path, value)]), full);
        Ok(Stmt::Assign(name, with, span))
    }

    /// `p = e else b`, at `else`: the match `| p => <the statements after it> | _ => b`, which
    /// `fold_guards` completes. `else { ... }` (Rust, Swift) is accepted as `else b`.
    fn guard_else(&mut self, p: Pat, value: Expr, start: Span) -> PResult<Stmt> {
        self.expect_kw("else")?;
        let other = if self.is_sym("{") && !(matches!(self.peek_at(1), Tok::Ident(_)) && matches!(self.peek_at(2), Tok::Sym(":" | "," | "}"))) {
            self.bump();
            let b = if matches!(self.peek(), Tok::Newline(_)) {
                self.block()?
            } else {
                self.expr_seq()?
            };
            self.expect_sym("}")?;
            self.note("p = e else { b } -> p = e else b");
            b
        } else {
            self.branch()?
        };
        let span = start.to(self.prev_span());
        let arms = vec![Arm { pat: p, guard: None, body: guard_pending() }, Arm { pat: Pat::Wild, guard: None, body: other }];
        Ok(Stmt::Expr(Expr::new(ExprKind::Match(Box::new(value), arms, MatchForm::LetElse), span)))
    }

    fn assign_ahead(&self) -> bool {
        matches!((self.peek(), self.peek_at(1)), (Tok::Ident(_), Tok::Sym(":=")))
    }

    fn block_or_expr(&mut self) -> PResult<Expr> {
        if matches!(self.peek(), Tok::Newline(c) if *c > self.line_indent) {
            return self.block();
        }
        self.expr()
    }

    fn block_or_seq(&mut self) -> PResult<Expr> {
        if matches!(self.peek(), Tok::Newline(c) if *c > self.line_indent) {
            return self.block();
        }
        self.expr_seq()
    }

    fn branch(&mut self) -> PResult<Expr> {
        if self.place_assign_ahead() {
            let start = self.span();
            let st = self.place_assign()?;
            return Ok(Expr::new(ExprKind::Block(vec![st]), start.to(self.prev_span())));
        }
        if !self.assign_ahead() {
            return self.block_or_expr();
        }
        let span = self.span();
        let Tok::Ident(name) = self.bump().tok else { unreachable!() };
        self.bump();
        let value = self.expr()?;
        let full = span.to(self.prev_span());
        Ok(Expr::new(ExprKind::Block(vec![Stmt::Assign(name, value, span)]), full))
    }

    fn arms(&mut self, indent: u32) -> PResult<Vec<Arm>> {
        let mut arms = Vec::new();
        // A `match` in an arm, with its own arms indented past the outer ones, ends at the first
        // arm to the left of its own.
        let mut col = None;
        loop {
            if let Some(c) = self.newline_then(|t| matches!(t, Tok::Sym("|"))) {
                if c < indent || col.is_some_and(|k| c < k) {
                    break;
                }
                col.get_or_insert(c);
                self.line_indent = c;
                self.bump();
            }
            if !self.eat_sym("|") {
                break;
            }
            let pat = self.pat()?;
            let guard = if self.eat_kw("if") { Some(self.expr()?) } else { None };
            self.expect_sym("=>")?;
            let saved = self.line_indent;
            let body = if self.assign_ahead() || self.place_assign_ahead() { self.branch()? } else { self.block_or_seq()? };
            self.line_indent = saved;
            arms.push(Arm { pat, guard, body });
        }
        Ok(arms)
    }

    fn handle_arms(&mut self, indent: u32) -> PResult<Vec<Arm>> {
        let mut arms = Vec::new();
        let mut col = None;
        loop {
            let op_ahead = |p: &Self, i: usize| match p.peek_at(i) {
                Tok::Kw("return") => true,
                Tok::Ident(n) => !is_upper(n) && matches!(p.peek_at(i + 1), Tok::Sym("(" | "=>")),
                _ => false,
            };
            if let Some(c) = self.newline_then(|t| matches!(t, Tok::Sym("|"))) {
                if c < indent || col.is_some_and(|k| k != c) || !op_ahead(self, 2) {
                    break;
                }
                col = Some(c);
                self.line_indent = c;
                self.bump();
            } else if self.is_sym("|") && !op_ahead(self, 1) {
                break;
            }
            if !self.eat_sym("|") {
                break;
            }
            let pat = if self.eat_kw("return") {
                self.expect_sym("(")?;
                let p = self.pat()?;
                self.expect_sym(")")?;
                Pat::Ctor { name: "return".into(), args: CtorArgs::Positional(vec![p]) }
            } else {
                match self.pat()? {
                    Pat::Bind(name) => Pat::Ctor { name, args: CtorArgs::Positional(vec![]) },
                    p @ Pat::Ctor { args: CtorArgs::Positional(_), .. } => p,
                    _ => return self.err("E_PARSE_HANDLER", "expected an operation arm like '| op(x) =>' or '| return(r) =>'"),
                }
            };
            self.expect_sym("=>")?;
            let saved = self.line_indent;
            let body = if self.assign_ahead() || self.place_assign_ahead() { self.branch()? } else { self.block_or_seq()? };
            self.line_indent = saved;
            arms.push(Arm { pat, guard: None, body });
        }
        Ok(arms)
    }

    fn block(&mut self) -> PResult<Expr> {
        let start = self.prev_span();
        let outer = self.line_indent;
        let alone = self.pos >= 2 && matches!(self.toks[self.pos - 2].tok, Tok::Newline(_));
        let col = match *self.peek() {
            Tok::Newline(c) if c > outer || (alone && c == outer) => c,
            _ => return self.err("E_PARSE_BLOCK", "expected an indented block on the next line"),
        };
        let mut stmts = Vec::new();
        while let Tok::Newline(c) = *self.peek() {
            if c >= col && matches!(self.peek_at(1), Tok::Sym(")" | "]" | "}")) {
                self.bump();
                break;
            }
            // No statement starts with `|`, so a `|` line at the block's indent ends the block: it is
            // an arm of the `catch`, `match` or `handle` the block belongs to (`catch do` + lines + `| _ => e`).
            if c == col && !stmts.is_empty() && matches!(self.peek_at(1), Tok::Sym("|")) {
                break;
            }
            if c != col {
                if c > col {
                    return self.err("E_PARSE_INDENT", "unexpected indentation");
                }
                break;
            }
            self.line_indent = c;
            self.bump();
            stmts.extend(self.stmt()?);
        }
        self.line_indent = outer;
        let mut stmts = fold_guards(stmts);
        if let [Stmt::Expr(_)] = stmts.as_slice() {
            let Some(Stmt::Expr(e)) = stmts.pop() else { unreachable!() };
            return Ok(e);
        }
        Ok(Expr::new(ExprKind::Block(stmts), start.to(self.prev_span())))
    }

    fn loop_body(&mut self) -> PResult<Expr> {
        if matches!(self.peek(), Tok::Newline(c) if *c > self.line_indent) {
            return self.block();
        }
        if self.is_kw("do") && matches!(self.peek_at(1), Tok::Newline(_)) {
            self.bump();
            return self.block();
        }
        self.branch()
    }

    fn stmt(&mut self) -> PResult<Vec<Stmt>> {
        if self.is_kw("fn") {
            let indent = self.line_indent;
            let f = self.fn_def()?;
            self.line_indent = indent;
            return Ok(vec![Stmt::Fn(Box::new(f))]);
        }
        if let (Tok::Ident(g), Tok::Ident(w)) = (self.peek(), self.peek_at(1))
            && g == "guard"
            && matches!(w.as_str(), "let" | "case")
        {
            let case = w == "case";
            let start = self.span();
            self.bump();
            self.bump();
            let p = if case {
                self.pat()?
            } else {
                let n = self.expect_ident()?;
                Pat::Ctor { name: "some".into(), args: CtorArgs::Positional(vec![Pat::Bind(n)]) }
            };
            self.expect_sym("=")?;
            let value = self.expr()?;
            if !self.is_kw("else") {
                return self.err("E_PARSE_GUARD", "a guard needs 'else' and what to do when the pattern doesn't match");
            }
            self.note(if case { "guard case p = e else -> p = e else" } else { "guard let x = e else -> some(x) = e else" });
            return Ok(vec![self.guard_else(p, value, start)?]);
        }
        if let Tok::Ident(w) = self.peek().clone()
            && matches!(w.as_str(), "let" | "const" | "val")
            && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Sym("(" | "["))
            && !matches!(self.peek_at(2), Tok::Sym(":=")) {
                self.bump();
                if matches!(self.peek(), Tok::Ident(m) if m == "mut") && matches!(self.peek_at(1), Tok::Ident(_)) {
                    self.bump();
                    self.note("let mut x = e -> var x = e");
                    let name = self.expect_ident()?;
                    let ann = if self.eat_sym(":") { Some(self.ty()?) } else { None };
                    self.expect_sym("=")?;
                    let v = self.expr()?;
                    return Ok(vec![Stmt::Var(name, match ann { Some(t) => v.ascribed(t), None => v })]);
                }
                self.note("let x = e -> x = e");
            }
        if let (Tok::Ident(name), Tok::Sym(op @ ("+" | "-" | "*" | "/" | "%")), Tok::Sym("=")) = (self.peek().clone(), self.peek_at(1).clone(), self.peek_at(2))
            && self.toks[self.pos + 2].span.start == self.toks[self.pos + 1].span.end {
                let bop = match op {
                    "+" => BinOp::Add,
                    "-" => BinOp::Sub,
                    "*" => BinOp::Mul,
                    "/" => BinOp::Div,
                    _ => BinOp::Rem,
                };
                let span = self.span();
                self.bump();
                self.bump();
                self.bump();
                self.note(&format!("x {op}= e -> x := x {op} e"));
                let rhs = self.expr()?;
                let full = span.to(rhs.span);
                let value = Expr::new(ExprKind::Binary(bop, Box::new(Expr::new(ExprKind::Name(name.clone()), span)), Box::new(rhs)), full);
                return Ok(vec![Stmt::Assign(name, value, span)]);
            }
        if self.eat_kw("var") {
            if self.is_sym("(") {
                let p = self.pat()?;
                self.expect_sym("=")?;
                let value = self.expr()?;
                let mut names = Vec::new();
                pat_binds(&p, &mut names);
                let mut out = vec![Stmt::Let(p, value)];
                for n in names {
                    let span = self.prev_span();
                    out.push(Stmt::Var(n.clone(), Expr::new(ExprKind::Name(n), span)));
                }
                return Ok(out);
            }
            let name = self.expect_ident()?;
            let ann = if self.eat_sym(":") { Some(self.ty()?) } else { None };
            self.expect_sym("=")?;
            let v = self.expr()?;
            return Ok(vec![Stmt::Var(name, match ann { Some(t) => v.ascribed(t), None => v })]);
        }
        if let (Tok::Ident(_), Tok::Sym(":")) = (self.peek().clone(), self.peek_at(1)) {
            let saved = self.pos;
            let name = self.expect_ident()?;
            self.bump();
            if let Ok(t) = self.ty()
                && self.eat_sym("=")
            {
                return Ok(vec![Stmt::Let(Pat::Bind(name), self.expr()?.ascribed(t))]);
            }
            self.pos = saved;
        }
        if self.eat_kw("for") {
            let pat = self.pat()?;
            self.expect_kw("in")?;
            let iter = self.expr()?;
            let body = self.loop_body()?;
            return Ok(vec![Stmt::For(pat, iter, body)]);
        }
        if self.eat_kw("while") {
            let start = self.prev_span().start;
            let cond = self.expr()?;
            let body = self.loop_body()?;
            return Ok(vec![while_is(start, cond, body)]);
        }
        if let (Tok::Ident(name), Tok::Sym(":=")) = (self.peek().clone(), self.peek_at(1)) {
            let span = self.span();
            self.bump();
            self.bump();
            return Ok(vec![Stmt::Assign(name, self.expr()?, span)]);
        }
        if self.place_assign_ahead() {
            return Ok(vec![self.place_assign()?]);
        }
        let saved = self.pos;
        let start = self.span();
        if let Ok(p) = self.pat()
            && self.eat_sym("=") {
                let value = self.expr()?;
                let next_line_else = self.newline_then(|t| matches!(t, Tok::Kw("else"))).is_some_and(|c| c > self.line_indent);
                if next_line_else {
                    self.bump();
                }
                if self.is_kw("else") {
                    if self.notes.last().is_some_and(|n| n == "let x = e -> x = e") {
                        self.notes.pop();
                        self.note("let p = e else -> p = e else");
                    }
                    return Ok(vec![self.guard_else(p, value, start)?]);
                }
                return Ok(vec![Stmt::Let(p, value)]);
            }
        self.pos = saved;
        Ok(vec![Stmt::Expr(self.expr()?)])
    }

    /// A pattern, with `|` between alternatives.
    fn pat(&mut self) -> PResult<Pat> {
        let first = self.pat_one()?;
        if !self.is_sym("|") || self.doubled("|") {
            return Ok(first);
        }
        let mut alts = Vec::new();
        let mut push = |p: Pat| match p {
            Pat::Or(xs) => alts.extend(xs),
            p => alts.push(p),
        };
        push(first);
        while self.is_sym("|") && !self.doubled("|") {
            self.bump();
            push(self.pat_one()?);
        }
        Ok(Pat::Or(alts))
    }

    fn list_pat(&mut self) -> PResult<Pat> {
        self.expect_sym("[")?;
        let (mut head, mut rest, mut tail) = (Vec::new(), None, Vec::new());
        while !self.is_sym("]") {
            let spread = match self.peek() {
                Tok::Sym("..") => Some(None),
                Tok::Sym("...") => Some(Some("...rest -> ..rest")),
                Tok::Sym("*") if matches!(self.peek_at(1), Tok::Ident(_)) => Some(Some("*rest -> ..rest")),
                _ => None,
            };
            if let Some(note) = spread {
                if rest.is_some() {
                    return self.err("E_PARSE_PATTERN", "a list pattern has at most one '..'");
                }
                self.bump();
                if let Some(n) = note {
                    self.note(n);
                }
                let name = match self.peek().clone() {
                    Tok::Ident(n) if !is_upper(&n) => {
                        self.bump();
                        Some(n)
                    }
                    Tok::Sym("_") => {
                        self.bump();
                        None
                    }
                    _ => None,
                };
                rest = Some(name);
            } else if rest.is_some() {
                tail.push(self.pat()?);
            } else {
                head.push(self.pat()?);
            }
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_sym("]")?;
        Ok(Pat::List { head, rest, tail })
    }

    fn pat_one(&mut self) -> PResult<Pat> {
        match self.peek().clone() {
            Tok::Sym("[") => self.list_pat(),
            Tok::Sym("_") => {
                self.bump();
                Ok(Pat::Wild)
            }
            Tok::Int(n) => {
                self.bump();
                Ok(Pat::Int(n))
            }
            Tok::Sym("-") if matches!(self.peek_at(1), Tok::Int(_)) => {
                self.bump();
                let Tok::Int(n) = self.bump().tok else { unreachable!() };
                Ok(Pat::Int(-n))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Pat::Str(s))
            }
            Tok::Interp(_) => self.err("E_PARSE_PATTERN", "a string pattern can't interpolate; write '\\{' for a literal brace"),
            Tok::Kw("true") => {
                self.bump();
                Ok(Pat::Bool(true))
            }
            Tok::Kw("false") => {
                self.bump();
                Ok(Pat::Bool(false))
            }
            Tok::Sym("(") => {
                self.bump();
                let mut items = Vec::new();
                while !self.is_sym(")") {
                    items.push(self.pat()?);
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym(")")?;
                Ok(if items.len() == 1 { items.pop().unwrap() } else { Pat::Tuple(items) })
            }
            Tok::Ident(name) => {
                self.bump();
                if self.eat_sym("(") {
                    let mut items = Vec::new();
                    while !self.is_sym(")") {
                        items.push(self.pat()?);
                        if !self.eat_sym(",") {
                            break;
                        }
                    }
                    self.expect_sym(")")?;
                    return Ok(Pat::Ctor { name, args: CtorArgs::Positional(items) });
                }
                if is_upper(&name) && self.eat_sym("{") {
                    if (self.is_sym("_") || self.is_sym("..")) && matches!(self.peek_at(1), Tok::Sym("}")) {
                        self.bump();
                        self.bump();
                        self.note("Ctor{_} pattern -> Ctor");
                        return Ok(Pat::Ctor { name, args: CtorArgs::None });
                    }
                    let mut fields = Vec::new();
                    while !self.is_sym("}") {
                        let f = self.expect_ident()?;
                        let p = if self.eat_sym(":") { self.pat()? } else { Pat::Bind(f.clone()) };
                        fields.push((f, p));
                        if !self.eat_sym(",") {
                            break;
                        }
                    }
                    self.expect_sym("}")?;
                    return Ok(Pat::Ctor { name, args: CtorArgs::Record(fields) });
                }
                if is_upper(&name) || name == "none" {
                    Ok(Pat::Ctor { name, args: CtorArgs::None })
                } else {
                    Ok(Pat::Bind(name))
                }
            }
            _ => self.err("E_PARSE_PATTERN", format!("expected a pattern, found {}", self.describe())),
        }
    }

    fn binop(&self) -> Option<BinOp> {
        Some(match self.peek() {
            Tok::Sym("+") => BinOp::Add,
            Tok::Sym("-") => BinOp::Sub,
            Tok::Sym("*") => BinOp::Mul,
            Tok::Sym("/") => BinOp::Div,
            Tok::Sym("%") => BinOp::Rem,
            Tok::Sym("**") => BinOp::Pow,
            Tok::Sym("==") => BinOp::Eq,
            Tok::Sym("!=") => BinOp::Ne,
            Tok::Sym("<") => BinOp::Lt,
            Tok::Sym("<=") => BinOp::Le,
            Tok::Sym(">") => BinOp::Gt,
            Tok::Sym(">=") => BinOp::Ge,
            Tok::Kw("and") => BinOp::And,
            Tok::Kw("or") => BinOp::Or,
            Tok::Sym("&") if self.doubled("&") => BinOp::And,
            Tok::Sym("|") if self.doubled("|") => BinOp::Or,
            _ => return None,
        })
    }

    fn binary(&mut self, min: u8) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        let mut tight = true;
        loop {
            if self.is_kw("with") && min <= 5 && tight {
                return self.with_updates(lhs);
            }
            if self.is_sym("..") && min <= 5 {
                self.bump();
                let rhs = self.binary(6)?;
                let span = lhs.span.to(rhs.span);
                lhs = Expr::new(ExprKind::Range(Box::new(lhs), Box::new(rhs)), span);
                continue;
            }
            let Some(op) = self.binop() else { break };
            let prec = op.prec();
            if prec < min {
                break;
            }
            if self.is_sym("&") || self.is_sym("|") {
                self.bump();
                self.note(if op == BinOp::And { "&& -> and" } else { "|| -> or" });
            }
            self.bump();
            tight = prec >= 5;
            let next_min = if op == BinOp::Pow { prec } else { prec + 1 };
            let rhs = self.binary(next_min)?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr::new(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span);
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        let start = self.span();
        if self.eat_kw("not") {
            let e = self.binary(4)?;
            return Ok(Expr::new(ExprKind::Unary(UnOp::Not, Box::new(e)), start.to(self.prev_span())));
        }
        if self.eat_sym("!") {
            self.note("!x -> not x");
            let e = self.unary()?;
            return Ok(Expr::new(ExprKind::Unary(UnOp::Not, Box::new(e)), start.to(self.prev_span())));
        }
        if self.eat_sym("&") {
            let op = if matches!(self.peek(), Tok::Ident(m) if m == "mut") && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Sym("(" | "[")) {
                self.bump();
                UnOp::RefMut
            } else {
                UnOp::Ref
            };
            let e = self.unary()?;
            return Ok(Expr::new(ExprKind::Unary(op, Box::new(e)), start.to(self.prev_span())));
        }
        if self.eat_sym("-") {
            let e = self.unary()?;
            if let ExprKind::Int(n) = e.kind {
                return Ok(Expr::new(ExprKind::Int(-n), start.to(e.span)));
            }
            if let ExprKind::Float(n) = e.kind {
                return Ok(Expr::new(ExprKind::Float(-n), start.to(e.span)));
            }
            return Ok(Expr::new(ExprKind::Unary(UnOp::Neg, Box::new(e)), start.to(self.prev_span())));
        }
        let e = self.primary()?;
        self.postfix(e)
    }

    fn args(&mut self) -> PResult<Vec<Expr>> {
        self.expect_sym("(")?;
        let mut out = Vec::new();
        while !self.is_sym(")") {
            let a = self.expr()?;
            out.push(self.wrap_placeholder(a));
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_sym(")")?;
        Ok(out)
    }

    fn wrap_placeholder(&self, e: Expr) -> Expr {
        if self.in_refine || !has_placeholder(&e) {
            return e;
        }
        let span = e.span;
        Expr::new(ExprKind::Lambda { params: vec!["_".into()], body: Box::new(e), implicit: true }, span)
    }

    fn postfix(&mut self, mut e: Expr) -> PResult<Expr> {
        loop {
            if self.eat_sym(".") {
                let name = match self.peek().clone() {
                    Tok::Ident(s) => s,
                    Tok::Int(n) => n.to_string(),
                    Tok::Kw(k) => k.to_string(),
                    _ => return self.err("E_PARSE_EXPECTED", format!("expected a field or method name, found {}", self.describe())),
                };
                self.bump();
                let mut targs = Vec::new();
                if self.is_sym("[") && matches!(self.peek_at(1), Tok::Ident(s) if is_upper(s)) {
                    let saved = self.pos;
                    self.bump();
                    let parsed: PResult<()> = (|| {
                        loop {
                            targs.push(self.ty()?);
                            if !self.eat_sym(",") {
                                break;
                            }
                        }
                        self.expect_sym("]").map(|_| ())
                    })();
                    if parsed.is_err() {
                        self.pos = saved;
                        targs.clear();
                    }
                }
                if self.is_sym("(") || !targs.is_empty() {
                    let args = if self.is_sym("(") { self.args()? } else { vec![] };
                    let span = e.span.to(self.prev_span());
                    e = Expr::new(ExprKind::Method { recv: Box::new(e), name, targs, args }, span);
                } else {
                    let span = e.span.to(self.prev_span());
                    e = Expr::new(ExprKind::Field(Box::new(e), name), span);
                }
            } else if matches!(&e.kind, ExprKind::Name(n) if n == "mmio") && self.is_sym("[") && matches!(self.peek_at(1), Tok::Ident(s) if is_upper(s)) {
                self.bump();
                let targs = vec![self.ty()?];
                self.expect_sym("]")?;
                let mut args = self.args()?;
                if args.is_empty() {
                    return self.err("E_PARSE_EXPECTED", "mmio[W](addr) needs an address");
                }
                let recv = args.remove(0);
                let span = e.span.to(self.prev_span());
                e = Expr::new(ExprKind::Method { recv: Box::new(recv), name: "mmio".into(), targs, args }, span);
            } else if self.is_sym("(") {
                let args = self.args()?;
                let span = e.span.to(self.prev_span());
                e = Expr::new(ExprKind::Call(Box::new(e), args), span);
            } else if self.eat_sym("[") {
                let idx = self.expr()?;
                self.expect_sym("]")?;
                let span = e.span.to(self.prev_span());
                e = Expr::new(ExprKind::Index(Box::new(e), Box::new(idx)), span);
            } else {
                return Ok(e);
            }
        }
    }

    fn asm(&mut self, start: Span) -> PResult<Expr> {
        let tspan = self.span();
        let parts = match self.bump().tok {
            Tok::Interp(p) => p,
            Tok::Str(s) => vec![StrPart::Lit(s)],
            _ => unreachable!(),
        };
        let mut tmpl = String::new();
        for part in parts {
            match part {
                StrPart::Lit(s) => tmpl.push_str(&s),
                StrPart::Expr(Expr { kind: ExprKind::Name(n), .. }) => tmpl.push_str(&format!("{{{n}}}")),
                StrPart::Expr(_) => return self.err("E_PARSE_ASM", "asm templates refer to operands by name, as {name}"),
            }
        }
        let mut groups: [Vec<(String, Expr)>; 3] = Default::default();
        let mut outs = Vec::new();
        loop {
            let g = match self.peek() {
                Tok::Kw("in") => 0,
                Tok::Ident(s) if s == "out" => 1,
                Tok::Ident(s) if s == "clobber" => 2,
                _ => break,
            };
            if !matches!(self.peek_at(1), Tok::Sym("(")) {
                break;
            }
            self.bump();
            self.bump();
            while !self.is_sym(")") {
                let sp = self.span();
                match (g, self.bump().tok) {
                    (2, Tok::Str(c)) => groups[2].push((c, Expr::new(ExprKind::Unit, sp))),
                    (0 | 1, Tok::Ident(n)) => {
                        self.expect_sym(":")?;
                        if g == 0 {
                            let e = self.expr()?;
                            groups[0].push((n, e));
                        } else {
                            outs.push(self.ty()?);
                            groups[1].push((n, Expr::new(ExprKind::Unit, sp)));
                        }
                    }
                    _ => return self.err("E_PARSE_ASM", "expected in(name: expr), out(name: Type) or clobber(\"reg\")"),
                }
                if !self.eat_sym(",") {
                    break;
                }
            }
            self.expect_sym(")")?;
        }
        let span = start.to(self.prev_span());
        let [ins, outn, clob] = groups;
        let rec = |c: &str, fields| Expr::new(ExprKind::Record { ctor: Some(c.into()), fields }, span);
        let args = vec![rec("in", ins), rec("out", outn), rec("clobber", clob)];
        Ok(Expr::new(ExprKind::Method { recv: Box::new(Expr::new(ExprKind::Str(vec![StrPart::Lit(tmpl)]), tspan)), name: "asm".into(), targs: outs, args }, span))
    }

    fn primary(&mut self) -> PResult<Expr> {
        let start = self.span();
        let tok = self.peek().clone();
        let kind = match tok {
            Tok::Int(n) => {
                self.bump();
                ExprKind::Int(n)
            }
            Tok::Float(n) => {
                self.bump();
                ExprKind::Float(n)
            }
            Tok::Str(s) => {
                self.bump();
                ExprKind::Str(vec![StrPart::Lit(s)])
            }
            Tok::Interp(parts) => {
                self.bump();
                ExprKind::Str(parts)
            }
            Tok::Kw("true") => {
                self.bump();
                ExprKind::Bool(true)
            }
            Tok::Kw("false") => {
                self.bump();
                ExprKind::Bool(false)
            }
            Tok::Sym("_") => {
                self.bump();
                ExprKind::Placeholder
            }
            Tok::Sym("?") => {
                self.bump();
                let adjacent = self.span().start == start.end;
                match self.peek().clone() {
                    Tok::Ident(n) if adjacent => {
                        self.bump();
                        ExprKind::Hole(Some(n))
                    }
                    _ => ExprKind::Hole(None),
                }
            }
            Tok::Kw("par") => {
                self.bump();
                ExprKind::Par(self.args()?)
            }
            Tok::Kw("if" | "match" | "catch" | "handle" | "do" | "raise" | "return") => return self.expr(),
            Tok::Ident(name) if name == "asm" && matches!(self.peek_at(1), Tok::Str(_) | Tok::Interp(_)) => {
                self.bump();
                return self.asm(start);
            }
            Tok::Ident(name) => {
                self.bump();
                if is_upper(&name) && self.is_sym("{") {
                    let fields = self.record_fields()?;
                    ExprKind::Record { ctor: Some(name), fields }
                } else if let Some((p, f)) = name.rsplit_once('.') {
                    // `x.UPPER` is a field access; the resolver turns `pkg.Name` and
                    // `pkg.module.Name` into the package's name.
                    let mut segs = p.split('.');
                    let first = segs.next().unwrap_or_default();
                    let mut end = start.start as usize + first.len();
                    let mut x = Expr::new(ExprKind::Name(first.to_string()), Span::new(start.start as usize, end));
                    for seg in segs {
                        end += 1 + seg.len();
                        x = Expr::new(ExprKind::Field(Box::new(x), seg.to_string()), Span::new(start.start as usize, end));
                    }
                    ExprKind::Field(Box::new(x), f.to_string())
                } else {
                    ExprKind::Name(name)
                }
            }
            Tok::Sym("{") => ExprKind::Record { ctor: None, fields: self.record_fields()? },
            Tok::Sym("[") => {
                self.bump();
                let mut items = Vec::new();
                while !self.is_sym("]") {
                    let item = self.expr()?;
                    if items.is_empty() && self.is_sym(";") {
                        self.bump();
                        let nspan = self.span();
                        let Tok::Int(n) = self.bump().tok else { return self.err("E_PARSE_ARRAY", "expected the array length, a literal, after ';'") };
                        self.expect_sym("]")?;
                        let targs = vec![Ty::Named { name: n.to_string(), args: vec![], span: nspan }];
                        return Ok(Expr::new(ExprKind::Method { recv: Box::new(item), name: "#array".into(), targs, args: vec![] }, start.to(self.prev_span())));
                    }
                    items.push(self.wrap_placeholder(item));
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym("]")?;
                ExprKind::List(items)
            }
            Tok::Sym("(") => {
                self.bump();
                let mut items = Vec::new();
                while !self.is_sym(")") {
                    items.push(self.expr()?);
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym(")")?;
                match items.len() {
                    0 => ExprKind::Unit,
                    1 => return Ok(items.pop().unwrap()),
                    _ => ExprKind::Tuple(items),
                }
            }
            Tok::Hash(_) => return self.err("E_UNSUPPORTED", "hash references are not supported in text input yet"),
            _ => return self.err("E_PARSE_EXPR", format!("expected an expression, found {}", self.describe())),
        };
        Ok(Expr::new(kind, start.to(self.prev_span())))
    }

    fn record_fields(&mut self) -> PResult<Vec<(String, Expr)>> {
        self.expect_sym("{")?;
        let mut out = Vec::new();
        while !self.is_sym("}") {
            let name = self.expect_ident()?;
            let value = if self.eat_sym(":") {
                self.expr()?
            } else {
                Expr::new(ExprKind::Name(name.clone()), self.prev_span())
            };
            out.push((name, value));
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_sym("}")?;
        Ok(out)
    }
}

/// The text of a string token as written between its quotes, interpolations included: what a
/// path or a symbol name means.
fn raw_text(t: &Tok) -> Option<String> {
    match t {
        Tok::Str(s) => Some(s.clone()),
        Tok::Interp(parts) => Some(parts.iter().map(|p| match p {
            StrPart::Lit(s) => s.clone(),
            StrPart::Expr(e) => format!("{{{}}}", crate::printer::expr(e, 0)),
        }).collect()),
        _ => None,
    }
}

fn pat_binds(p: &Pat, out: &mut Vec<String>) {
    p.binds(out)
}

/// Whether `e` has a `_` that no implicit lambda inside it binds.
fn free_placeholder(e: &Expr) -> bool {
    let mut found = false;
    crate::visit::walk_expr(e, &mut |x| {
        found |= matches!(x.kind, ExprKind::Placeholder);
        !matches!(x.kind, ExprKind::Lambda { implicit: true, .. })
    });
    found
}

pub fn has_placeholder(e: &Expr) -> bool {
    let mut found = false;
    crate::visit::walk_expr(e, &mut |x| {
        if matches!(x.kind, ExprKind::Placeholder) {
            found = true;
        }
        !matches!(x.kind, ExprKind::Lambda { .. })
    });
    found
}


/// Replaces `return e` in tail position of a lambda body with `e`.
fn tail_return(e: &mut Expr) -> bool {
    match &mut e.kind {
        ExprKind::Return(x) => {
            let x = std::mem::replace(&mut **x, Expr::new(ExprKind::Unit, Span::default()));
            *e = x;
            true
        }
        ExprKind::Block(stmts) => match stmts.last_mut() {
            Some(Stmt::Expr(x)) => tail_return(x),
            _ => false,
        },
        ExprKind::If(_, t, Some(f)) => tail_return(t) | tail_return(f),
        ExprKind::Match(_, arms, _) | ExprKind::Catch(_, arms) => arms.iter_mut().fold(false, |h, a| tail_return(&mut a.body) | h),
        _ => false,
    }
}

/// `if e1 is p1 and ... then t else other`. One test, or later tests that bind nothing, is the
/// two-arm `IfIs` match with the rest as its guard. When a later test binds names the then
/// branch needs, the tests up to the last such one nest, and the innermost yields `some(t)`:
/// `match (match e1 | p1 => (match e2 | p2 if g => some(t) | _ => none) | _ => none)`
/// `| some(it) => it | _ => other`, so `other` is written once and sees none of the names.
fn if_is(scrut: Expr, pat: Pat, guard: Option<Expr>, cspan: Span, t: Expr, other: Expr, span: Span) -> Expr {
    let mut segs = vec![(scrut, pat, guard, cspan)];
    loop {
        let last = segs.last_mut().unwrap();
        match last.2.take() {
            Some(Expr { kind: ExprKind::Match(s, arms, MatchForm::Is), span }) => {
                let a = arms.into_iter().next().unwrap();
                segs.push((*s, a.pat, a.guard, span));
            }
            g => {
                last.2 = g;
                break;
            }
        }
    }
    let binds = |p: &Pat| {
        let mut v = Vec::new();
        p.binds(&mut v);
        !v.is_empty()
    };
    let k = segs.iter().rposition(|(_, p, _, _)| binds(p)).unwrap_or(0);
    while segs.len() > k + 1 {
        let (s, p, g, sp) = segs.pop().unwrap();
        let flag = |b: bool| Expr::new(ExprKind::Bool(b), point(s.span.end));
        let arms = vec![Arm { pat: p, guard: g, body: flag(true) }, Arm { pat: Pat::Wild, guard: None, body: flag(false) }];
        segs.last_mut().unwrap().2 = Some(Expr::new(ExprKind::Match(Box::new(s), arms, MatchForm::Is), sp));
    }
    if segs.len() == 1 {
        let (s, p, g, _) = segs.pop().unwrap();
        let arms = vec![Arm { pat: p, guard: g, body: t }, Arm { pat: Pat::Wild, guard: None, body: other }];
        return Expr::new(ExprKind::Match(Box::new(s), arms, MatchForm::IfIs), span);
    }
    let at = point(span.start);
    let callee = Expr::new(ExprKind::Name("some".into()), point(span.start + 1));
    let mut acc = Expr::new(ExprKind::Call(Box::new(callee), vec![t]), at);
    for (s, p, g, sp) in segs.into_iter().rev() {
        let none = Expr::new(ExprKind::Name("none".into()), at);
        let arms = vec![Arm { pat: p, guard: g, body: acc }, Arm { pat: Pat::Wild, guard: None, body: none }];
        acc = Expr::new(ExprKind::Match(Box::new(s), arms, MatchForm::IfIs), sp);
    }
    let it = Pat::Ctor { name: "some".into(), args: CtorArgs::Positional(vec![Pat::Bind(CHAIN_VALUE.into())]) };
    let arms = vec![Arm { pat: it, guard: None, body: Expr::new(ExprKind::Name(CHAIN_VALUE.into()), point(span.end)) }, Arm { pat: Pat::Wild, guard: None, body: other }];
    Expr::new(ExprKind::Match(Box::new(acc), arms, MatchForm::IfChain), span)
}

/// `while e1 is p1 and ... body` where a test binds names: `while (if e1 is p1 and ... then`
/// `(body; true) else false) ()`, so the names are bound afresh each time round. A test that
/// binds nothing stays a plain `Bool` condition. The parts it adds get spans no other
/// expression has, since the checker records types by span.
fn while_is(start: u32, cond: Expr, body: Expr) -> Stmt {
    fn binds(e: &Expr) -> bool {
        let ExprKind::Match(_, arms, MatchForm::Is) = &e.kind else { return false };
        let mut v = Vec::new();
        arms[0].pat.binds(&mut v);
        !v.is_empty() || arms[0].guard.as_ref().is_some_and(binds)
    }
    if !binds(&cond) {
        return Stmt::While(cond, body);
    }
    let cspan = cond.span;
    let ExprKind::Match(s, arms, _) = cond.kind else { unreachable!() };
    let a = arms.into_iter().next().unwrap();
    let end = body.span.end;
    let t = Expr::new(ExprKind::Block(vec![Stmt::Expr(body), Stmt::Expr(Expr::new(ExprKind::Bool(true), point(end)))]), Span { start: cspan.start, end });
    let other = Expr::new(ExprKind::Bool(false), point(cspan.end));
    let c = if_is(*s, a.pat, a.guard, cspan, t, other, Span { start, end });
    Stmt::While(c, Expr::new(ExprKind::Unit, point(start)))
}

/// The body of a `p = e else b` statement until the statements after it are folded in.
fn guard_pending() -> Expr {
    Expr::new(ExprKind::Hole(Some("\0guard".into())), Span::default())
}

/// Moves the statements after each `p = e else b` into its match: `| p => rest | _ => b`.
fn fold_guards(mut stmts: Vec<Stmt>) -> Vec<Stmt> {
    let mut i = stmts.len();
    while i > 0 {
        i -= 1;
        let pending = matches!(&stmts[i], Stmt::Expr(Expr { kind: ExprKind::Match(_, arms, MatchForm::LetElse), .. }) if arms[0].body == guard_pending());
        if !pending {
            continue;
        }
        let rest = stmts.split_off(i + 1);
        let body = match rest.as_slice() {
            [] => Expr::new(ExprKind::Unit, Span::default()),
            [Stmt::Expr(_)] => {
                let Some(Stmt::Expr(e)) = rest.into_iter().next() else { unreachable!() };
                e
            }
            _ => {
                let span = rest.iter().map(stmt_span).reduce(|a, b| a.to(b)).unwrap_or_default();
                Expr::new(ExprKind::Block(rest), span)
            }
        };
        if let Stmt::Expr(Expr { kind: ExprKind::Match(_, arms, _), .. }) = &mut stmts[i] {
            arms[0].body = body;
        }
    }
    stmts
}

fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::Let(_, e) | Stmt::Var(_, e) | Stmt::Expr(e) | Stmt::While(e, _) => e.span,
        Stmt::Assign(_, e, sp) => sp.to(e.span),
        Stmt::For(_, a, b) => a.span.to(b.span),
        Stmt::Fn(f) => f.span,
    }
}

/// An empty span at `at`: where the parser's own nodes go, so no source expression shares it.
fn point(at: u32) -> Span {
    Span::new(at as usize, at as usize)
}
