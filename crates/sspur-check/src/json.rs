use super::*;

impl Checker {
    pub(crate) fn is_json_recv(&self, recv: &Expr) -> bool {
        matches!(&recv.kind, ExprKind::Name(n) if n == "json") && self.lookup("json").is_none() && !self.fns.contains_key("json")
    }

    pub(crate) fn infer_json(&mut self, name: &str, targs: &[Ty], args: &[Expr], span: Span) -> Type {
        match (name, targs.len(), args.len()) {
            ("encode", 0, 1) => {
                let t = self.infer(&args[0], None);
                self.json_site((span.start, span.end), t, args[0].span, false);
                Type::str()
            }
            ("decode", 1, 1) => {
                let t = self.infer(&args[0], Some(&Type::str()));
                self.expect(&Type::str(), &t, args[0].span);
                let target = self.conv_ty(&targs[0]);
                if let Some(a) = self.refined_alias_in(&targs[0]) {
                    #[cfg(target_family = "wasm")]
                    self.push_diag("E_JSON", "error", span, format!("json.decode cannot check the refinement of {a}"), Some("decode a type without 'where' and validate it".into()), vec![]);
                    #[cfg(not(target_family = "wasm"))]
                    let _ = a;
                }
                #[cfg(not(target_family = "wasm"))]
                if self.refined_alias_in(&targs[0]).is_some() || (self.json_bad(&target, true, &mut vec![]).is_some() && self.json_bad_r(&target, true, true, &mut vec![]).is_none()) {
                    self.net_sites.insert((span.start, span.end), NetSite::Decode);
                }
                self.json_site((span.start, span.end), target.clone(), span, true);
                Type::app("Res", vec![target, Type::str()])
            }
            _ => {
                for a in args {
                    self.infer(a, None);
                }
                self.push_diag("E_UNKNOWN_METHOD", "error", span, format!("no json operation '{name}' with these arguments"), Some("json.encode(v) or json.decode[T](s)".into()), vec![]);
                self.fresh()
            }
        }
    }

    pub(crate) fn json_site(&mut self, key: (u32, u32), t: Type, span: Span, decode: bool) {
        if !self.bounds.is_empty() {
            self.json_bounds.push((key, self.bounds.clone()));
        }
        self.json_sites.push((key, t, span, decode));
    }

    pub(crate) fn refined_alias_in(&self, t: &Ty) -> Option<String> {
        match t {
            Ty::Named { name, args, .. } => {
                if matches!(self.types.get(name).map(|i| &i.kind), Some(TypeKind::Alias(_))) && self.refined_names.contains(name) {
                    return Some(name.clone());
                }
                args.iter().find_map(|a| self.refined_alias_in(a))
            }
            Ty::Tuple(xs) => xs.iter().find_map(|x| self.refined_alias_in(x)),
            Ty::Fn { .. } => None,
        }
    }

    pub(crate) fn collect_refined(&mut self, m: &Module) {
        for d in &m.defs {
            let Def::Type(t) = d else { continue };
            let refined_field = |f: &Field| f.refine.is_some() || matches!(&f.ty, Ty::Named { name, .. } if m.defs.iter().any(|d| matches!(d, Def::Type(TypeDef { name: n, body: TypeBody::Alias(_, Some(_)), .. }) if n == name)));
            let r = match &t.body {
                TypeBody::Alias(_, r) => r.is_some(),
                TypeBody::Record(fs) => fs.iter().any(refined_field),
                TypeBody::Sum(vs) => vs.iter().any(|v| v.fields.iter().flatten().any(refined_field)),
                TypeBody::New(_) => false,
            };
            if r {
                self.refined_names.insert(t.name.clone());
            }
        }
    }

    pub(crate) fn json_bad(&self, t: &Type, decode: bool, seen: &mut Vec<String>) -> Option<String> {
        self.json_bad_r(t, decode, false, seen)
    }

