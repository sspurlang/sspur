use sspur_syntax::*;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Default)]
pub struct Resolution<'a> {
    pub user_methods: Option<&'a HashSet<(u32, u32)>>,
    pub record_types: Option<&'a HashMap<(u32, u32), String>>,
}

pub fn base32(bytes: &[u8]) -> String {
    const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for b in bytes {
        buf = (buf << 8) | u32::from(*b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHA[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHA[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn hash_module(m: &Module) -> Vec<(String, String)> {
    hash_module_with(m, &Resolution::default())
}

pub fn hash_module_with(m: &Module, res: &Resolution) -> Vec<(String, String)> {
    let hashes = Hasher::new(m, res).run();
    m.defs.iter().map(|d| (d.name().to_string(), base32(&hashes[d.name()]))).collect()
}

pub fn dependencies(m: &Module, res: &Resolution) -> BTreeMap<String, Vec<String>> {
    let h = Hasher::new(m, res);
    m.defs.iter().map(|d| (d.name().to_string(), h.deps(d).into_iter().filter(|n| n != d.name()).collect())).collect()
}

pub fn root_hash(entries: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = entries.iter().collect();
    sorted.sort();
    let mut h = blake3::Hasher::new();
    for (n, x) in sorted {
        h.update(n.as_bytes());
        h.update(b"\0");
        h.update(x.as_bytes());
        h.update(b"\n");
    }
    base32(h.finalize().as_bytes())
}

struct Hasher<'a> {
    defs: BTreeMap<String, &'a Def>,
    ctor_owner: HashMap<String, (String, usize)>,
    op_owner: HashMap<String, (String, usize)>,
    res: &'a Resolution<'a>,
    done: HashMap<String, [u8; 32]>,
    trait_uses: HashMap<String, Vec<String>>,
}

#[derive(Default)]
struct Enc {
    buf: Vec<u8>,
    locals: Vec<String>,
    tparams: Vec<String>,
}

impl Enc {
    fn tag(&mut self, t: u8) {
        self.buf.push(t);
    }

    fn uint(&mut self, mut n: u64) {
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                self.buf.push(b);
                break;
            }
            self.buf.push(b | 0x80);
        }
    }

    fn str(&mut self, s: &str) {
        self.uint(s.len() as u64);
        self.buf.extend_from_slice(s.as_bytes());
    }

    fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
}

impl<'a> Hasher<'a> {
    fn new(m: &'a Module, res: &'a Resolution<'a>) -> Self {
        let mut ctor_owner = HashMap::new();
        for d in &m.defs {
            if let Def::Type(TypeDef { name, body: TypeBody::Sum(vs), .. }) = d {
                for (i, v) in vs.iter().enumerate() {
                    ctor_owner.insert(v.name.clone(), (name.clone(), i));
                }
            }
        }
        let mut op_owner = HashMap::new();
        for d in &m.defs {
            if let Def::Effect(e) = d {
                for (i, op) in e.ops.iter().enumerate() {
                    op_owner.insert(op.name.clone(), (e.name.clone(), i));
                }
            }
        }
        let mut trait_uses: HashMap<String, Vec<String>> = HashMap::new();
        let mut method_trait: HashMap<String, String> = BUILTIN_TRAITS.iter().flat_map(|(t, ms)| ms.iter().map(|m| (m.to_string(), t.to_string()))).collect();
        for d in &m.defs {
            if let Def::Trait(t) = d {
                for x in &t.methods {
                    method_trait.insert(x.sig.name.clone(), t.name.clone());
                    trait_uses.entry(x.sig.name.clone()).or_default().push(t.name.clone());
                }
            }
        }
        let by_trait: HashMap<String, Vec<String>> = method_trait.iter().fold(HashMap::new(), |mut acc, (m, t)| {
            acc.entry(t.clone()).or_default().push(m.clone());
            acc
        });
        for d in &m.defs {
            if let Def::Impl(i) = d {
                let local = i.trait_name.rsplit("__").next().unwrap_or(&i.trait_name);
                for m in by_trait.get(&i.trait_name).or_else(|| by_trait.get(local)).into_iter().flatten() {
                    trait_uses.entry(m.clone()).or_default().push(i.key.clone());
                }
                trait_uses.entry(format!("#{local}")).or_default().push(i.key.clone());
            }
        }
        Hasher { defs: m.defs.iter().map(|d| (d.name().to_string(), d)).collect(), ctor_owner, op_owner, res, done: HashMap::new(), trait_uses }
    }

    /// Traits and impls a definition can reach through method calls and operators.
    fn trait_deps(&self, d: &Def) -> Vec<String> {
        if self.trait_uses.is_empty() {
            return vec![];
        }
        let mut exprs: Vec<&Expr> = Vec::new();
        match d {
            Def::Fn(f) => {
                exprs.extend(f.pres.iter().chain(f.posts.iter()).chain(f.examples.iter()));
                exprs.push(&f.body);
            }
            Def::Test(t) => exprs.push(&t.body),
            Def::Static(s) => exprs.push(&s.init),
            Def::Trait(t) => exprs.extend(t.methods.iter().map(|m| &m.sig.body)),
            Def::Impl(i) => exprs.extend(i.fns.iter().map(|f| &f.body)),
            _ => {}
        }
        let mut out: HashSet<String> = HashSet::new();
        let mut add = |k: &str| {
            for n in self.trait_uses.get(k).into_iter().flatten() {
                out.insert(n.clone());
            }
        };
        for e in exprs {
            visit::walk_expr(e, &mut |x| {
                match &x.kind {
                    ExprKind::Name(n) | ExprKind::Method { name: n, .. } | ExprKind::Field(_, n) => add(n),
                    ExprKind::Binary(op, ..) => {
                        if let Some(t) = op_trait(*op) {
                            add(&format!("#{t}"));
                        }
                    }
                    ExprKind::Unary(UnOp::Neg, _) => add("#Neg"),
                    ExprKind::Index(..) => add("#Index"),
                    _ => {}
                }
                true
            });
        }
        let mut v: Vec<String> = out.into_iter().filter(|n| n != d.name()).collect();
        v.sort();
        v
    }

    fn deps(&self, d: &Def) -> Vec<String> {
        let mut out = HashSet::new();
        let mut add = |n: &str| {
            if self.defs.contains_key(n) {
                out.insert(n.to_string());
            } else if let Some((t, _)) = self.ctor_owner.get(n).or_else(|| self.op_owner.get(n)) {
                out.insert(t.clone());
            }
        };
        let mut tys: Vec<&Ty> = Vec::new();
        let mut exprs: Vec<&Expr> = Vec::new();
        match d {
            Def::Type(t) => {
                match &t.body {
                    TypeBody::Record(fs) => collect_fields(fs, &mut tys, &mut exprs),
                    TypeBody::Sum(vs) => vs.iter().filter_map(|v| v.fields.as_ref()).for_each(|fs| collect_fields(fs, &mut tys, &mut exprs)),
                    TypeBody::Alias(ty, r) => {
                        tys.push(ty);
                        exprs.extend(r.iter());
                    }
                    TypeBody::New(ty) => tys.push(ty),
                }
                if let Some(d) = &t.drop {
                    add(d);
                }
            }
            Def::Fn(f) => {
                for p in &f.params {
                    tys.push(&p.ty);
                    exprs.extend(p.refine.iter());
                }
                tys.extend(f.ret.iter());
                for e in &f.effects {
                    add(&e.name);
                    tys.extend(e.args.iter());
                }
                exprs.extend(f.pres.iter().chain(f.posts.iter()).chain(f.examples.iter()));
                exprs.push(&f.body);
            }
            Def::Test(t) => exprs.push(&t.body),
            Def::Store(st) => tys.extend([&st.key, &st.val]),
            Def::Static(s) => {
                tys.push(&s.ty);
                exprs.push(&s.init);
            }
            Def::Svc(sv) => {
                tys.extend(sv.params.iter().map(|p| &p.ty));
                sv.eps.iter().for_each(|e| add(&e.handler));
            }
            Def::Effect(e) => {
                for op in &e.ops {
                    tys.extend(op.params.iter().map(|p| &p.ty));
                    tys.extend(op.ret.iter());
                }
            }
            Def::Use(_) => {}
            Def::Trait(t) => {
                for m in &t.methods {
                    for p in &m.sig.params {
                        tys.push(&p.ty);
                    }
                    tys.extend(m.sig.ret.iter());
                    exprs.push(&m.sig.body);
                }
            }
            Def::Impl(i) => {
                add(&i.trait_name);
                tys.push(&i.target);
                tys.extend(i.trait_args.iter());
                for f in &i.fns {
                    for p in &f.params {
                        tys.push(&p.ty);
                    }
                    tys.extend(f.ret.iter());
                    exprs.extend(f.pres.iter().chain(f.posts.iter()));
                    exprs.push(&f.body);
                }
            }
        }
        for d in [d] {
            if let Def::Fn(f) = d {
                for p in &f.tparams {
                    tys.extend(p.bounds.iter());
                }
            }
        }
        while let Some(t) = tys.pop() {
            match t {
                Ty::Named { name, args, .. } => {
                    add(name);
                    tys.extend(args.iter());
                }
                Ty::Tuple(xs) => tys.extend(xs.iter()),
                Ty::Fn { params, ret, effects } => {
                    tys.extend(params.iter());
                    tys.push(ret);
                    effects.iter().for_each(|e| tys.extend(e.args.iter()));
                }
            }
        }
        for e in exprs {
            visit::walk_expr(e, &mut |x| {
                match &x.kind {
                    ExprKind::Name(n) => add(n),
                    ExprKind::Method { name, .. } | ExprKind::Field(_, name) => add(name),
                    ExprKind::Record { ctor: Some(c), .. } => add(c),
                    ExprKind::Record { ctor: None, .. } => {
                        if let Some(n) = self.res.record_types.and_then(|r| r.get(&(x.span.start, x.span.end))) {
                            add(n);
                        }
                    }
                    ExprKind::Match(_, arms, _) | ExprKind::Catch(_, arms) | ExprKind::Handle(_, arms) => arms.iter().for_each(|a| pat_names(&a.pat, &mut add)),
                    ExprKind::Table(rows) => rows.iter().flat_map(|r| r.cells.iter()).for_each(|c| {
                        if let Cell::Pat(p) = c {
                            pat_names(p, &mut add)
                        }
                    }),
                    ExprKind::Block(stmts) => {
                        for s in stmts {
                            if let Stmt::Let(p, _) | Stmt::For(p, _, _) = s {
                                pat_names(p, &mut add);
                            }
                        }
                    }
                    _ => {}
                }
                true
            });
        }
        out.extend(self.trait_deps(d));
        let mut v: Vec<String> = out.into_iter().collect();
        v.sort();
        v
    }

    fn run(mut self) -> HashMap<String, [u8; 32]> {
        let names: Vec<String> = self.defs.keys().cloned().collect();
        let graph: HashMap<String, Vec<String>> = names.iter().map(|n| (n.clone(), self.deps(self.defs[n]))).collect();
        for scc in tarjan(&names, &graph) {
            self.hash_scc(&scc);
        }
        self.done
    }

    fn hash_scc(&mut self, scc: &[String]) {
        let group: HashMap<String, usize> = scc.iter().map(|n| (n.clone(), 0)).collect();
        let mut shaped: Vec<([u8; 32], String)> = scc.iter().map(|n| (*blake3::hash(&self.encode(self.defs[n], &group)).as_bytes(), n.clone())).collect();
        shaped.sort();
        let order: HashMap<String, usize> = shaped.iter().enumerate().map(|(i, (_, n))| (n.clone(), i)).collect();
        if scc.len() == 1 {
            let n = &scc[0];
            let h = blake3::hash(&self.encode(self.defs[n], &order));
            self.done.insert(n.clone(), *h.as_bytes());
            return;
        }
        let mut all = Vec::new();
        for (_, n) in &shaped {
            all.extend(self.encode(self.defs[n], &order));
        }
        let group_hash = blake3::hash(&all);
        for (n, i) in order {
            let mut h = blake3::Hasher::new();
            h.update(group_hash.as_bytes());
            h.update(&(i as u64).to_le_bytes());
            self.done.insert(n, *h.finalize().as_bytes());
        }
    }

    fn global_ref(&self, enc: &mut Enc, name: &str, group: &HashMap<String, usize>) -> bool {
        if let Some(i) = group.get(name) {
            enc.tag(b'S');
            enc.uint(*i as u64);
            return true;
        }
        if let Some(h) = self.done.get(name) {
            enc.tag(b'R');
            enc.bytes(h);
            return true;
        }
        false
    }

    fn ctor_ref(&self, enc: &mut Enc, name: &str, group: &HashMap<String, usize>) -> bool {
        let Some((owner, idx)) = self.ctor_owner.get(name) else { return false };
        enc.tag(b'C');
        self.global_ref(enc, owner, group);
        enc.uint(*idx as u64);
        true
    }

    fn op_ref(&self, enc: &mut Enc, name: &str, group: &HashMap<String, usize>) -> bool {
        let Some((owner, idx)) = self.op_owner.get(name) else { return false };
        enc.tag(b'O');
        if !self.global_ref(enc, owner, group) {
            enc.str(owner);
        }
        enc.uint(*idx as u64);
        true
    }

    fn encode(&self, d: &Def, group: &HashMap<String, usize>) -> Vec<u8> {
        let mut enc = Enc::default();
        match d {
            Def::Use(u) => {
                enc.tag(b'U');
                enc.str(&u.pkg);
                enc.uint(u.names.len() as u64);
                u.names.iter().for_each(|n| enc.str(n));
            }
            Def::Type(t) => {
                enc.tag(b'T');
                enc.tparams = t.params.iter().map(|p| p.name.clone()).collect();
                enc.uint(t.params.len() as u64);
                match &t.body {
                    TypeBody::Record(fs) => {
                        enc.tag(b'r');
                        self.fields(&mut enc, fs, group);
                    }
                    TypeBody::Sum(vs) => {
                        enc.tag(b's');
                        enc.uint(vs.len() as u64);
                        for v in vs {
                            match &v.fields {
                                Some(fs) => {
                                    enc.tag(1);
                                    self.fields(&mut enc, fs, group);
                                }
                                None => enc.tag(0),
                            }
                        }
                    }
                    TypeBody::Alias(ty, r) => {
                        enc.tag(b'a');
                        self.ty(&mut enc, ty, group);
                        self.opt_refine(&mut enc, r.as_ref(), group);
                    }
                    TypeBody::New(ty) => {
                        enc.tag(b'n');
                        self.ty(&mut enc, ty, group);
                    }
                }
                let mut derives = t.derives.clone();
                derives.sort();
                enc.uint(derives.len() as u64);
                derives.iter().for_each(|x| enc.str(x));
                if t.res {
                    enc.tag(b'R');
                    match &t.drop {
                        Some(d) => self.expr(&mut enc, &Expr::new(ExprKind::Name(d.clone()), Span::default()), group),
                        None => enc.tag(0),
                    }
                }
            }
            Def::Fn(f) => {
                enc.tag(b'F');
                enc.tparams = f.tparams.iter().map(|p| p.name.clone()).collect();
                enc.uint(f.tparams.len() as u64);
                if f.tparams.iter().any(|p| !p.bounds.is_empty()) {
                    self.bounds(&mut enc, &f.tparams, group);
                }
                enc.uint(f.params.len() as u64);
                for p in &f.params {
                    self.ty(&mut enc, &p.ty, group);
                }
                for p in &f.params {
                    self.opt_refine(&mut enc, p.refine.as_ref(), group);
                }
                match &f.ret {
                    Some(t) => self.ty(&mut enc, t, group),
                    None => enc.tag(b'u'),
                }
                let mut effs: Vec<Vec<u8>> = f
                    .effects
                    .iter()
                    .map(|e| {
                        let mut sub = Enc { tparams: enc.tparams.clone(), ..Default::default() };
                        self.effect(&mut sub, e, group);
                        sub.buf
                    })
                    .collect();
                effs.sort();
                enc.uint(effs.len() as u64);
                effs.iter().for_each(|b| enc.bytes(b));
                enc.locals = f.params.iter().map(|p| p.name.clone()).collect();
                enc.uint(f.pres.len() as u64);
                for p in &f.pres {
                    self.expr(&mut enc, p, group);
                }
                enc.uint(f.posts.len() as u64);
                enc.locals.push("r".into());
                for p in &f.posts {
                    self.expr(&mut enc, p, group);
                }
                enc.locals.pop();
                let saved = std::mem::take(&mut enc.locals);
                enc.uint(f.examples.len() as u64);
                for x in &f.examples {
                    self.expr(&mut enc, x, group);
                }
                enc.locals = saved;
                if let Some(r) = &f.trusted {
                    enc.tag(b'U');
                    enc.str(r);
                }
                if let Some(v) = &f.interrupt {
                    enc.tag(b'Q');
                    enc.str(v);
                }
                if let Some(k) = &f.kernel {
                    enc.tag(b'K');
                    self.expr(&mut enc, &k.grid, group);
                    self.expr(&mut enc, &k.group, group);
                    if let Some((h, gh)) = &k.y {
                        enc.tag(b'Y');
                        self.expr(&mut enc, h, group);
                        self.expr(&mut enc, gh, group);
                    }
                }
                self.expr(&mut enc, &f.body, group);
                if let Some(x) = &f.ext {
                    enc.tag(b'C');
                    enc.str(x.lib.as_deref().unwrap_or(""));
                    enc.str(&x.symbol);
                }
            }
            Def::Test(t) => {
                enc.tag(b'X');
                self.expr(&mut enc, &t.body, group);
            }
            Def::Store(st) => {
                enc.tag(b'S');
                enc.str(&st.kind);
                self.ty(&mut enc, &st.key, group);
                self.ty(&mut enc, &st.val, group);
            }
            Def::Static(s) => {
                enc.tag(b'G');
                self.ty(&mut enc, &s.ty, group);
                self.expr(&mut enc, &s.init, group);
            }
            Def::Svc(sv) => {
                enc.tag(b'V');
                if !sv.params.is_empty() {
                    enc.tag(b'(');
                    enc.uint(sv.params.len() as u64);
                    for p in &sv.params {
                        enc.str(&p.name);
                        self.ty(&mut enc, &p.ty, group);
                    }
                }
                enc.uint(sv.eps.len() as u64);
                for e in &sv.eps {
                    enc.str(&e.method);
                    enc.str(&e.path);
                    self.name(&mut enc, &e.handler, group);
                }
            }
            Def::Trait(t) => {
                enc.tag(b'W');
                enc.uint(t.params.len() as u64);
                enc.uint(t.methods.len() as u64);
                for m in &t.methods {
                    enc.str(&m.sig.name);
                    enc.tag(u8::from(m.default));
                    let mut f = m.sig.clone();
                    f.tparams.insert(0, TParam { name: "Self".into(), kind: None, refine: None, bounds: vec![] });
                    f.tparams.splice(1..1, t.params.iter().cloned());
                    enc.bytes(&self.encode(&Def::Fn(f), group));
                }
            }
            Def::Impl(i) => {
                enc.tag(b'I');
                self.name(&mut enc, &i.trait_name, group);
                enc.tparams = i.tparams.iter().map(|p| p.name.clone()).collect();
                self.ty(&mut enc, &i.target, group);
                enc.uint(i.trait_args.len() as u64);
                for a in &i.trait_args {
                    self.ty(&mut enc, a, group);
                }
                self.bounds(&mut enc, &i.tparams, group);
                let mut fns: Vec<&FnDef> = i.fns.iter().collect();
                fns.sort_by(|a, b| a.name.cmp(&b.name));
                enc.uint(fns.len() as u64);
                for f in fns {
                    enc.str(&f.name);
                    let mut g = f.clone();
                    g.tparams.splice(0..0, i.tparams.iter().cloned().chain([TParam { name: "Self".into(), kind: None, refine: None, bounds: vec![] }]));
                    enc.bytes(&self.encode(&Def::Fn(g), group));
                }
            }
            Def::Effect(e) => {
                enc.tag(b'E');
                enc.tparams = e.params.iter().map(|p| p.name.clone()).collect();
                enc.uint(e.params.len() as u64);
                enc.uint(e.ops.len() as u64);
                for op in &e.ops {
                    enc.str(&op.name);
                    enc.uint(op.params.len() as u64);
                    for p in &op.params {
                        self.ty(&mut enc, &p.ty, group);
                    }
                    match &op.ret {
                        Some(t) => self.ty(&mut enc, t, group),
                        None => enc.tag(b'u'),
                    }
                }
            }
        }
        let extra = self.trait_deps(d);
        if !extra.is_empty() {
            enc.tag(b'J');
            for n in &extra {
                if !self.global_ref(&mut enc, n, group) {
                    enc.str(n);
                }
            }
        }
        enc.buf
    }

    fn bounds(&self, enc: &mut Enc, ps: &[TParam], group: &HashMap<String, usize>) {
        enc.tag(b'B');
        for p in ps {
            enc.uint(p.bounds.len() as u64);
            for b in &p.bounds {
                self.ty(enc, b, group);
            }
        }
    }

    fn fields(&self, enc: &mut Enc, fs: &[Field], group: &HashMap<String, usize>) {
        enc.uint(fs.len() as u64);
        for f in fs {
            enc.str(&f.name);
            self.ty(enc, &f.ty, group);
            self.opt_refine(enc, f.refine.as_ref(), group);
        }
    }

    fn opt_refine(&self, enc: &mut Enc, r: Option<&Expr>, group: &HashMap<String, usize>) {
        match r {
            Some(r) => {
                enc.tag(1);
                enc.locals.push("_".into());
                self.expr(enc, r, group);
                enc.locals.pop();
            }
            None => enc.tag(0),
        }
    }

    fn effect(&self, enc: &mut Enc, e: &Effect, group: &HashMap<String, usize>) {
        enc.str(&e.name);
        enc.uint(e.args.len() as u64);
        for a in &e.args {
            self.ty(enc, a, group);
        }
    }

    fn ty(&self, enc: &mut Enc, t: &Ty, group: &HashMap<String, usize>) {
        match t {
            Ty::Named { name, args, .. } => {
                if let Some(i) = enc.tparams.iter().position(|p| p == name) {
                    enc.tag(b'p');
                    enc.uint(i as u64);
                } else if !self.global_ref(enc, name, group) {
                    enc.tag(b'b');
                    enc.str(name);
                }
                enc.uint(args.len() as u64);
                for a in args {
                    self.ty(enc, a, group);
                }
            }
            Ty::Tuple(xs) => {
                enc.tag(b't');
                enc.uint(xs.len() as u64);
                xs.iter().for_each(|x| self.ty(enc, x, group));
            }
            Ty::Fn { params, ret, effects } => {
                enc.tag(b'f');
                enc.uint(params.len() as u64);
                params.iter().for_each(|x| self.ty(enc, x, group));
                self.ty(enc, ret, group);
                let mut names: Vec<String> = effects.iter().map(printer::effect).collect();
                names.sort();
                enc.uint(names.len() as u64);
                names.iter().for_each(|n| enc.str(n));
            }
        }
    }

    fn name(&self, enc: &mut Enc, n: &str, group: &HashMap<String, usize>) {
        if let Some(i) = enc.locals.iter().rev().position(|l| l == n) {
            enc.tag(b'l');
            enc.uint(i as u64);
        } else if self.ctor_ref(enc, n, group) || self.op_ref(enc, n, group) || self.global_ref(enc, n, group) {
        } else {
            enc.tag(b'g');
            enc.str(n);
        }
    }

    fn method_name(&self, enc: &mut Enc, span: Span, name: &str, group: &HashMap<String, usize>) {
        let user = self.res.user_methods.map_or(self.defs.contains_key(name), |u| u.contains(&(span.start, span.end)));
        if !(user && self.global_ref(enc, name, group)) {
            enc.tag(b'g');
            enc.str(name);
        }
    }

    fn expr(&self, enc: &mut Enc, e: &Expr, group: &HashMap<String, usize>) {
        match &e.kind {
            ExprKind::Int(n) => {
                enc.tag(b'i');
                enc.bytes(&n.to_le_bytes());
            }
            ExprKind::Float(x) => {
                enc.tag(b'd');
                enc.bytes(&x.to_bits().to_le_bytes());
            }
            ExprKind::Str(parts) => {
                enc.tag(b'"');
                enc.uint(parts.len() as u64);
                for p in parts {
                    match p {
                        StrPart::Lit(l) => {
                            enc.tag(0);
                            enc.str(l);
                        }
                        StrPart::Expr(x) => {
                            enc.tag(1);
                            self.expr(enc, x, group);
                        }
                    }
                }
            }
            ExprKind::Bool(b) => enc.tag(if *b { b'Y' } else { b'N' }),
            ExprKind::Unit => enc.tag(b'u'),
            ExprKind::Name(n) => self.name(enc, n, group),
            ExprKind::Hole(_) => enc.tag(b'?'),
            ExprKind::Placeholder => self.name(enc, "_", group),
            ExprKind::Field(x, f) => {
                enc.tag(b'.');
                self.expr(enc, x, group);
                self.method_name(enc, e.span, f, group);
            }
            ExprKind::Call(f, args) => {
                enc.tag(b'(');
                self.expr(enc, f, group);
                self.list(enc, args, group);
            }
            ExprKind::Method { recv, name, targs, args } => {
                enc.tag(b'm');
                self.expr(enc, recv, group);
                self.method_name(enc, e.span, name, group);
                enc.uint(targs.len() as u64);
                targs.iter().for_each(|t| self.ty(enc, t, group));
                self.list(enc, args, group);
            }
            ExprKind::Index(a, i) => {
                enc.tag(b'[');
                self.expr(enc, a, group);
                self.expr(enc, i, group);
            }
            ExprKind::Lambda { params, body, .. } => {
                enc.tag(b'\\');
                enc.uint(params.len() as u64);
                let n = enc.locals.len();
                enc.locals.extend(params.iter().cloned());
                self.expr(enc, body, group);
                enc.locals.truncate(n);
            }
            ExprKind::Binary(op, l, r) => {
                enc.tag(b'o');
                enc.str(op.symbol());
                self.expr(enc, l, group);
                self.expr(enc, r, group);
            }
            ExprKind::Unary(op, x) => {
                enc.tag(match op {
                    UnOp::Neg => b'-',
                    UnOp::Not => b'!',
                    UnOp::Ref => b'&',
                    UnOp::RefMut => b'M',
                });
                self.expr(enc, x, group);
            }
            ExprKind::Range(a, b) => {
                enc.tag(b'~');
                self.expr(enc, a, group);
                self.expr(enc, b, group);
            }
            ExprKind::If(c, t, f) => {
                enc.tag(b'I');
                self.expr(enc, c, group);
                self.expr(enc, t, group);
                match f {
                    Some(f) => self.expr(enc, f, group),
                    None => enc.tag(b'u'),
                }
            }
            ExprKind::Match(s, arms, _) | ExprKind::Catch(s, arms) | ExprKind::Handle(s, arms) => {
                enc.tag(match e.kind {
                    ExprKind::Match(_, _, MatchForm::IfIs) => b'J',
                    ExprKind::Match(_, _, MatchForm::Is) => b'A',
                    ExprKind::Match(_, _, MatchForm::IfChain) => b'Z',
                    ExprKind::Match(_, _, MatchForm::LetElse) => b'D',
                    ExprKind::Match(..) => b'M',
                    ExprKind::Catch(..) => b'K',
                    _ => b'H',
                });
                self.expr(enc, s, group);
                enc.uint(arms.len() as u64);
                for a in arms {
                    let n = enc.locals.len();
                    self.pat(enc, &a.pat, group);
                    match &a.guard {
                        Some(g) => {
                            enc.tag(1);
                            self.expr(enc, g, group);
                        }
                        None => enc.tag(0),
                    }
                    self.expr(enc, &a.body, group);
                    enc.locals.truncate(n);
                }
            }
            ExprKind::Block(stmts) => {
                enc.tag(b'B');
                enc.uint(stmts.len() as u64);
                let n = enc.locals.len();
                for s in stmts {
                    if let Stmt::Fn(f) = s {
                        enc.locals.push(f.name.clone());
                    }
                }
                for s in stmts {
                    self.stmt(enc, s, group);
                }
                enc.locals.truncate(n);
            }
            ExprKind::Record { ctor, fields } => {
                enc.tag(b'{');
                let owner = ctor.clone().or_else(|| self.res.record_types.and_then(|r| r.get(&(e.span.start, e.span.end)).cloned()));
                match owner {
                    Some(c) => {
                        if !(self.ctor_ref(enc, &c, group) || self.global_ref(enc, &c, group)) {
                            enc.tag(b'g');
                            enc.str(&c);
                        }
                    }
                    None => enc.tag(0),
                }
                let mut sorted: Vec<&(String, Expr)> = fields.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                enc.uint(sorted.len() as u64);
                for (n, v) in sorted {
                    enc.str(n);
                    self.expr(enc, v, group);
                }
            }
            ExprKind::List(xs) => {
                enc.tag(b'L');
                self.list(enc, xs, group);
            }
            ExprKind::Tuple(xs) => {
                enc.tag(b'(');
                enc.tag(b't');
                self.list(enc, xs, group);
            }
            ExprKind::Par(xs) => {
                enc.tag(b'P');
                self.list(enc, xs, group);
            }
            ExprKind::Raise(x) => {
                enc.tag(b'^');
                self.expr(enc, x, group);
            }
            ExprKind::Return(x) => {
                enc.tag(b'<');
                self.expr(enc, x, group);
            }
            ExprKind::Table(rows) => {
                enc.tag(b'T');
                enc.uint(rows.len() as u64);
                for r in rows {
                    let n = enc.locals.len();
                    enc.uint(r.cells.len() as u64);
                    for c in &r.cells {
                        match c {
                            Cell::Any => enc.tag(b'_'),
                            Cell::Pat(p) => {
                                enc.tag(b'p');
                                self.pat(enc, p, group);
                            }
                            Cell::Cond(e) => {
                                enc.tag(b'c');
                                self.expr(enc, e, group);
                            }
                        }
                    }
                    self.expr(enc, &r.out, group);
                    enc.locals.truncate(n);
                }
            }
            ExprKind::With(base, ups) => {
                enc.tag(b'W');
                self.expr(enc, base, group);
                enc.uint(ups.len() as u64);
                for (path, v) in ups {
                    enc.uint(path.len() as u64);
                    for seg in path {
                        match seg {
                            PathSeg::Field(f) => {
                                enc.tag(b'f');
                                enc.str(f);
                            }
                            PathSeg::Index(i) => {
                                enc.tag(b'x');
                                self.expr(enc, i, group);
                            }
                        }
                    }
                    self.expr(enc, v, group);
                }
            }
        }
    }

    fn list(&self, enc: &mut Enc, xs: &[Expr], group: &HashMap<String, usize>) {
        enc.uint(xs.len() as u64);
        xs.iter().for_each(|x| self.expr(enc, x, group));
    }

    fn stmt(&self, enc: &mut Enc, s: &Stmt, group: &HashMap<String, usize>) {
        match s {
            Stmt::Expr(e) => {
                enc.tag(b'e');
                self.expr(enc, e, group);
            }
            Stmt::Let(p, e) => {
                enc.tag(b'=');
                self.expr(enc, e, group);
                self.pat(enc, p, group);
            }
            Stmt::Var(n, e) => {
                enc.tag(b'v');
                self.expr(enc, e, group);
                enc.locals.push(n.clone());
            }
            Stmt::Assign(n, e, _) => {
                enc.tag(b':');
                self.name(enc, n, group);
                self.expr(enc, e, group);
            }
            Stmt::While(c, body) => {
                enc.tag(b'w');
                self.expr(enc, c, group);
                self.expr(enc, body, group);
            }
            Stmt::Fn(f) => {
                enc.tag(b'n');
                enc.uint(f.params.len() as u64);
                for p in &f.params {
                    self.ty(enc, &p.ty, group);
                }
                match &f.ret {
                    Some(t) => self.ty(enc, t, group),
                    None => enc.tag(b'u'),
                }
                let mut effs: Vec<String> = f.effects.iter().map(printer::effect).collect();
                effs.sort();
                enc.uint(effs.len() as u64);
                effs.iter().for_each(|e| enc.str(e));
                let n = enc.locals.len();
                enc.locals.extend(f.params.iter().map(|p| p.name.clone()));
                for p in &f.params {
                    self.opt_refine(enc, p.refine.as_ref(), group);
                }
                enc.uint(f.pres.len() as u64);
                f.pres.iter().for_each(|x| self.expr(enc, x, group));
                enc.locals.push("r".into());
                enc.uint(f.posts.len() as u64);
                f.posts.iter().for_each(|x| self.expr(enc, x, group));
                enc.locals.pop();
                self.expr(enc, &f.body, group);
                enc.locals.truncate(n);
            }
            Stmt::For(p, it, body) => {
                enc.tag(b'4');
                self.expr(enc, it, group);
                let n = enc.locals.len();
                self.pat(enc, p, group);
                self.expr(enc, body, group);
                enc.locals.truncate(n);
            }
        }
    }

    fn pat(&self, enc: &mut Enc, p: &Pat, group: &HashMap<String, usize>) {
        self.pat_in(enc, p, group, false)
    }

    /// `again`: the names were bound by an earlier alternative of an or-pattern, so a
    /// binding is encoded as a reference to that local.
    fn pat_in(&self, enc: &mut Enc, p: &Pat, group: &HashMap<String, usize>, again: bool) {
        let sub = |enc: &mut Enc, x: &Pat| self.pat_in(enc, x, group, again);
        match p {
            Pat::Wild => enc.tag(b'_'),
            Pat::Bind(n) if again => {
                enc.tag(b'X');
                enc.uint(enc.locals.iter().rev().position(|l| l == n).unwrap_or(usize::MAX) as u64);
            }
            Pat::Bind(n) => {
                enc.tag(b'x');
                enc.locals.push(n.clone());
            }
            Pat::Or(alts) => {
                enc.tag(b'|');
                enc.uint(alts.len() as u64);
                for (i, a) in alts.iter().enumerate() {
                    self.pat_in(enc, a, group, again || i > 0);
                }
            }
            Pat::List { head, rest, tail } => {
                enc.tag(b'[');
                enc.uint(head.len() as u64);
                head.iter().for_each(|x| sub(enc, x));
                match rest {
                    None => enc.tag(0),
                    Some(r) => {
                        enc.tag(if r.is_some() { 2 } else { 1 });
                        if let Some(r) = r {
                            sub(enc, &Pat::Bind(r.clone()));
                        }
                        enc.uint(tail.len() as u64);
                        tail.iter().for_each(|x| sub(enc, x));
                    }
                }
            }
            Pat::Int(n) => {
                enc.tag(b'i');
                enc.bytes(&n.to_le_bytes());
            }
            Pat::Str(s) => {
                enc.tag(b'"');
                enc.str(s);
            }
            Pat::Bool(b) => enc.tag(if *b { b'Y' } else { b'N' }),
            Pat::Tuple(xs) => {
                enc.tag(b't');
                enc.uint(xs.len() as u64);
                xs.iter().for_each(|x| sub(enc, x));
            }
            Pat::Ctor { name, args } => {
                enc.tag(b'c');
                if !(self.ctor_ref(enc, name, group) || self.op_ref(enc, name, group) || self.global_ref(enc, name, group)) {
                    enc.tag(b'g');
                    enc.str(name);
                }
                match args {
                    CtorArgs::None => enc.tag(0),
                    CtorArgs::Positional(xs) => {
                        enc.tag(1);
                        enc.uint(xs.len() as u64);
                        xs.iter().for_each(|x| sub(enc, x));
                    }
                    CtorArgs::Record(fs) => {
                        enc.tag(2);
                        let mut sorted: Vec<&(String, Pat)> = fs.iter().collect();
                        sorted.sort_by(|a, b| a.0.cmp(&b.0));
                        enc.uint(sorted.len() as u64);
                        for (f, sp) in sorted {
                            enc.str(f);
                            sub(enc, sp);
                        }
                    }
                }
            }
        }
    }
}

fn collect_fields<'a>(fs: &'a [Field], tys: &mut Vec<&'a Ty>, exprs: &mut Vec<&'a Expr>) {
    for f in fs {
        tys.push(&f.ty);
        exprs.extend(f.refine.iter());
    }
}

fn pat_names(p: &Pat, add: &mut impl FnMut(&str)) {
    match p {
        Pat::Ctor { name, args } => {
            add(name);
            match args {
                CtorArgs::Positional(xs) => xs.iter().for_each(|x| pat_names(x, add)),
                CtorArgs::Record(fs) => fs.iter().for_each(|(_, x)| pat_names(x, add)),
                CtorArgs::None => {}
            }
        }
        Pat::Tuple(xs) | Pat::Or(xs) => xs.iter().for_each(|x| pat_names(x, add)),
        Pat::List { head, tail, .. } => head.iter().chain(tail).for_each(|x| pat_names(x, add)),
        _ => {}
    }
}

fn tarjan(names: &[String], graph: &HashMap<String, Vec<String>>) -> Vec<Vec<String>> {
    struct St<'g> {
        graph: &'g HashMap<String, Vec<String>>,
        index: HashMap<String, usize>,
        low: HashMap<String, usize>,
        on: HashSet<String>,
        stack: Vec<String>,
        next: usize,
        out: Vec<Vec<String>>,
    }
    fn go(s: &mut St, v: &str) {
        s.index.insert(v.into(), s.next);
        s.low.insert(v.into(), s.next);
        s.next += 1;
        s.stack.push(v.into());
        s.on.insert(v.into());
        for w in s.graph.get(v).cloned().unwrap_or_default() {
            if !s.index.contains_key(&w) {
                go(s, &w);
                let lw = s.low[&w];
                let lv = s.low.get_mut(v).unwrap();
                *lv = (*lv).min(lw);
            } else if s.on.contains(&w) {
                let iw = s.index[&w];
                let lv = s.low.get_mut(v).unwrap();
                *lv = (*lv).min(iw);
            }
        }
        if s.low[v] == s.index[v] {
            let mut comp = Vec::new();
            loop {
                let w = s.stack.pop().unwrap();
                s.on.remove(&w);
                comp.push(w.clone());
                if w == v {
                    break;
                }
            }
            comp.sort();
            s.out.push(comp);
        }
    }
    let mut s = St { graph, index: HashMap::new(), low: HashMap::new(), on: HashSet::new(), stack: vec![], next: 0, out: vec![] };
    for n in names {
        if !s.index.contains_key(n) {
            go(&mut s, n);
        }
    }
    s.out
}
