//! Decoding JSON into types with `where` refinements: `json.decode[T]`, `r.json[T]` and the body
//! of a svc endpoint served by `http.serve`. The value is decoded into a mirror of `T` without the
//! refinements, each clause is checked, and only then is the `T` built, so a value that breaks a
//! clause is an `err` naming the field and the clause instead of a trap. Everything becomes
//! SSPUR, so both tiers run it.
use super::elab_net::Gen;
use super::*;

const HELPERS: &str = "type SspurJsonErr = SspurJsonBad{msg: Str}\n\nfn __jat(at: Str, f: Str) -> Str\n= if at == \"\" then f else at + \".\" + f\n\nfn __jmsg(at: Str, p: Str) -> Str\n= if at == \"\" then \"expected a value where \" + p else at + \": must satisfy \" + p";

fn lit(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '{' => o.push_str("\\{"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// The clause with its `_` replaced by `x`. The program is printed and parsed again, so `x` may
/// be any expression text, such as `x.title`.
pub(crate) fn clause(pred: &Expr, x: &str) -> String {
    let mut e = pred.clone();
    visit::walk_expr_mut(&mut e, &mut |y| {
        if matches!(y.kind, ExprKind::Placeholder) {
            y.kind = ExprKind::Name(x.to_string());
        }
    });
    printer::expr(&e, 0)
}

fn ident(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

impl Gen<'_> {
    fn tdef(&self, name: &str) -> Option<&TypeDef> {
        self.m.defs.iter().find_map(|d| match d {
            Def::Type(t) if t.name == name => Some(t),
            _ => None,
        })
    }

    /// Whether decoding `ty` has refinements to check.
    pub(crate) fn needs_check(&self, ty: &Ty) -> bool {
        self.needs_in(ty, &mut vec![])
    }

    fn needs_in(&self, ty: &Ty, seen: &mut Vec<String>) -> bool {
        match ty {
            Ty::Tuple(xs) => xs.iter().any(|x| self.needs_in(x, seen)),
            Ty::Fn { .. } => false,
            Ty::Named { name, args, .. } => {
                if args.iter().any(|a| self.needs_in(a, seen)) {
                    return true;
                }
                if seen.contains(name) {
                    return false;
                }
                let Some(t) = self.tdef(name) else { return false };
                seen.push(name.clone());
                match &t.body {
                    TypeBody::Alias(target, r) => r.is_some() || self.needs_in(target, seen),
                    TypeBody::New(inner) => self.needs_in(inner, seen),
                    TypeBody::Record(fs) => fs.iter().any(|f| f.refine.is_some() || self.needs_in(&f.ty, seen)),
                    TypeBody::Sum(vs) => vs.iter().flat_map(|v| v.fields.iter().flatten()).any(|f| f.refine.is_some() || self.needs_in(&f.ty, seen)),
                }
            }
        }
    }

    fn unsupported(ty: &Ty, what: &str) -> String {
        format!("E_JSON json.decode cannot check the refinements of {}: {what}; decode a type without 'where' and validate it", printer::ty(ty))
    }

    /// The mirror of `ty` without refinements, as type text.
    fn jraw(&mut self, ty: &Ty) -> Result<String, String> {
        if !self.needs_check(ty) {
            return Ok(printer::ty(ty));
        }
        match ty {
            Ty::Named { name, args, .. } if matches!(name.as_str(), "List" | "Opt") && args.len() == 1 => Ok(format!("{name}[{}]", self.jraw(&args[0])?)),
            Ty::Named { name, args, .. } => {
                let Some(t) = self.tdef(name).cloned() else { return Err(Self::unsupported(ty, "refinements inside this type")) };
                if !args.is_empty() || !t.params.is_empty() {
                    return Err(Self::unsupported(ty, "generic types are not supported"));
                }
                match &t.body {
                    TypeBody::Alias(target, _) => self.jraw(target),
                    TypeBody::New(inner) => self.jraw(inner),
                    TypeBody::Record(fs) => {
                        self.jdefine(name, fs)?;
                        Ok(format!("SspurJ{name}"))
                    }
                    TypeBody::Sum(_) => Err(Self::unsupported(ty, "sum types are not supported")),
                }
            }
            _ => Err(Self::unsupported(ty, "tuples and maps are not supported")),
        }
    }

    /// The expression that turns `x`, a value of `raw(ty)`, into a `ty`, raising `SspurJsonBad`
    /// for a clause that doesn't hold. `at` is a `Str` expression for where `x` is in the input.
    fn jconv(&mut self, ty: &Ty, x: &str, at: &str, depth: usize) -> Result<String, String> {
        if !self.needs_check(ty) {
            return Ok(x.to_string());
        }
        let Ty::Named { name, args, .. } = ty else { return Err(Self::unsupported(ty, "tuples are not supported")) };
        match name.as_str() {
            "List" => {
                let p = format!("__p{depth}");
                let inner = self.jconv(&args[0], &format!("{p}.1"), &format!("{at} + \"[\" + {p}.0.str + \"]\""), depth + 1)?;
                return Ok(format!("{x}.enumerate.map({p} => {inner})"));
            }
            "Opt" => {
                let v = format!("__v{depth}");
                let inner = self.jconv(&args[0], &v, at, depth + 1)?;
                return Ok(format!("{x}.map({v} => {inner})"));
            }
            _ => {}
        }
        let Some(t) = self.tdef(name).cloned() else { return Err(Self::unsupported(ty, "refinements inside this type")) };
        match &t.body {
            TypeBody::Alias(target, None) => self.jconv(target, x, at, depth),
            TypeBody::Alias(target, Some(pred)) => {
                let f = format!("sspur_ja_{name}");
                if self.done.insert(f.clone()) {
                    let r = self.jraw(target)?;
                    let inner = self.jconv(target, "v", "at", 1)?;
                    let check = format!("if not ({}) then raise SspurJsonBad{{msg: __jmsg(at, {})}}", clause(pred, "v"), lit(&printer::expr(pred, 0)));
                    self.extra.push(format!("fn {f}(v: {r}, at: Str) -> {name} ! fail[SspurJsonErr]\n= do\n  {check}\n  {inner}"));
                }
                Ok(format!("{f}({x}, {at})"))
            }
            TypeBody::New(inner) => Ok(format!("{name}({})", self.jconv(&inner.clone(), x, at, depth)?)),
            TypeBody::Record(_) => {
                self.jraw(ty)?;
                Ok(format!("sspur_jv_{name}({x}, {at})"))
            }
            TypeBody::Sum(_) => Err(Self::unsupported(ty, "sum types are not supported")),
        }
    }

    /// The mirror record `SspurJ<name>` and `sspur_jv_<name>`, which checks it and builds the real one.
    fn jdefine(&mut self, name: &str, fs: &[Field]) -> Result<(), String> {
        let f = format!("sspur_jv_{name}");
        if !self.done.insert(f.clone()) {
            return Ok(());
        }
        let mut raw = Vec::new();
        let mut checks = String::new();
        let mut inits = Vec::new();
        for fd in fs {
            raw.push(format!("{}: {}", fd.name, self.jraw(&fd.ty)?));
            let at = format!("__jat(at, {})", lit(&fd.name));
            if let Some(p) = &fd.refine {
                writeln!(checks, "  if not ({}) then raise SspurJsonBad{{msg: __jmsg({at}, {})}}", clause(p, &format!("x.{}", fd.name)), lit(&printer::expr(p, 0))).unwrap();
            }
            inits.push(format!("{}: {}", fd.name, self.jconv(&fd.ty, &format!("x.{}", fd.name), &at, 1)?));
        }
        self.extra.push(format!("type SspurJ{name} = {{{}}}", raw.join(", ")));
        self.extra.push(format!("fn {f}(x: SspurJ{name}, at: Str) -> {name} ! fail[SspurJsonErr]\n= do\n{checks}  {name}{{{}}}", inits.join(", ")));
        Ok(())
    }

    /// `__jdec_<T>(s: Str) -> Res[T, Str]`, `json.decode[T]` with the refinements checked.
    pub(crate) fn decoder(&mut self, ty: &Ty) -> Result<String, String> {
        let ts = printer::ty(ty);
        let f = format!("__jdec_{}", ident(&ts));
        if !self.done.insert(f.clone()) {
            return Ok(f);
        }
        if self.done.insert("__jhelpers".into()) {
            self.extra.push(HELPERS.to_string());
        }
        let r = self.jraw(ty)?;
        let c = self.jconv(ty, "x", "\"\"", 1)?;
        self.extra.push(format!("fn {f}_v(x: {r}) -> Res[{ts}, Str]\n= catch ok({c})\n  | SspurJsonBad{{msg}} => err(msg)"));
        self.extra.push(format!("fn {f}(s: Str) -> Res[{ts}, Str]\n= match json.decode[{r}](s)\n  | ok(x) => {f}_v(x)\n  | err(e) => err(e)"));
        Ok(f)
    }
}

impl Gen<'_> {
    /// For `config.load`: a line per `where` clause of a field of `tn`, on the value bound to
    /// `__c_<field>`, that makes the load an `err` naming the field instead of a trap.
    pub(crate) fn config_checks(&self, tn: &str, fields: &[String]) -> String {
        let Some(TypeDef { body: TypeBody::Record(fs), .. }) = self.tdef(tn) else { return String::new() };
        let mut out = String::new();
        for f in fields {
            let Some(fd) = fs.iter().find(|x| x.name == *f) else { continue };
            let alias = match &fd.ty {
                Ty::Named { name, .. } => match self.tdef(name).map(|t| &t.body) {
                    Some(TypeBody::Alias(_, Some(p))) => Some(p),
                    _ => None,
                },
                _ => None,
            };
            for p in fd.refine.iter().chain(alias) {
                writeln!(out, "  if not ({}) then raise SspurBadConfig{{msg: {}}}", clause(p, &format!("__c_{f}")), lit(&format!("{f}: must satisfy {}", printer::expr(p, 0)))).unwrap();
            }
        }
        out
    }
}

use std::fmt::Write as _;