    /// `json_bad`, where `refined_ok` accepts types with `where` refinements, which svc bodies
    /// are checked against after decoding.
    pub(crate) fn json_bad_r(&self, t: &Type, decode: bool, refined_ok: bool, seen: &mut Vec<String>) -> Option<String> {
        match t {
            Type::Fn(..) => Some("functions are not data".into()),
            Type::Param(p) if self.bounds.iter().any(|(bp, bt, _)| bp == p && (bt == crate::implicit::DECODE || (!decode && bt == "Json"))) => None,
            Type::Param(p) if decode => Some(format!("type parameter {p} is not concrete")),
            Type::Param(p) => Some(format!("type parameter {p} is not concrete; give it a bound, as in [{p}: Json]")),
            Type::Var(_) => Some("its type is not known; add a type annotation".into()),
            Type::Tuple(xs) => xs.iter().find_map(|x| self.json_bad_r(x, decode, refined_ok, seen)),
            Type::Con(n, a) => {
                if let Some(x) = a.iter().find_map(|x| self.json_bad_r(x, decode, refined_ok, seen)) {
                    return Some(x);
                }
                match n.as_str() {
                    "Int" | "F64" | "Bool" | "Str" | "Unit" | "List" | "Opt" | "Map" | "Pii" | "#Set" | "#Heap" | "#StrBuf" | "#Time" | "#Duration" | "#Bits" | "#Bytes" | "#HashMap" | "#HashSet" | "#BigInt" | "#Dec" => None,
                    "Secret" => Some("secrets cannot be encoded".into()),
                    "#Regex" | "#Zone" | "#File" | "#View" => Some(format!("{} has no JSON form", &n[1..])),
                    "#Locale" if decode => Some("decoding cannot check the locale tag; decode a Str and call locale".into()),
                    "#Ratio" if decode => Some("decoding cannot check that a Ratio is normalized; decode num and den and call ratio_big".into()),
                    _ if seen.contains(n) => None,
                    _ if decode && !refined_ok && self.refined_names.contains(n) => Some(format!("{n} has 'where' refinements that decoding cannot check")),
                    _ => {
                        let Some(info) = self.types.get(n) else { return Some(format!("{n} has no JSON form")) };
                        let map: HashMap<String, Type> = info.params.iter().cloned().zip(a.iter().cloned()).collect();
                        match &info.kind {
                            TypeKind::Record(fs) => {
                                seen.push(n.clone());
                                fs.iter().find_map(|(_, ft)| self.json_bad_r(&subst_params(ft, &map), decode, refined_ok, seen))
                            }
                            TypeKind::New(inner) => self.json_bad_r(inner, decode, refined_ok, seen),
                            TypeKind::Sum(vs) => {
                                seen.push(n.clone());
                                vs.iter().filter_map(|v| self.ctors.get(v).and_then(|c| c.fields.clone())).flatten().find_map(|(_, ft)| self.json_bad_r(&subst_params(&ft, &map), decode, refined_ok, seen))
                            }
                            TypeKind::Alias(_) => Some(format!("{n} has no JSON form")),
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn finish_json(&mut self, m: &Module) -> HashMap<(u32, u32), Type> {
        let _ = m;
        let mut out = HashMap::new();
        let saved = std::mem::take(&mut self.bounds);
        for (key, t, span, decode) in std::mem::take(&mut self.json_sites) {
            self.bounds = self.json_bounds.iter().find(|(k, _)| *k == key).map(|(_, b)| b.clone()).unwrap_or_default();
            let r = self.resolve(&t);
            #[cfg(not(target_family = "wasm"))]
            let refined_ok = matches!(self.net_sites.get(&key), Some(NetSite::Decode | NetSite::Body));
            #[cfg(target_family = "wasm")]
            let refined_ok = false;
            if let Some(why) = self.json_bad_r(&r, decode, refined_ok, &mut vec![]) {
                self.push_diag("E_JSON", "error", span, format!("{r} cannot be {}: {why}", if decode { "decoded from JSON" } else { "encoded as JSON" }), None, vec![]);
            }
            out.insert(key, r);
        }
        self.bounds = saved;
        self.json_bounds.clear();
        out
    }
}
