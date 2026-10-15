use super::*;

impl Checker {
    pub(crate) fn is_json_recv(&self, recv: &Expr) -> bool {
        matches!(&recv.kind, ExprKind::Name(n) if n == "json") && self.lookup("json").is_none() && !self.fns.contains_key("json")
    }

    pub(crate) fn infer_json(&mut self, name: &str, targs: &[Ty], args: &[Expr], span: Span) -> Type {
        match (name, targs.len(), args.len()) {
            ("encode", 0, 1) => {
                let t = self.infer(&args[0], None);
                self.json_sites.push(((span.start, span.end), t, args[0].span, false));
                Type::str()
            }
            ("decode", 1, 1) => {
                let t = self.infer(&args[0], Some(&Type::str()));
                self.expect(&Type::str(), &t, args[0].span);
                if let Some(a) = self.refined_alias_in(&targs[0]) {
                    self.push_diag("E_JSON", "error", span, format!("json.decode cannot check the refinement of {a}"), Some("decode a type without 'where' and validate it".into()), vec![]);
                }
                let target = self.conv_ty(&targs[0]);
                self.json_sites.push(((span.start, span.end), target.clone(), span, true));
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
        match t {
            Type::Fn(..) => Some("functions are not data".into()),
            Type::Param(p) => Some(format!("type parameter {p} is not concrete")),
            Type::Var(_) => Some("its type is not known; add a type annotation".into()),
            Type::Tuple(xs) => xs.iter().find_map(|x| self.json_bad(x, decode, seen)),
            Type::Con(n, a) => {
                if let Some(x) = a.iter().find_map(|x| self.json_bad(x, decode, seen)) {
                    return Some(x);
                }
                match n.as_str() {
                    "Int" | "F64" | "Bool" | "Str" | "Unit" | "List" | "Opt" | "Map" | "Pii" | "#Set" | "#Heap" | "#StrBuf" | "#Time" | "#Duration" | "#Bits" | "#Bytes" | "#HashMap" | "#HashSet" | "#BigInt" | "#Dec" => None,
                    "Secret" => Some("secrets cannot be encoded".into()),
                    "#Regex" | "#Zone" | "#File" | "#View" => Some(format!("{} has no JSON form", &n[1..])),
                    "#Locale" if decode => Some("decoding cannot check the locale tag; decode a Str and call locale".into()),
                    "#Ratio" if decode => Some("decoding cannot check that a Ratio is normalized; decode num and den and call ratio_big".into()),
                    _ if seen.contains(n) => None,
                    _ if decode && self.refined_names.contains(n) => Some(format!("{n} has 'where' refinements that decoding cannot check")),
                    _ => {
                        let Some(info) = self.types.get(n) else { return Some(format!("{n} has no JSON form")) };
                        let map: HashMap<String, Type> = info.params.iter().cloned().zip(a.iter().cloned()).collect();
                        match &info.kind {
                            TypeKind::Record(fs) => {
                                seen.push(n.clone());
                                fs.iter().find_map(|(_, ft)| self.json_bad(&subst_params(ft, &map), decode, seen))
                            }
                            TypeKind::New(inner) => self.json_bad(inner, decode, seen),
                            TypeKind::Sum(vs) => {
                                seen.push(n.clone());
                                vs.iter().filter_map(|v| self.ctors.get(v).and_then(|c| c.fields.clone())).flatten().find_map(|(_, ft)| self.json_bad(&subst_params(&ft, &map), decode, seen))
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
        for (key, t, span, decode) in std::mem::take(&mut self.json_sites) {
            let r = self.resolve(&t);
            if let Some(why) = self.json_bad(&r, decode, &mut vec![]) {
                self.push_diag("E_JSON", "error", span, format!("{r} cannot be {}: {why}", if decode { "decoded from JSON" } else { "encoded as JSON" }), None, vec![]);
            }
            out.insert(key, r);
        }
        out
    }
}
