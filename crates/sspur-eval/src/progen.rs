use std::collections::{BTreeSet, HashMap};

pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for _ in 0..4 {
            r.next();
        }
        r
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum T {
    Int,
    Bool,
    Str,
    List,
    Rec,
    Sum,
    Tree,
    Map,
    Set,
    HMap,
    Clo,
    F64,
    Res,
}

#[derive(Clone)]
struct Var {
    name: String,
    ty: T,
    mutable: bool,
    assignable: bool,
    index_of: Option<String>,
    eff: Vec<String>,
}

#[derive(Clone)]
struct Sig {
    name: String,
    params: Vec<T>,
    ret: T,
    effects: Vec<String>,
}

#[derive(Default)]
struct Cx {
    vars: Vec<Var>,
    eff: BTreeSet<String>,
    ask_ok: bool,
    lambda: bool,
    ret: Option<T>,
    no_risky: bool,
    marks: Vec<usize>,
    /// The value type of each open block, `None` for `Unit`: what a guard's `else` gives.
    rets: Vec<Option<T>>,
    moved: BTreeSet<String>,
    res_loop: HashMap<String, u32>,
    loops: u32,
}

const BOUNDARY: &[&str] = &[
    "9223372036854775807",
    "(-9223372036854775807 - 1)",
    "9223372036854775806",
    "(-9223372036854775807)",
    "4611686018427387904",
    "(-4611686018427387904)",
    "3037000499",
    "3037000500",
    "(-3037000500)",
    "2147483647",
    "(-2147483648)",
    "4294967296",
];

const FLITS: &[&str] = &["0.0", "(-0.0)", "1.5", "0.1", "2.0", "(-3.25)", "1e300", "1e-300", "0.5", "3.0", "(-1e308)", "123456789.125", "4503599627370497.0", "1e16", "0.3", "7.0"];

const STRS: &[&str] = &["\"\"", "\"a\"", "\"ab\"", "\"Hello\"", "\" x y \"", "\"é\"", "\"日本\"", "\"a,b,c\"", "\"42\"", "\"-7\"", "\"zz9\""];

pub struct Gen {
    r: Rng,
    p: String,
    c: String,
    sigs: Vec<Sig>,
    n: usize,
    out: String,
    sys: bool,
}

pub fn program(seed: u64, id: usize) -> String {
    let mut g = Gen { r: Rng::new(seed), p: format!("p{id}"), c: format!("P{id}"), sigs: vec![], n: 0, out: String::new(), sys: false };
    g.module();
    g.out
}

/// Where to break a generated line in two, so the fuzzer covers continued lines (ADR 0032):
/// after the first ` and `, ` or `, ` + `, ` * ` or ` == `, or before a `.method`, outside strings.
fn break_at(s: &str) -> Option<usize> {
    if s.contains('\n') || s.contains("{\"") || s.contains('\\') || s.trim_start().starts_with('|') {
        return None;
    }
    let b = s.as_bytes();
    let mut in_str = false;
    for i in 1..b.len() {
        match b[i] {
            b'"' => in_str = !in_str,
            _ if in_str => {}
            b'.' if (b[i - 1].is_ascii_alphanumeric() || matches!(b[i - 1], b')' | b']' | b'_')) && b.get(i + 1).is_some_and(u8::is_ascii_lowercase) && b[i - 1] != b'.' => {
                let start = s[..i].rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |j| j + 1);
                if !s[start..i].starts_with(|c: char| c.is_ascii_digit()) {
                    return Some(i);
                }
            }
            b' ' => {
                for op in [" and ", " or ", " + ", " * ", " == "] {
                    if s[i..].starts_with(op) {
                        return Some(i + op.len() - 1);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn ty_name(c: &str, t: T) -> String {
    match t {
        T::Int => "Int".into(),
        T::Bool => "Bool".into(),
        T::Str => "Str".into(),
        T::List => "List[Int]".into(),
        T::Rec => format!("{c}R"),
        T::Sum => format!("{c}S"),
        T::Tree => format!("{c}T"),
        T::Map => "Map[Int, Int]".into(),
        T::Set => "Set[Int]".into(),
        T::HMap => "HashMap[Str, Int]".into(),
        T::Clo => "Int -> Int".into(),
        T::F64 => "F64".into(),
        T::Res => format!("{c}Rs"),
    }
}

impl Gen {
    fn fresh(&mut self, base: &str) -> String {
        self.n += 1;
        format!("{base}{}", self.n)
    }

    fn line(&mut self, ind: usize, s: &str) {
        for _ in 0..ind {
            self.out.push_str("  ");
        }
        match self.r.chance(12).then(|| break_at(s)).flatten() {
            Some(k) => {
                let (a, b) = s.split_at(k);
                let pad = "  ".repeat(ind + 1 + self.r.below(2) as usize);
                self.out.push_str(&format!("{}\n{pad}{}", a.trim_end(), b.trim_start()));
            }
            None => self.out.push_str(s),
        }
        self.out.push('\n');
    }

    fn eff_of(&self, cx: &Cx) -> String {
        if cx.eff.is_empty() {
            String::new()
        } else {
            format!(" ! {}", cx.eff.iter().cloned().collect::<Vec<_>>().join(", "))
        }
    }

    fn module(&mut self) {
        let (p, c) = (self.p.clone(), self.c.clone());
        self.sys = self.r.chance(35);
        if self.sys {
            self.out.push_str("profile sys\n");
        }
        let refine = ["_ >= 0", "_ > -100", "_ != 0", "_ >= -1000 and _ <= 1000"][self.r.below(4) as usize];
        let traits = self.r.chance(50);
        let derive = if traits { " derive Eq, Ord, Show, Hash" } else { "" };
        self.out.push_str(&format!("type {c}R = {{a: Int, b: Int where {refine}}}{derive}\n"));
        self.out.push_str(&format!("type {c}S = {c}A{{x: Int}} | {c}B{{x: Int, y: Int}} | {c}C\n"));
        self.out.push_str(&format!("type {c}T = {c}Lf | {c}Nd{{l: {c}T, v: Int, r: {c}T}}\n"));
        self.out.push_str(&format!("type {c}E = {c}Bad{{c: Int}} | {c}Worse\n"));
        self.out.push_str(&format!("type {c}G[T] = {c}Nil | {c}Cons{{h: T, t: {c}G[T]}}\n"));
        self.out.push_str(&format!("type {c}P[A, B] = {{fst: A, snd: B}}\n"));
        if self.sys {
            self.out.push_str(&format!("res type {c}Rs = {{id: Int, v: Int}} drop {p}_rdrop\n"));
        }
        self.out.push_str(&format!("effect {p}ask(x: Int) -> Int\n\n"));
        self.tree_helpers();
        self.extra_helpers();
        if traits {
            self.traits();
        }
        self.risky();
        self.asker();
        let k = 3 + self.r.below(5);
        for i in 0..k {
            match self.r.below(10) {
                0 | 1 => self.recursive(),
                2 => self.tree_rec(),
                _ => self.random_fn(i < 2),
            }
        }
        self.entry();
    }

    fn sig(&mut self, name: &str, params: &[T], ret: T, effects: &[&str]) {
        self.sigs.push(Sig { name: name.into(), params: params.to_vec(), ret, effects: effects.iter().map(|s| s.to_string()).collect() });
    }

    fn tree_helpers(&mut self) {
        let (p, c) = (self.p.clone(), self.c.clone());
        let cmp = ["x < v", "x <= v", "x > v"][self.r.below(3) as usize];
        self.out.push_str(&format!(
            "fn {p}_ins(t: {c}T, x: Int) -> {c}T\n= match t\n  | {c}Lf => {c}Nd{{l: {c}Lf, v: x, r: {c}Lf}}\n  | {c}Nd{{l, v, r}} => if {cmp} then {c}Nd{{l: {p}_ins(l, x), v: v, r: r}} else {c}Nd{{l: l, v: v, r: {p}_ins(r, x)}}\n\n"
        ));
        self.sig(&format!("{p}_ins"), &[T::Tree, T::Int], T::Tree, &[]);
        let upd = ["v + d", "v * d", "v - d", "(v + d) % 1000", "d / (v + 1)"][self.r.below(5) as usize];
        self.out.push_str(&format!(
            "fn {p}_inc(t: {c}T, d: Int) -> {c}T\n= match t\n  | {c}Lf => {c}Lf\n  | {c}Nd{{l, v, r}} => {c}Nd{{l: {p}_inc(l, d), v: {upd}, r: {p}_inc(r, d)}}\n\n"
        ));
        self.sig(&format!("{p}_inc"), &[T::Tree, T::Int], T::Tree, &[]);
        self.out.push_str(&format!("fn {p}_tsum(t: {c}T) -> Int\n= match t\n  | {c}Lf => 0\n  | {c}Nd{{l, v, r}} => {p}_tsum(l) + v + {p}_tsum(r)\n\n"));
        self.sig(&format!("{p}_tsum"), &[T::Tree], T::Int, &[]);
        self.out.push_str(&format!("fn {p}_items(t: {c}T) -> List[Int]\n= match t\n  | {c}Lf => []\n  | {c}Nd{{l, v, r}} => {p}_items(l) + [v] + {p}_items(r)\n\n"));
        self.sig(&format!("{p}_items"), &[T::Tree], T::List, &[]);
        self.out.push_str(&format!("fn {p}_build(xs: List[Int]) -> {c}T\n= do\n  var t = {c}Lf\n  for x in xs\n    t := {p}_ins(t, x)\n  t\n\n"));
        self.sig(&format!("{p}_build"), &[T::List], T::Tree, &[]);
        let kexpr = ["x % 7", "x / 3", "x", "x * x % 11"][self.r.below(4) as usize];
        self.out.push_str(&format!("fn {p}_mkmap(xs: List[Int]) -> Map[Int, Int]\n= xs.fold(empty_map(), (m, x) => m.put({kexpr}, m.get({kexpr}).or(0) + x))\n\n"));
        self.sig(&format!("{p}_mkmap"), &[T::List], T::Map, &[]);
        self.out.push_str(&format!("fn {p}_mkhm(xs: List[Int]) -> HashMap[Str, Int]\n= xs.fold(hash_map(), (m, x) => m.put(\"k{{x % 5}}\", x))\n\n"));
        self.sig(&format!("{p}_mkhm"), &[T::List], T::HMap, &[]);
    }

    fn extra_helpers(&mut self) {
        let (p, c) = (self.p.clone(), self.c.clone());
        self.out.push_str(&format!("fn {p}_gfrom[T](xs: List[T]) -> {c}G[T]\n= xs.reverse.fold({c}Nil, (acc, x) => {c}Cons{{h: x, t: acc}})\n\n"));
        self.out.push_str(&format!("fn {p}_gto[T](g: {c}G[T]) -> List[T]\n= match g\n  | {c}Nil => []\n  | {c}Cons{{h, t}} => [h] + {p}_gto(t)\n\n"));
        self.out.push_str(&format!("fn {p}_glen[T](g: {c}G[T]) -> Int\n= match g\n  | {c}Nil => 0\n  | {c}Cons{{t}} => 1 + {p}_glen(t)\n\n"));
        self.out.push_str(&format!("fn {p}_gmap[A, B](g: {c}G[A], f: A -> B) -> {c}G[B]\n= match g\n  | {c}Nil => {c}Nil\n  | {c}Cons{{h, t}} => {c}Cons{{h: f(h), t: {p}_gmap(t, f)}}\n\n"));
        self.out.push_str(&format!("fn {p}_gnth[T](g: {c}G[T], k: Int, d: T) -> T\n= match g\n  | {c}Nil => d\n  | {c}Cons{{h, t}} => if k <= 0 then h else {p}_gnth(t, k - 1, d)\n\n"));
        self.out.push_str(&format!("fn {p}_swap[A, B](q: {c}P[A, B]) -> {c}P[B, A]\n= {c}P{{fst: q.snd, snd: q.fst}}\n\n"));
        self.out.push_str(&format!("fn {p}_pick[T](b: Bool, x: T, y: T) -> T\n= if b then x else y\n\n"));
        self.out.push_str(&format!("fn {p}_gapp[A, e](x: A, f: A -> A ! e) -> A ! e\n= f(f(x))\n\n"));
        self.out.push_str(&format!("fn {p}_lg(a: Int) -> Int ! log\n= do\n  log(\"lg {{a}}\")\n  a * 2 + 1\n\n"));
        self.sig(&format!("{p}_lg"), &[T::Int], T::Int, &["log"]);
        let (k1, k2) = (self.r.range(-3, 9), self.r.range(1, 5));
        self.out.push_str(&format!("fn {p}_lfold(xs: List[Int]) -> Int\n= match xs\n  | [] => {k1}\n  | [x] => x % 1000\n  | [x, y, ..rest] => (x - y) % 1000 + {p}_lfold(rest)\n\n"));
        self.sig(&format!("{p}_lfold"), &[T::List], T::Int, &[]);
        self.out.push_str(&format!("fn {p}_lends(xs: List[Int]) -> Int\n= match xs\n  | [0] | [{k2}] => {k1}\n  | [first, last] | [first, .., last] => first % 100 - last % 100\n  | [x] => x % 7\n  | [] => -1\n\n"));
        self.sig(&format!("{p}_lends"), &[T::List], T::Int, &[]);
        self.out.push_str(&format!("fn {p}_linit(xs: List[Int]) -> List[Int]\n= match xs\n  | [..init, _] if init.len > {k2} => init\n  | [_, ..rest] => rest\n  | [] => [{k1}]\n\n"));
        self.sig(&format!("{p}_linit"), &[T::List], T::List, &[]);
        let op = ["x * 0.5 + 1.0", "x * x - 2.0", "(x + 1.0) / 3.0", "x - x * 0.25"][self.r.below(4) as usize];
        self.out.push_str(&format!("fn {p}_fl(x: F64, k: Int) -> F64\n= if k <= 0 then x else {p}_fl({op}, k - 1)\n\n"));
        self.sig(&format!("{p}_fl"), &[T::F64, T::Int], T::F64, &[]);
        if self.sys {
            self.out.push_str(&format!("fn {p}_rdrop(r: {c}Rs) -> Unit ! log\n= log(\"drop {{r.id}} {{r.v}}\")\n\n"));
            self.out.push_str(&format!("fn {p}_rmk(id: Int, v: Int) -> {c}Rs\n= {c}Rs{{id: id, v: v}}\n\n"));
            self.out.push_str(&format!("fn {p}_rbump(r: &mut {c}Rs, d: Int)\n= do\n  r.v := r.v + d\n  ()\n\n"));
            self.out.push_str(&format!("fn {p}_rget(r: &{c}Rs) -> Int\n= r.v * 2 + r.id\n\n"));
            self.out.push_str(&format!("fn {p}_rtake(r: {c}Rs) -> Int ! log\n= r.v + r.id\n\n"));
        }
    }

    /// A trait with a default method, impls for a record, a sum and Int, an operator impl,
    /// and bounded generics, each reached through a monomorphic wrapper.
    fn traits(&mut self) {
        let (p, c) = (self.p.clone(), self.c.clone());
        let k = self.r.range(2, 9);
        let dflt = ["x.{p}_sz * 2 + 1", "x.{p}_sz - {k}", "if x.{p}_sz > {k} then x.{p}_sz else {k}"][self.r.below(3) as usize].replace("{p}", &p).replace("{k}", &k.to_string());
        self.out.push_str(&format!("trait {c}Sh\n  fn {p}_sz(x: Self) -> Int\n  fn {p}_dsc(x: Self) -> Int = {dflt}\n\n"));
        let rsz = ["r.a + r.b", "r.a * {k} - r.b", "r.b % {k} + r.a"][self.r.below(3) as usize].replace("{k}", &k.to_string());
        self.out.push_str(&format!("impl {c}Sh for {c}R\n  fn {p}_sz(r: {c}R) -> Int = {rsz}\n\n"));
        self.out.push_str(&format!("impl {c}Sh for {c}S\n  fn {p}_sz(s: {c}S) -> Int\n  = match s\n    | {c}A{{x}} => x\n    | {c}B{{x, y}} => x - y\n    | {c}C => {k}\n  fn {p}_dsc(s: {c}S) -> Int = s.{p}_sz + 1\n\n"));
        self.out.push_str(&format!("impl {c}Sh for Int\n  fn {p}_sz(n: Int) -> Int = n % {}\n\n", k + 90));
        let add = ["x.a + y.a", "x.a - y.b", "x.a * y.a"][self.r.below(3) as usize];
        self.out.push_str(&format!("impl Add for {c}R\n  fn add(x: {c}R, y: {c}R) -> {c}R = {c}R{{a: {add}, b: x.b}}\n\n"));
        self.out.push_str(&format!("fn {p}_tot[T: {c}Sh](xs: List[T]) -> Int\n= xs.map(_.{p}_dsc).sum\n\n"));
        let cmp = [">", "<", ">="][self.r.below(3) as usize];
        self.out.push_str(&format!("fn {p}_mx[T: Ord + Show](xs: List[T], d: T) -> Str\n= xs.fold(d, (m, x) => if x {cmp} m then x else m).show\n\n"));
        self.out.push_str(&format!("fn {p}_acc[T: Add](xs: List[T], z: T) -> T\n= xs.fold(z, (a, x) => a + x)\n\n"));
        self.out.push_str(&format!("fn {p}_trsz(r: {c}R) -> Int = r.{p}_dsc\n\n"));
        self.sig(&format!("{p}_trsz"), &[T::Rec], T::Int, &[]);
        self.out.push_str(&format!("fn {p}_tssz(s: {c}S) -> Int = {p}_tot([s, {c}C])\n\n"));
        self.sig(&format!("{p}_tssz"), &[T::Sum], T::Int, &[]);
        self.out.push_str(&format!("fn {p}_tlsz(xs: List[Int]) -> Int = {p}_tot(xs) + {p}_acc(xs, {k})\n\n"));
        self.sig(&format!("{p}_tlsz"), &[T::List], T::Int, &[]);
        self.out.push_str(&format!("fn {p}_trmx(x: {c}R, y: {c}R) -> Str = {p}_mx([x, y], x + y)\n\n"));
        self.sig(&format!("{p}_trmx"), &[T::Rec, T::Rec], T::Str, &[]);
        self.out.push_str(&format!("fn {p}_trlt(x: {c}R, y: {c}R) -> Bool = x < y or x.hash == y.hash\n\n"));
        self.sig(&format!("{p}_trlt"), &[T::Rec, T::Rec], T::Bool, &[]);
        self.out.push_str(&format!("fn {p}_tsmx(xs: List[Int]) -> Str = {p}_mx(xs.map(_ % {k}), 0) + {p}_mx([\"a\", \"b\"], \"\")\n\n"));
        self.sig(&format!("{p}_tsmx"), &[T::List], T::Str, &[]);
    }

    fn risky_effects(&self, cx: &mut Cx) {
        let name = format!("{}_risky", self.p);
        if let Some(sg) = self.sigs.iter().find(|s| s.name == name) {
            cx.eff.extend(sg.effects.iter().cloned());
        }
    }

    fn live_res(&self, cx: &Cx) -> Vec<String> {
        cx.vars.iter().filter(|v| v.ty == T::Res && !cx.moved.contains(&v.name)).map(|v| v.name.clone()).collect()
    }

    fn res_stmt(&mut self, cx: &mut Cx, ind: usize) {
        let p = self.p.clone();
        let live = self.live_res(cx);
        cx.eff.insert("log".into());
        if live.is_empty() || self.r.chance(30) {
            let name = self.fresh("r");
            let id = self.r.range(0, 99);
            let v = self.int(cx, 1);
            self.line(ind, &format!("var {name} = {p}_rmk({id}, {v})"));
            cx.vars.push(Var { name: name.clone(), ty: T::Res, mutable: true, assignable: false, index_of: None, eff: vec![] });
            cx.res_loop.insert(name, cx.loops);
            return;
        }
        let r = live[self.r.below(live.len() as u64) as usize].clone();
        let here = cx.res_loop.get(&r).copied() == Some(cx.loops);
        match self.r.below(5) {
            0 => {
                let d = self.int_atom(cx);
                self.line(ind, &format!("{p}_rbump(&mut {r}, {d})"));
            }
            1 if here => {
                let x = self.fresh("x");
                self.line(ind, &format!("{x} = {p}_rtake({r})"));
                cx.moved.insert(r);
                self.bind(cx, x, T::Int, false);
            }
            2 if here => {
                let b = self.boolean(cx, 1);
                let q = self.fresh("q");
                self.line(ind, &format!("if {b} then do"));
                self.line(ind + 1, &format!("{q} = {p}_rtake({r})"));
                self.line(ind + 1, &format!("log(\"took {{{q}}}\")"));
                self.line(ind, "else do");
                self.line(ind + 1, &format!("{p}_rbump(&mut {r}, 1)"));
                cx.moved.insert(r);
            }
            3 => {
                let id = self.r.range(100, 199);
                let v = self.int_atom(cx);
                self.line(ind, &format!("{r} := {p}_rmk({id}, {v})"));
            }
            _ => {
                let x = self.fresh("x");
                self.line(ind, &format!("{x} = {p}_rget(&{r})"));
                self.bind(cx, x, T::Int, false);
            }
        }
    }

    fn par_stmt(&mut self, cx: &mut Cx, ind: usize) {
        let vars: Vec<Var> = cx.vars.iter().filter(|v| !v.mutable && !matches!(v.ty, T::Clo | T::Res)).cloned().collect();
        let mut parts = vec![];
        let k = 2 + self.r.below(2) as usize;
        let mut tys = vec![];
        for i in 0..k {
            let t = if i == 0 { T::Int } else { [T::Int, T::Int, T::Str, T::F64, T::List][self.r.below(5) as usize] };
            let mut e = None;
            for _ in 0..3 {
                let mut inner = Cx { vars: vars.clone(), lambda: true, no_risky: true, ..Default::default() };
                let x = self.expr(&mut inner, t, 2);
                if inner.eff.iter().all(|e| e == "div") {
                    cx.eff.extend(inner.eff);
                    e = Some(x);
                    break;
                }
            }
            parts.push(e.unwrap_or_else(|| match t {
                T::Str => "\"t\"".into(),
                T::F64 => "1.5".into(),
                T::List => "[1, 2]".into(),
                _ => self.lit_int(),
            }));
            tys.push(t);
        }
        let names: Vec<String> = tys.iter().map(|_| self.fresh("pa")).collect();
        self.line(ind, &format!("({}) = par({})", names.join(", "), parts.join(", ")));
        for (n, t) in names.into_iter().zip(tys) {
            self.bind(cx, n, t, false);
        }
    }

    fn float(&mut self, cx: &mut Cx, d: u32) -> String {
        if d == 0 || self.r.chance(20) {
            return self.float_atom(cx);
        }
        let d1 = d - 1;
        let p = self.p.clone();
        match self.r.below(17) {
            0..=4 => {
                let op = ["+", "-", "*", "/", "+", "*", "%"][self.r.below(7) as usize];
                let (a, b) = (self.float(cx, d1), self.float(cx, d1));
                format!("({a} {op} {b})")
            }
            5 => {
                let a = self.float(cx, d1);
                format!("(-({a}))")
            }
            6 => {
                let a = self.float(cx, d1);
                format!("{a}.{}", ["abs", "sqrt", "abs"][self.r.below(3) as usize])
            }
            7 => {
                let a = self.int(cx, d1);
                format!("{a}.to_f64")
            }
            8 => {
                let (c, a, b) = (self.boolean(cx, d1), self.float(cx, d1), self.float(cx, d1));
                format!("(if {c} then {a} else {b})")
            }
            9 => {
                let (a, b) = (self.float(cx, d1), self.float(cx, d1));
                format!("{}({a}, {b})", if self.r.chance(50) { "min" } else { "max" })
            }
            10 => self.call(cx, T::F64, d1).unwrap_or_else(|| "2.5".into()),
            11 => {
                let (a, b) = (self.float(cx, d1), self.float(cx, d1));
                if self.r.chance(50) { format!("[{a}, {b}, 0.25].sum") } else { format!("[{a}, {b}].fold(0.5, (s, x) => s * 0.5 + x)") }
            }
            12 => {
                let (c, a, b) = (self.boolean(cx, 0), self.float(cx, d1), self.float(cx, d1));
                format!("{p}_pick({c}, {a}, {b})")
            }
            13 => {
                let (a, b, k) = (self.float(cx, d1), self.float(cx, d1), self.int_atom(cx));
                format!("{p}_gnth({p}_gfrom([{a}, {b}]), {k}, 9.5)")
            }
            14 => {
                let l = self.list(cx, d1);
                let (f, k) = (self.float_atom(cx), self.int_atom(cx));
                [
                    format!("{l}.map(x => x.to_f64 * 0.5).sum"),
                    format!("{l}.map(x => x.to_f64 / 3.0 + {f}).sort.first.or(0.25)"),
                    format!("(0..{k} % 70000).map(e => e.to_f64 * 0.1).sum"),
                    format!("(0..{k} % 70000).map(e => (e % 7).to_f64 * {f}).filter(x => x > 1.0).sum"),
                    format!("{l}.map(x => x.to_f64).fold({f}, (s, x) => s * 0.5 + x)"),
                ][self.r.below(5) as usize]
                    .clone()
            }
            15 => {
                let (a, s) = (self.float(cx, d1), self.string(cx, 0));
                format!("{p}_swap({}P{{fst: {s}, snd: {a}}}).fst", self.c)
            }
            _ => self.float_atom(cx),
        }
    }

    fn float_atom(&mut self, cx: &Cx) -> String {
        if self.r.chance(55)
            && let Some(v) = self.var_of(cx, T::F64)
        {
            return v;
        }
        FLITS[self.r.below(FLITS.len() as u64) as usize].to_string()
    }

    fn risky(&mut self) {
        let (p, c) = (self.p.clone(), self.c.clone());
        let mut cx = Cx::default();
        cx.vars.push(Var { name: "a".into(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        let k1 = self.r.range(2, 7);
        let k2 = self.r.range(2, 9);
        cx.no_risky = true;
        let body = self.int(&mut cx, 2);
        cx.eff.remove(&format!("fail[{c}E]"));
        let eff = if cx.eff.is_empty() { format!(" ! fail[{c}E]") } else { format!(" ! fail[{c}E], {}", cx.eff.iter().cloned().collect::<Vec<_>>().join(", ")) };
        self.out.push_str(&format!(
            "fn {p}_risky(a: Int) -> Int{eff}\n= if a % {k1} == 0 then raise {c}Bad{{c: a}} else if a % {k2} == 1 then raise {c}Worse else {body}\n\n"
        ));
        let mut effs = vec![format!("fail[{c}E]")];
        effs.extend(cx.eff.iter().cloned());
        self.sigs.push(Sig { name: format!("{p}_risky"), params: vec![T::Int], ret: T::Int, effects: effs });
    }

    fn asker(&mut self) {
        let p = self.p.clone();
        let mut cx = Cx::default();
        cx.vars.push(Var { name: "a".into(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        let e = self.int(&mut cx, 1);
        let op = ["+", "-", "*"][self.r.below(3) as usize];
        cx.eff.insert(format!("{p}ask"));
        let eff = self.eff_of(&cx);
        self.out.push_str(&format!("fn {p}_eff(a: Int) -> Int{eff}\n= {p}ask(a) {op} {p}ask({e})\n\n"));
        let effs: Vec<String> = cx.eff.into_iter().collect();
        self.sigs.push(Sig { name: format!("{p}_eff"), params: vec![T::Int], ret: T::Int, effects: effs });
    }

    fn recursive(&mut self) {
        let p = self.p.clone();
        let name = self.fresh(&format!("{p}_rec"));
        let refined = self.r.chance(40);
        let mut cx = Cx::default();
        cx.vars.push(Var { name: "n".into(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        cx.vars.push(Var { name: "acc".into(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        let step = self.int(&mut cx, 2);
        let base = self.int(&mut cx, 1);
        let dec = ["n - 1", "n / 2", "n - 2", "n % 10 - 1"][self.r.below(4) as usize];
        let stop = if refined { "n <= 0" } else { ["n <= 0", "n < 2", "n == 0"][self.r.below(3) as usize] };
        let body = if self.r.chance(50) {
            format!("if {stop} then {base} else {name}({dec}, {step})")
        } else {
            format!("if {stop} then {base} else ({step}) + {name}({dec}, acc)")
        };
        let post = if self.r.chance(25) { format!("\n  post r {}", ["!= 7", ">= acc", ">= -1000000", "> -9223372036854775807"][self.r.below(4) as usize]) } else { String::new() };
        let pt = if refined { "Int where _ >= 0" } else { "Int" };
        let eff = self.eff_of(&cx);
        self.out.push_str(&format!("fn {name}(n: {pt}, acc: Int) -> Int{eff}{post}\n= {body}\n\n"));
        let effs: Vec<String> = cx.eff.into_iter().collect();
        self.sigs.push(Sig { name, params: vec![T::Int, T::Int], ret: T::Int, effects: effs });
    }

    fn tree_rec(&mut self) {
        let (p, c) = (self.p.clone(), self.c.clone());
        let name = self.fresh(&format!("{p}_tr"));
        let mut cx = Cx::default();
        cx.vars.push(Var { name: "k".into(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        cx.vars.push(Var { name: "v".into(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        let nv = self.int(&mut cx, 2);
        let swap = self.r.chance(30);
        let (lf, rt) = if swap { (format!("{name}(r, k)"), format!("{name}(l, k)")) } else { (format!("{name}(l, k)"), format!("{name}(r, k)")) };
        let eff = self.eff_of(&cx);
        let arm = if self.r.chance(30) {
            format!("if v > k then {c}Nd{{l: {lf}, v: {nv}, r: r}} else {c}Nd{{l: l, v: v, r: {rt}}}")
        } else {
            format!("{c}Nd{{l: {lf}, v: {nv}, r: {rt}}}")
        };
        self.out.push_str(&format!("fn {name}(t: {c}T, k: Int) -> {c}T{eff}\n= match t\n  | {c}Lf => {c}Lf\n  | {c}Nd{{l, v, r}} => {arm}\n\n"));
        let effs: Vec<String> = cx.eff.into_iter().collect();
        self.sigs.push(Sig { name, params: vec![T::Tree, T::Int], ret: T::Tree, effects: effs });
    }

    fn random_fn(&mut self, simple: bool) {
        let p = self.p.clone();
        let name = self.fresh(&format!("{p}_f"));
        let mut cx = Cx::default();
        let np = 1 + self.r.below(3) as usize;
        let mut params = vec![];
        let mut ptext = vec![];
        let mut pres = vec![];
        for i in 0..np {
            let t = if simple || self.r.chance(55) { T::Int } else { [T::List, T::Str, T::Rec, T::Sum, T::Tree, T::Map, T::F64][self.r.below(7) as usize] };
            let pn = ["a", "b", "c"][i].to_string();
            let mut tn = ty_name(&self.c, t);
            if t == T::Int && self.r.chance(25) {
                tn = format!("Int where {}", ["_ >= 0", "_ > 0", "_ < 1000", "_ >= -50 and _ <= 50"][self.r.below(4) as usize]);
            }
            ptext.push(format!("{pn}: {tn}"));
            params.push(t);
            cx.vars.push(Var { name: pn, ty: t, mutable: false, assignable: false, index_of: None, eff: vec![] });
        }
        let ints: Vec<String> = cx.vars.iter().filter(|v| v.ty == T::Int).map(|v| v.name.clone()).collect();
        if !ints.is_empty() && self.r.chance(35) {
            let a = &ints[self.r.below(ints.len() as u64) as usize];
            let b = &ints[self.r.below(ints.len() as u64) as usize];
            pres.push(match self.r.below(4) {
                0 => format!("pre {a} <= {b}"),
                1 => format!("pre {a} > -1000 and {a} < 1000"),
                2 => format!("pre {a} != {b} or {a} == 0"),
                _ => format!("pre {a} >= 0"),
            });
        }
        let ret = if simple { T::Int } else { [T::Int, T::Int, T::Int, T::Bool, T::Str, T::List, T::Rec, T::Tree, T::Map, T::Set, T::HMap, T::F64][self.r.below(12) as usize] };
        cx.ret = Some(ret);
        if ret == T::Int && self.r.chance(25) {
            pres.push(format!("post {}", ["r >= 0", "r != 0", "r > -1000000", "r >= a or r < a"][self.r.below(4) as usize]).replace(" a", &format!(" {}", ints.first().cloned().unwrap_or("0".into()))));
        }
        let save = std::mem::take(&mut self.out);
        if self.r.chance(30) && !matches!(ret, T::Clo) {
            let e = self.expr(&mut cx, ret, 3);
            self.out.push_str(&format!("= {e}\n"));
        } else {
            self.line(0, "= do");
            let nb = 2 + self.r.below(5) as usize; self.block(&mut cx, 1, nb, Some(ret), 0);
        }
        let body = std::mem::replace(&mut self.out, save);
        let eff = self.eff_of(&cx);
        self.out.push_str(&format!("fn {name}({}) -> {}{eff}\n", ptext.join(", "), ty_name(&self.c, ret)));
        for pr in pres {
            self.out.push_str(&format!("  {pr}\n"));
        }
        self.out.push_str(&body);
        self.out.push('\n');
        let effs: Vec<String> = cx.eff.into_iter().collect();
        self.sigs.push(Sig { name, params, ret, effects: effs });
    }

    fn entry(&mut self) {
        let p = self.p.clone();
        let mut cx = Cx { ret: Some(T::Str), ..Default::default() };
        let save = std::mem::take(&mut self.out);
        self.line(0, "= do");
        let nb = 3 + self.r.below(4) as usize; self.block(&mut cx, 1, nb, Some(T::Str), 0);
        let body = std::mem::replace(&mut self.out, save);
        let eff = self.eff_of(&cx);
        self.out.push_str(&format!("fn {p}_main() -> Str{eff}\n{body}\n"));
    }

    fn block(&mut self, cx: &mut Cx, ind: usize, n: usize, ret: Option<T>, loop_depth: u32) {
        let mark = cx.vars.len();
        cx.marks.push(mark);
        cx.rets.push(ret);
        let start = self.out.len();
        for _ in 0..n {
            self.stmt(cx, ind, loop_depth);
        }
        if let Some(t) = ret {
            let e = self.expr(cx, t, 3);
            self.line(ind, &e);
        } else if !cx.vars.iter().any(|v| v.assignable) || self.out.len() == start {
            let e = self.int(cx, 1);
            self.line(ind, &format!("log(\"{{{e}}}\")"));
            cx.eff.insert("log".into());
        }
        cx.vars.truncate(mark);
        cx.marks.pop();
        cx.rets.pop();
    }

    /// `p = e else b`: the rest of the block sees what `p` binds, and `b` is the block's
    /// value (or a `return`) when `e` doesn't match.
    fn guard(&mut self, cx: &mut Cx, ind: usize) {
        let ret = cx.rets.last().copied().flatten();
        let (x, r) = (self.fresh("g"), self.fresh("g"));
        let c = self.c.clone();
        let (pat, val, binds) = match self.r.below(3) {
            0 => (format!("[{x}, ..{r}]"), self.list(cx, 2), vec![(x.clone(), T::Int), (r.clone(), T::List)]),
            1 => (format!("some({x})"), format!("{}.first", self.list(cx, 2)), vec![(x.clone(), T::Int)]),
            _ => (format!("{c}A{{x: {x}}} | {c}B{{x: {x}}}"), self.expr(cx, T::Sum, 2), vec![(x.clone(), T::Int)]),
        };
        let other = match (ret, cx.ret) {
            (_, Some(t)) if !cx.lambda && self.r.chance(30) => format!("return {}", self.expr(cx, t, 1)),
            (Some(t), _) => self.expr(cx, t, 1),
            (None, _) => "()".into(),
        };
        self.line(ind, &format!("{pat} = {val} else {other}"));
        for (n, t) in binds {
            self.bind(cx, n, t, false);
        }
    }

    fn bind(&mut self, cx: &mut Cx, name: String, ty: T, mutable: bool) {
        cx.vars.push(Var { name, ty, mutable, assignable: mutable, index_of: None, eff: vec![] });
    }

    fn stmt(&mut self, cx: &mut Cx, ind: usize, loop_depth: u32) {
        let k = self.r.below(100);
        let assignable: Vec<Var> = cx.vars.iter().filter(|v| v.assignable).cloned().collect();
        if k < 3 {
            self.guard(cx, ind);
        } else if k < 14 {
            let t = self.any_ty();
            let e = self.expr(cx, t, 3);
            let shadow = cx.vars.iter().filter(|v| v.ty == t && !v.mutable).map(|v| v.name.clone()).collect::<Vec<_>>();
            let mark = cx.marks.last().copied().unwrap_or(0);
            let local: Vec<String> = cx.vars[mark..].iter().filter(|v| !v.mutable && v.ty != t && !cx.vars[..mark].iter().any(|o| o.name == v.name)).map(|v| v.name.clone()).collect();
            let name = if !shadow.is_empty() && self.r.chance(25) {
                shadow[self.r.below(shadow.len() as u64) as usize].clone()
            } else if !local.is_empty() && self.r.chance(15) {
                let n = local[self.r.below(local.len() as u64) as usize].clone();
                cx.vars.retain(|v| v.name != n);
                n
            } else {
                self.fresh("x")
            };
            self.line(ind, &format!("{name} = {e}"));
            self.bind(cx, name, t, false);
        } else if k < 28 {
            let t = [T::Int, T::Int, T::Int, T::List, T::Str, T::Tree, T::Map, T::Set, T::Rec, T::F64][self.r.below(10) as usize];
            let e = self.expr(cx, t, 2);
            let name = self.fresh("v");
            self.line(ind, &format!("var {name} = {e}"));
            self.bind(cx, name, t, true);
        } else if k < 44 && !assignable.is_empty() {
            let v = &assignable[self.r.below(assignable.len() as u64) as usize];
            let e = self.update(cx, v);
            self.line(ind, &format!("{} := {e}", v.name));
        } else if k < 56 && loop_depth < 2 {
            self.for_loop(cx, ind, loop_depth);
        } else if k < 60 && loop_depth < 2 {
            let c = self.fresh("c");
            let lim = self.r.range(0, 6);
            self.ensure_acc(cx, ind);
            self.line(ind, &format!("var {c} = 0"));
            self.line(ind, &format!("while {c} < {lim}"));
            cx.vars.push(Var { name: c.clone(), ty: T::Int, mutable: true, assignable: false, index_of: None, eff: vec![] });
            cx.loops += 1;
            let nb = 1 + self.r.below(3) as usize; self.block(cx, ind + 1, nb, None, loop_depth + 1);
            cx.loops -= 1;
            self.line(ind + 1, &format!("{c} := {c} + 1"));
            cx.eff.insert("div".into());
        } else if k < 68 && !assignable.is_empty() {
            let b = self.boolean(cx, 2);
            let v = &assignable[self.r.below(assignable.len() as u64) as usize];
            let e = self.update(cx, v);
            if self.r.chance(60) {
                self.line(ind, &format!("if {b} then {} := {e}", v.name));
            } else {
                self.line(ind, &format!("if {b} then do"));
                let nb = 1 + self.r.below(2) as usize; self.block(cx, ind + 1, nb, None, loop_depth + 1);
                self.line(ind, "else do");
                self.block(cx, ind + 1, 1, None, loop_depth + 1);
            }
        } else if k < 71 {
            self.multi(cx, ind);
        } else if k < 74 {
            self.block_lambda(cx, ind);
        } else if k < 79 {
            let ints: Vec<Var> = cx.vars.iter().filter(|v| v.ty == T::Int && (v.mutable || self.r.chance(50))).cloned().collect();
            if !ints.is_empty() {
                let v = &ints[self.r.below(ints.len() as u64) as usize];
                let f = self.fresh("f");
                let x = self.fresh("y");
                let op = ["+", "*", "-", "%"][self.r.below(4) as usize];
                self.line(ind, &format!("{f} = {x} => {x} {op} {}", v.name));
                cx.vars.push(Var { name: f, ty: T::Clo, mutable: false, assignable: false, index_of: None, eff: vec![] });
            } else {
                let name = self.fresh("v");
                let e = self.int(cx, 2);
                self.line(ind, &format!("var {name} = {e}"));
                self.bind(cx, name, T::Int, true);
            }
        } else if k < 83 && !cx.lambda {
            if let Some(t) = cx.ret {
                let b = self.boolean(cx, 1);
                let e = self.expr(cx, t, 2);
                self.line(ind, &format!("if {b} then return {e}"));
            }
        } else if k < 86 {
            let e = self.int(cx, 2);
            let s = self.string(cx, 1);
            self.line(ind, &format!("log(\"{{{e}}} {{{s}}}\")"));
            cx.eff.insert("log".into());
        } else if k < 90 {
            self.local_fn(cx, ind);
        } else if k < 97 && self.sys && !cx.lambda && (k >= 94 || self.r.chance(50)) {
            self.res_stmt(cx, ind);
        } else if k < 97 {
            self.par_stmt(cx, ind);
        } else {
            let name = self.fresh("v");
            let e = self.int(cx, 2);
            self.line(ind, &format!("var {name} = {e}"));
            self.bind(cx, name, T::Int, true);
        }
    }

    /// A statement whose lambda argument has a `do` block body.
    fn block_lambda(&mut self, cx: &mut Cx, ind: usize) {
        let name = self.fresh("bl");
        let (x, y) = (self.fresh("x"), self.fresh("y"));
        let vars: Vec<Var> = cx.vars.iter().filter(|v| v.ty != T::Res && !cx.moved.contains(&v.name)).cloned().collect();
        let mut inner = Cx { vars, ask_ok: cx.ask_ok, lambda: true, no_risky: cx.no_risky, ..Default::default() };
        let int = |name: &String| Var { name: name.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] };
        let kind = self.r.below(5);
        let (head, ty) = match kind {
            0 => {
                let k = self.r.range(0, 400);
                (format!("{name} = (0..{k}).map({x} => do"), T::Int)
            }
            1 => {
                let (l, a) = (self.list(cx, 1), self.int_atom(cx));
                let acc = self.fresh("a");
                inner.vars.push(int(&acc));
                (format!("{name} = {l}.fold({a}, ({acc}, {x}) => do"), T::Int)
            }
            _ => {
                let l = self.list(cx, 1);
                (format!("{name} = {l}.map({x} => do"), T::List)
            }
        };
        self.line(ind, &head);
        inner.vars.push(int(&x));
        let b = ind + 1;
        if kind == 0 {
            let (m, c) = (self.r.range(1, 9), self.r.range(2, 40));
            self.line(b, &format!("{y} = ({x} * {m}) % {c}"));
            let t = self.r.range(0, 9);
            let f = self.r.range(0, 4);
            self.line(b, &format!("if {y} > {t} then {y} - {t} else 0).filter(_ > {f}).sum"));
            self.bind(cx, name, ty, false);
            return;
        }
        let e = self.int(&mut inner, 2);
        self.line(b, &format!("{y} = {e}"));
        inner.vars.push(int(&y));
        let acc: Vec<String> = cx.vars.iter().filter(|v| v.assignable && v.ty == T::Int).map(|v| v.name.clone()).collect();
        if !acc.is_empty() && self.r.chance(40) {
            let v = acc[self.r.below(acc.len() as u64) as usize].clone();
            self.line(b, &format!("{v} := ({v} + {y}) % 1000003"));
        }
        if self.r.chance(20) {
            self.line(b, &format!("log(\"bl {{{y}}}\")"));
            inner.eff.insert("log".into());
        }
        let close = if self.r.chance(25) { "\n".to_string() + &"  ".repeat(ind) + ")" } else { ")".to_string() };
        if kind == 4 {
            let (e1, e2) = (self.int(&mut inner, 1), self.int(&mut inner, 1));
            let k = self.r.range(2, 5);
            self.line(b, &format!("match {y} % {k}"));
            self.line(b, &format!("| 0 => {e1}"));
            self.line(b, &format!("| _ => {e2}{close}"));
        } else {
            let e = self.int(&mut inner, 2);
            self.line(b, &format!("{e}{close}"));
        }
        cx.eff.extend(inner.eff);
        self.bind(cx, name, ty, false);
    }

    fn ensure_acc(&mut self, cx: &mut Cx, ind: usize) {
        if !cx.vars.iter().any(|v| v.assignable) {
            let name = self.fresh("acc");
            self.line(ind, &format!("var {name} = 0"));
            self.bind(cx, name, T::Int, true);
        }
    }

    fn local_fn(&mut self, cx: &mut Cx, ind: usize) {
        let ints: Vec<Var> = cx.vars.iter().filter(|v| v.assignable && v.ty == T::Int).cloned().collect();
        let Some(v) = ints.last() else { return };
        let f = self.fresh("g");
        let k = self.fresh("k");
        let mut inner = Cx { vars: cx.vars.clone(), lambda: true, ..Default::default() };
        inner.vars.push(Var { name: k.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        let e = self.int(&mut inner, 2);
        let eff: Vec<String> = inner.eff.into_iter().collect();
        let row = if eff.is_empty() { String::new() } else { format!(" ! {}", eff.join(", ")) };
        self.line(ind, &format!("fn {f}({k}: Int) -> Int{row}"));
        self.line(ind, "= do");
        self.line(ind + 1, &format!("{} := {e}", v.name));
        self.line(ind + 1, &v.name);
        cx.vars.push(Var { name: f, ty: T::Clo, mutable: false, assignable: false, index_of: None, eff });
    }

    fn for_loop(&mut self, cx: &mut Cx, ind: usize, loop_depth: u32) {
        self.ensure_acc(cx, ind);
        let i = self.fresh("i");
        let lists: Vec<Var> = cx.vars.iter().filter(|v| v.ty == T::List).cloned().collect();
        let mut index_of = None;
        let src = match self.r.below(6) {
            0 | 1 if !lists.is_empty() => {
                let l = &lists[self.r.below(lists.len() as u64) as usize];
                index_of = Some(l.name.clone());
                format!("0..{}.len", l.name)
            }
            2 if !lists.is_empty() => lists[self.r.below(lists.len() as u64) as usize].name.clone(),
            3 => {
                let (a, b) = (self.r.range(-5, 5), self.r.range(-5, 12));
                let s = [1, 2, 3, -1, -2][self.r.below(5) as usize];
                format!("range({a}, {b}, {s})")
            }
            _ => {
                let hi = if self.r.chance(30) {
                    let e = self.int_atom(cx);
                    format!("{e} % 8")
                } else {
                    self.r.range(0, 7).to_string()
                };
                format!("0..{hi}")
            }
        };
        self.line(ind, &format!("for {i} in {src}"));
        cx.vars.push(Var { name: i.clone(), ty: T::Int, mutable: false, assignable: false, index_of, eff: vec![] });
        if self.r.chance(15) && !lists.is_empty() {
            let l = &lists[self.r.below(lists.len() as u64) as usize];
            if l.mutable {
                let k = self.r.range(0, 3);
                self.line(ind + 1, &format!("{} := {}.take({k})", l.name, l.name));
            } else {
                self.line(ind + 1, &format!("{} = {}.drop(1)", l.name, l.name));
            }
        }
        cx.loops += 1;
        let nb = 1 + self.r.below(3) as usize; self.block(cx, ind + 1, nb, None, loop_depth + 1);
        cx.loops -= 1;
        cx.vars.retain(|v| v.name != i);
    }

    fn multi(&mut self, cx: &mut Cx, ind: usize) {
        let name = self.fresh("m");
        let c = self.c.clone();
        let p = self.p.clone();
        let int_var = |n: &str| Var { name: n.to_string(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] };
        match self.r.below(9) {
            6 => {
                let l = self.list(cx, 2);
                let (x, y, rest) = (self.fresh("q"), self.fresh("q"), self.fresh("q"));
                self.line(ind, &format!("{name} = match {l}"));
                let e0 = self.int(cx, 1);
                cx.vars.push(int_var(&x));
                let e1 = self.int(cx, 2);
                cx.vars.push(int_var(&y));
                cx.vars.push(Var { name: rest.clone(), ty: T::List, mutable: false, assignable: false, index_of: None, eff: vec![] });
                let e2 = self.int(cx, 2);
                cx.vars.truncate(cx.vars.len() - 3);
                let k = self.r.range(-2, 4);
                match self.r.below(4) {
                    0 => {
                        self.line(ind + 1, &format!("| [] => {e0}"));
                        self.line(ind + 1, &format!("| [{x}] => {e1}"));
                        self.line(ind + 1, &format!("| [{x}, {y}, ..{rest}] => {e2}"));
                    }
                    1 => {
                        self.line(ind + 1, &format!("| [{k}, ..{rest}] | [_, {k}, ..{rest}] => {e0} + {rest}.len"));
                        self.line(ind + 1, &format!("| [{x}, .., {y}] => {e1} - {y}"));
                        self.line(ind + 1, &format!("| _ => {e0}"));
                    }
                    2 => {
                        self.line(ind + 1, &format!("| [..{rest}, {y}, {x}] if {x} > {k} => {e2}"));
                        self.line(ind + 1, &format!("| [{x}] | [_, {x}] => {e1}"));
                        self.line(ind + 1, &format!("| [_, _, ..] | [] => {e0}"));
                    }
                    _ => {
                        let s = self.expr(cx, T::Sum, 1);
                        self.line(ind + 1, &format!("| [] => {e0}"));
                        self.line(ind + 1, &format!("| [{x}, ..] => match ({x} % 3, {s})"));
                        self.line(ind + 2, &format!("| (0, {c}A{{x: {y}}}) | (1, {c}B{{y: {y}}}) => {y} + {x}"));
                        self.line(ind + 2, &format!("| (-1 | 2 | -2, _) => {x}"));
                        self.line(ind + 2, &format!("| (_, {c}C) => {k}"));
                        self.line(ind + 2, &format!("| _ => {x} * 2"));
                    }
                }
            }
            7 => {
                let s = self.expr(cx, T::Sum, 2);
                let x = self.fresh("q");
                cx.vars.push(int_var(&x));
                let e1 = self.int(cx, 2);
                cx.vars.pop();
                let e2 = self.int(cx, 1);
                let n = self.int(cx, 1);
                let (k1, k2) = (self.r.range(-3, 2), self.r.range(3, 6));
                self.line(ind, &format!("{name} = match {s}"));
                self.line(ind + 1, &format!("| {c}A{{x: {x}}} | {c}B{{y: {x}}} => {e1}"));
                self.line(ind + 1, &format!("| {c}C => match {n} % 8"));
                self.line(ind + 2, &format!("| {k1} | {k2} | 7 => {e2}"));
                self.line(ind + 2, "| _ => 1");
            }
            8 => {
                let x = self.fresh("q");
                let k = self.r.range(-5, 5);
                let (head, ty) = if self.r.chance(50) {
                    (format!("{}.first", self.list(cx, 2)), "some")
                } else {
                    (self.expr(cx, T::Sum, 2), "sum")
                };
                cx.vars.push(int_var(&x));
                let e1 = self.int(cx, 2);
                cx.vars.pop();
                let e2 = self.int(cx, 1);
                let pat = if ty == "some" { format!("some({x})") } else { format!("{c}A{{x: {x}}} | {c}B{{x: {x}}}") };
                let guard = if self.r.chance(50) { format!(" and {x} > {k}") } else { String::new() };
                if self.r.chance(40) {
                    let (y, l2) = (self.fresh("q"), self.list(cx, 1));
                    cx.vars.push(int_var(&x));
                    cx.vars.push(int_var(&y));
                    let e1 = self.int(cx, 2);
                    cx.vars.truncate(cx.vars.len() - 2);
                    self.line(ind, &format!("{name} = if {head} is {pat} and ({l2}) is [_, {y}, ..]{guard} then {e1} + {y} else {e2}"));
                } else {
                    self.line(ind, &format!("{name} = if {head} is {pat}{guard} then {e1} else {e2}"));
                }
            }
            5 => {
                let lg = self.fresh("lg");
                let m = self.fresh("s");
                self.line(ind, &format!("var {lg} = 0"));
                let mut inner = Cx { vars: cx.vars.clone(), ask_ok: cx.ask_ok, lambda: true, no_risky: cx.no_risky, ..Default::default() };
                let arg = self.int(&mut inner, 1);
                let more = self.int(&mut inner, 2);
                inner.eff.remove("log");
                cx.eff.extend(inner.eff);
                self.line(ind, &format!("{name} = handle {p}_lg({arg}) + {more}"));
                match self.r.below(4) {
                    0 => {
                        let e = self.int_atom(cx);
                        let n = self.r.range(3, 8);
                        self.line(ind + 1, &format!("| log({m}) => if {m}.len > {n} then {e} else do"));
                        self.line(ind + 2, &format!("{lg} := {lg} + {m}.len"));
                        self.line(ind + 2, "resume()");
                    }
                    1 => {
                        let r = self.fresh("q");
                        self.line(ind + 1, &format!("| log({m}) => do"));
                        self.line(ind + 2, &format!("{lg} := {lg} + 1"));
                        self.line(ind + 2, &format!("{r} = resume()"));
                        self.line(ind + 2, &format!("{lg} := {lg} * 3 + {m}.len"));
                        self.line(ind + 2, &format!("{r} + {m}.len"));
                    }
                    _ => {
                        self.line(ind + 1, &format!("| log({m}) => do"));
                        self.line(ind + 2, &format!("{lg} := {lg} + {m}.len + 1"));
                        self.line(ind + 2, "resume()");
                    }
                }
                cx.vars.push(Var { name: lg, ty: T::Int, mutable: true, assignable: true, index_of: None, eff: vec![] });
            }
            0 => {
                let s = self.expr(cx, T::Sum, 2);
                self.line(ind, &format!("{name} = match {s}"));
                let (x, y) = (self.fresh("q"), self.fresh("q"));
                cx.vars.push(Var { name: x.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
                let e1 = self.int(cx, 2);
                cx.vars.push(Var { name: y.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
                let e2 = self.int(cx, 2);
                cx.vars.truncate(cx.vars.len() - 2);
                let e3 = self.int(cx, 1);
                self.line(ind + 1, &format!("| {c}A{{x: {x}}} => {e1}"));
                if self.r.chance(30) {
                    let g = self.boolean(cx, 1);
                    let e4 = self.int(cx, 1);
                    self.line(ind + 1, &format!("| {c}B{{x: {x}, y: {y}}} if {g} => {e4}"));
                }
                cx.vars.push(Var { name: x.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
                cx.vars.push(Var { name: y.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
                cx.vars.truncate(cx.vars.len() - 2);
                self.line(ind + 1, &format!("| {c}B{{x: {x}, y: {y}}} => {e2}"));
                self.line(ind + 1, &format!("| {c}C => {e3}"));
            }
            1 => {
                let l = self.list(cx, 2);
                let q = self.fresh("q");
                self.line(ind, &format!("{name} = match {l}.first"));
                cx.vars.push(Var { name: q.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
                let e1 = self.int(cx, 2);
                cx.vars.pop();
                let e2 = self.int(cx, 1);
                self.line(ind + 1, &format!("| some({q}) => {e1}"));
                self.line(ind + 1, &format!("| none => {e2}"));
            }
            2 => {
                let mut inner = Cx { vars: cx.vars.clone(), ask_ok: cx.ask_ok, lambda: true, ..Default::default() };
                let arg = self.int(&mut inner, 2);
                let e = format!("{p}_risky({arg})");
                self.risky_effects(&mut inner);
                let more = self.int(&mut inner, 1);
                inner.eff.remove(&format!("fail[{c}E]"));
                cx.eff.extend(inner.eff);
                let q = self.fresh("q");
                self.line(ind, &format!("{name} = catch {e} + {more}"));
                cx.vars.push(Var { name: q.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
                let e1 = self.int(cx, 1);
                cx.vars.pop();
                let e2 = self.int(cx, 1);
                self.line(ind + 1, &format!("| {c}Bad{{c: {q}}} => {e1}"));
                self.line(ind + 1, &format!("| {c}Worse => {e2}"));
            }
            3 => {
                let mut inner = Cx { vars: cx.vars.clone(), ask_ok: true, lambda: true, ..Default::default() };
                let arg = self.int(&mut inner, 2);
                let more = self.int(&mut inner, 1);
                if let Some(sg) = self.sigs.iter().find(|s| s.name == format!("{p}_eff")) {
                    inner.eff.extend(sg.effects.iter().cloned());
                }
                inner.eff.remove(&format!("{p}ask"));
                cx.eff.extend(inner.eff);
                let q = self.fresh("q");
                self.line(ind, &format!("{name} = handle {p}_eff({arg}) + {more}"));
                cx.vars.push(Var { name: q.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
                let e1 = self.int(cx, 1);
                cx.vars.pop();
                match self.r.below(10) {
                    0..=4 => self.line(ind + 1, &format!("| {p}ask({q}) => resume({e1})")),
                    5 | 6 => self.line(ind + 1, &format!("| {p}ask({q}) => {e1}")),
                    7 => {
                        let r = self.fresh("q");
                        self.line(ind + 1, &format!("| {p}ask({q}) => do"));
                        self.line(ind + 2, &format!("{r} = resume({e1})"));
                        self.line(ind + 2, &format!("{r} * 2 + {q}"));
                    }
                    8 => {
                        let k = self.r.range(-3, 9);
                        self.line(ind + 1, &format!("| {p}ask({q}) => if {q} > {k} then {e1} else resume({q} + 1)"));
                    }
                    _ => {
                        let k = self.r.range(-3, 9);
                        cx.eff.insert(format!("fail[{c}E]"));
                        self.line(ind + 1, &format!("| {p}ask({q}) => if {q} > {k} then raise {c}Bad{{c: {q}}} else resume({e1})"));
                    }
                }
            }
            _ => {
                let t = self.expr(cx, T::Tree, 2);
                let (l, v, r) = (self.fresh("q"), self.fresh("q"), self.fresh("q"));
                self.line(ind, &format!("{name} = match {t}"));
                let e2 = self.int(cx, 1);
                cx.vars.push(Var { name: v.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
                cx.vars.push(Var { name: l.clone(), ty: T::Tree, mutable: false, assignable: false, index_of: None, eff: vec![] });
                cx.vars.push(Var { name: r.clone(), ty: T::Tree, mutable: false, assignable: false, index_of: None, eff: vec![] });
                let e1 = self.int(cx, 2);
                cx.vars.truncate(cx.vars.len() - 3);
                self.line(ind + 1, &format!("| {c}Lf => {e2}"));
                self.line(ind + 1, &format!("| {c}Nd{{l: {l}, v: {v}, r: {r}}} => {e1}"));
            }
        }
        self.bind(cx, name, T::Int, false);
    }

    fn update(&mut self, cx: &mut Cx, v: &Var) -> String {
        let n = &v.name;
        let p = self.p.clone();
        match v.ty {
            T::Int => match self.r.below(4) {
                0 => {
                    let e = self.int(cx, 2);
                    format!("{n} + {e}")
                }
                1 => {
                    let e = self.int(cx, 1);
                    format!("{n} * {e}")
                }
                _ => self.int(cx, 2),
            },
            T::List => match self.r.below(4) {
                0 => {
                    let e = self.int(cx, 2);
                    format!("{n}.push({e})")
                }
                1 => {
                    let l = self.lambda(cx);
                    format!("{n}.map({l})")
                }
                _ => self.list(cx, 2),
            },
            T::Tree => match self.r.below(3) {
                0 => {
                    let e = self.int(cx, 2);
                    format!("{p}_inc({n}, {e})")
                }
                _ => {
                    let e = self.int(cx, 2);
                    format!("{p}_ins({n}, {e})")
                }
            },
            T::Map => {
                let (a, b) = (self.int(cx, 1), self.int(cx, 2));
                if self.r.chance(70) { format!("{n}.put({a}, {b})") } else { format!("{n}.remove({a})") }
            }
            T::Set => {
                let a = self.int(cx, 1);
                if self.r.chance(70) { format!("{n}.add({a})") } else { format!("{n}.remove({a})") }
            }
            T::Str => {
                let s = self.string(cx, 1);
                format!("{n} + {s}")
            }
            T::Rec => {
                let e = self.int(cx, 2);
                format!("{n} with {} := {e}", if self.r.chance(50) { "a" } else { "b" })
            }
            T::F64 => {
                let e = self.float(cx, 2);
                [format!("{n} + {e}"), format!("{n} * 0.5 + {e}"), e][self.r.below(3) as usize].clone()
            }
            t => self.expr(cx, t, 2),
        }
    }

    fn any_ty(&mut self) -> T {
        [T::Int, T::Int, T::Int, T::Bool, T::Str, T::List, T::List, T::Rec, T::Sum, T::Tree, T::Map, T::Set, T::HMap, T::F64][self.r.below(14) as usize]
    }

    fn expr(&mut self, cx: &mut Cx, t: T, d: u32) -> String {
        match t {
            T::Int => self.int(cx, d),
            T::Bool => self.boolean(cx, d),
            T::Str => self.string(cx, d),
            T::List => self.list(cx, d),
            T::Rec => self.rec(cx, d),
            T::Sum => self.sum(cx, d),
            T::Tree => self.tree(cx, d),
            T::Map => self.map(cx, d),
            T::Set => self.set(cx, d),
            T::HMap => self.hmap(cx, d),
            T::Clo => "(z => z)".into(),
            T::F64 => self.float(cx, d),
            T::Res => "()".into(),
        }
    }

    fn var_of(&mut self, cx: &Cx, t: T) -> Option<String> {
        let vs: Vec<&Var> = cx.vars.iter().filter(|v| v.ty == t).collect();
        if vs.is_empty() { None } else { Some(vs[self.r.below(vs.len() as u64) as usize].name.clone()) }
    }

    fn lit_int(&mut self) -> String {
        let k = self.r.below(100);
        if k < 70 {
            self.r.range(-12, 30).to_string()
        } else if k < 88 {
            self.r.range(-2000, 2000).to_string()
        } else {
            BOUNDARY[self.r.below(BOUNDARY.len() as u64) as usize].to_string()
        }
    }

    fn int_atom(&mut self, cx: &Cx) -> String {
        if self.r.chance(65)
            && let Some(v) = self.var_of(cx, T::Int)
        {
            return v;
        }
        let s = self.lit_int();
        if s.starts_with('-') { format!("({s})") } else { s }
    }

    fn call(&mut self, cx: &mut Cx, ret: T, d: u32) -> Option<String> {
        let cands: Vec<Sig> = self.sigs.iter().filter(|s| s.ret == ret && (cx.ask_ok || !s.effects.iter().any(|e| e.ends_with("ask"))) && !(cx.no_risky && s.name.ends_with("_risky"))).cloned().collect();
        if cands.is_empty() {
            return None;
        }
        let s = cands[self.r.below(cands.len() as u64) as usize].clone();
        let args: Vec<String> = s.params.iter().map(|t| self.expr(cx, *t, d.saturating_sub(1))).collect();
        for e in &s.effects {
            cx.eff.insert(e.clone());
        }
        Some(format!("{}({})", s.name, args.join(", ")))
    }

    fn int(&mut self, cx: &mut Cx, d: u32) -> String {
        if d == 0 || self.r.chance(20) {
            return self.int_atom(cx);
        }
        let d1 = d - 1;
        let p = self.p.clone();
        match self.r.below(46) {
            0..=7 => {
                let op = ["+", "-", "*", "+", "-", "*", "/", "%"][self.r.below(8) as usize];
                let (a, b) = (self.int(cx, d1), self.int(cx, d1));
                format!("({a} {op} {b})")
            }
            8 => {
                let a = self.int(cx, d1);
                format!("(-({a}))")
            }
            9 => {
                let (c, a, b) = (self.boolean(cx, d1), self.int(cx, d1), self.int(cx, d1));
                format!("(if {c} then {a} else {b})")
            }
            10 | 11 => self.call(cx, T::Int, d1).unwrap_or_else(|| "1".into()),
            12 => {
                let l = self.list(cx, d1);
                [format!("{l}.len"), format!("{l}.sum"), format!("{l}.fold(0, (s, x) => s + x * 2)"), format!("{l}.max.or(0)")][self.r.below(4) as usize].clone()
            }
            13 => {
                let (l, i) = (self.list(cx, d1), self.int(cx, d1));
                if self.r.chance(50) { format!("{l}[{i}]") } else { format!("{l}.get({i}).or(-1)") }
            }
            14 | 15 => {
                let idx: Vec<Var> = cx.vars.iter().filter(|v| v.index_of.is_some()).cloned().collect();
                if let Some(v) = idx.last() {
                    format!("{}[{}]", v.index_of.clone().unwrap(), v.name)
                } else {
                    self.int_atom(cx)
                }
            }
            16 => {
                let r = self.rec(cx, d1);
                format!("{r}.{}", if self.r.chance(50) { "a" } else { "b" })
            }
            17 => {
                let s = self.string(cx, d1);
                [format!("{s}.len"), format!("{s}.to_int.or(3)"), format!("{s}.byte_len")][self.r.below(3) as usize].clone()
            }
            18 => {
                let (m, k) = (self.map(cx, d1), self.int(cx, d1));
                if self.r.chance(60) { format!("{m}.get({k}).or(0)") } else { format!("{m}.len") }
            }
            19 => {
                let st = self.set(cx, d1);
                [format!("{st}.len"), format!("{st}.min.or(0)"), format!("{st}.items.sum")][self.r.below(3) as usize].clone()
            }
            20 => {
                let (h, s) = (self.hmap(cx, d1), self.string(cx, 0));
                if self.r.chance(60) { format!("{h}.get({s}).or(5)") } else { format!("{h}.len") }
            }
            21 => {
                let (a, b) = (self.int(cx, d1), self.int(cx, d1));
                let m = ["wrapping_add", "wrapping_mul", "band", "bxor", "saturating_mul", "saturating_sub", "gcd"][self.r.below(7) as usize];
                format!("{a}.{m}({b})")
            }
            22 => {
                let (a, b) = (self.int(cx, d1), self.int(cx, d1));
                let m = ["checked_add", "checked_mul", "checked_div", "checked_sub"][self.r.below(4) as usize];
                format!("{a}.{m}({b}).or(-9)")
            }
            23 => {
                let a = self.int(cx, d1);
                [format!("{a}.abs"), format!("({a} ** {})", self.r.range(0, 3)), format!("{a}.shl({})", self.r.range(0, 70)), format!("{a}.popcount")][self.r.below(4) as usize].clone()
            }
            24 => {
                let (a, b, c) = (self.int(cx, d1), self.int(cx, d1), self.int(cx, d1));
                [format!("min({a}, {b})"), format!("max({a}, {b})"), format!("clamp({a}, {b}, {c})")][self.r.below(3) as usize].clone()
            }
            25 => {
                let t = self.tree(cx, d1);
                format!("{p}_tsum({t})")
            }
            26 if cx.ask_ok => {
                let a = self.int(cx, d1);
                format!("{p}ask({a})")
            }
            27 => {
                let clos: Vec<String> = cx.vars.iter().filter(|v| v.ty == T::Clo).map(|v| v.name.clone()).collect();
                if let Some(f) = clos.last() {
                    let eff = cx.vars.iter().rev().find(|v| &v.name == f).map(|v| v.eff.clone()).unwrap_or_default();
                    cx.eff.extend(eff);
                    let a = self.int(cx, d1);
                    format!("{f}({a})")
                } else {
                    self.int_atom(cx)
                }
            }
            28 | 29 => self.pipeline(cx, d1),
            30 if !cx.no_risky => {
                let a = self.int(cx, d1);
                self.risky_effects(cx);
                format!("{p}_risky({a})")
            }
            31 => {
                let sm = self.sum(cx, d1);
                format!("{sm}.str.len")
            }
            32 | 33 => {
                let s = self.string(cx, d1);
                let k = self.int_atom(cx);
                [
                    format!("{s}.split(\",\").len"),
                    format!("{s}.index_of(\"a\").or(-1)"),
                    format!("{s}.byte({k})"),
                    format!("{s}.words.len"),
                    format!("{s}.chars.len"),
                    format!("{s}.pad_left(3, \"*\").len"),
                ][self.r.below(6) as usize]
                    .clone()
            }
            36 | 37 => {
                let f = self.float(cx, d1);
                [
                    format!("{f}.round"),
                    format!("{f}.floor"),
                    format!("{f}.ceil"),
                    format!("{f}.trunc"),
                    format!("({f}.{} * 1000.0).round", ["sin", "cos", "exp", "ln", "atan", "sqrt"][self.r.below(6) as usize]),
                    format!("{f}.fmt({}).len", self.r.range(0, 4)),
                    format!("\"{{{f}}}\".len"),
                ][self.r.below(7) as usize]
                    .clone()
            }
            38 => {
                let l = self.list(cx, d1);
                let k = self.int_atom(cx);
                [
                    format!("{p}_glen({p}_gfrom({l}))"),
                    format!("{p}_gto({p}_gfrom({l})).sum"),
                    format!("{p}_gnth({p}_gfrom({l}), {k}, -1)"),
                ][self.r.below(3) as usize]
                    .clone()
            }
            39 => {
                let l = self.list(cx, d1);
                let f = self.pure_lambda(cx);
                format!("{p}_gto({p}_gmap({p}_gfrom({l}), {f})).sum")
            }
            40 => {
                let (b, x, y) = (self.boolean(cx, 0), self.int(cx, d1), self.int(cx, d1));
                format!("{p}_pick({b}, {x}, {y})")
            }
            41 => {
                let (s, i) = (self.string(cx, d1), self.int(cx, d1));
                if self.r.chance(50) { format!("{p}_swap({}P{{fst: {s}, snd: {i}}}).fst", self.c) } else { format!("{p}_glen({p}_gmap({p}_gfrom([{s}, \"b\"]), x => x.len))") }
            }
            42 => {
                let (a, f) = (self.int(cx, d1), self.lambda(cx));
                format!("{p}_gapp({a}, {f})")
            }
            43 if !cx.lambda => {
                let live = self.live_res(cx);
                if live.is_empty() {
                    self.int_atom(cx)
                } else {
                    format!("{p}_rget(&{})", live[self.r.below(live.len() as u64) as usize])
                }
            }
            44 => {
                let l = self.list(cx, d1);
                format!("{p}_glen({p}_gmap({p}_gfrom({l}), x => x.to_f64 * 1.5))")
            }
            34 | 35 => {
                let l = self.list(cx, d1);
                let k = self.int_atom(cx);
                let f = self.lambda(cx);
                [
                    format!("{l}.sort_by({f}).first.or(0)"),
                    format!("{l}.enumerate.map(p => p.0 * p.1).sum"),
                    format!("{l}.windows(2).len"),
                    format!("{l}.index_of({k}).or(-1)"),
                    format!("{l}.counts.len"),
                    format!("{l}.last.or(7)"),
                    format!("{l}.zip({l}.reverse).map(p => p.0 - p.1).sum"),
                    format!("{l}.chunks({k} % 4 + 1).len"),
                ][self.r.below(8) as usize]
                    .clone()
            }
            _ => self.int_atom(cx),
        }
    }

    fn pipeline(&mut self, cx: &mut Cx, d: u32) -> String {
        let src = if self.r.chance(40) {
            let (a, b) = (self.r.range(-4, 4), self.r.range(0, 40));
            let s = [1, 1, 2, 3, -1, -3][self.r.below(6) as usize];
            if s < 0 { format!("range({b}, {a}, {s})") } else { format!("range({a}, {b}, {s})") }
        } else if self.r.chance(30) {
            format!("(0..{})", self.r.range(0, 30))
        } else {
            self.list(cx, d)
        };
        let mut out = src;
        for _ in 0..1 + self.r.below(3) {
            if self.r.chance(55) {
                let l = self.lambda(cx);
                out = format!("{out}.map({l})");
            } else {
                let l = self.blambda(cx);
                out = format!("{out}.filter({l})");
            }
        }
        format!("{out}.{}", ["sum", "sum", "len"][self.r.below(3) as usize])
    }

    fn lambda(&mut self, cx: &mut Cx) -> String {
        let x = self.fresh("e");
        cx.vars.push(Var { name: x.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        let was = cx.lambda;
        cx.lambda = true;
        let b = if self.r.chance(50) {
            let op = ["+", "*", "-", "/", "%"][self.r.below(5) as usize];
            let k = self.int_atom(cx);
            format!("{x} {op} {k}")
        } else {
            self.int(cx, 2)
        };
        cx.lambda = was;
        cx.vars.pop();
        format!("{x} => {b}")
    }

    fn pure_lambda(&mut self, cx: &mut Cx) -> String {
        let x = self.fresh("e");
        let op = ["+", "*", "-", "%", "/"][self.r.below(5) as usize];
        let k = self.int_atom(cx);
        format!("{x} => {x} {op} {k}")
    }

    fn blambda(&mut self, cx: &mut Cx) -> String {
        let x = self.fresh("e");
        cx.vars.push(Var { name: x.clone(), ty: T::Int, mutable: false, assignable: false, index_of: None, eff: vec![] });
        let was = cx.lambda;
        cx.lambda = true;
        let b = if self.r.chance(60) {
            let op = ["<", ">", "==", "!=", ">="][self.r.below(5) as usize];
            let k = self.int_atom(cx);
            if self.r.chance(30) { format!("{x} % 2 {op} {k}") } else { format!("{x} {op} {k}") }
        } else {
            self.boolean(cx, 2)
        };
        cx.lambda = was;
        cx.vars.pop();
        format!("{x} => {b}")
    }

    fn boolean(&mut self, cx: &mut Cx, d: u32) -> String {
        if d == 0 {
            if let Some(v) = self.var_of(cx, T::Bool) {
                return v;
            }
            let (a, b) = (self.int_atom(cx), self.int_atom(cx));
            return format!("({a} < {b})");
        }
        let d1 = d - 1;
        match self.r.below(16) {
            15 => {
                let (x, k) = (self.fresh("q"), self.r.range(-3, 4));
                let c = self.c.clone();
                match self.r.below(4) {
                    0 => format!("(({}) is [_, ..])", self.list(cx, d1)),
                    1 => format!("(({}) is [{x}, ..] and {x} > {k})", self.list(cx, d1)),
                    2 => format!("(({}).first is some({x}) and {x} % 3 == {})", self.list(cx, d1), k.rem_euclid(3)),
                    _ => format!("(({}) is {c}A{{x: _}} | {c}C)", self.expr(cx, T::Sum, d1)),
                }
            }
            12 | 13 => {
                let op = ["<", "<=", ">", ">=", "==", "!="][self.r.below(6) as usize];
                let (a, b) = (self.float(cx, d1), self.float(cx, d1));
                if self.r.chance(25) { format!("{a}.{}", ["is_nan", "is_finite"][self.r.below(2) as usize]) } else { format!("({a} {op} {b})") }
            }
            14 => {
                let (b, x, y) = (self.boolean(cx, 0), self.boolean(cx, d1), self.boolean(cx, d1));
                if self.r.chance(50) {
                    format!("{}_pick({b}, {x}, {y})", self.p)
                } else {
                    let (l, m) = (self.list(cx, d1), self.list(cx, d1));
                    format!("({p}_gfrom({l}) == {p}_gfrom({m}))", p = self.p)
                }
            }
            0..=3 => {
                let op = ["<", "<=", ">", ">=", "==", "!="][self.r.below(6) as usize];
                let (a, b) = (self.int(cx, d1), self.int(cx, d1));
                format!("({a} {op} {b})")
            }
            4 => {
                let (a, b) = (self.boolean(cx, d1), self.boolean(cx, d1));
                format!("({a} {} {b})", if self.r.chance(50) { "and" } else { "or" })
            }
            5 => {
                let a = self.boolean(cx, d1);
                format!("(not {a})")
            }
            6 => {
                let (l, a) = (self.list(cx, d1), self.int(cx, d1));
                if self.r.chance(50) { format!("{l}.contains({a})") } else { format!("{l}.is_empty") }
            }
            7 => {
                let (st, a) = (self.set(cx, d1), self.int(cx, d1));
                format!("{st}.has({a})")
            }
            8 => {
                let (m, a) = (self.map(cx, d1), self.int(cx, d1));
                format!("{m}.has({a})")
            }
            9 => {
                let (s, t) = (self.string(cx, d1), self.string(cx, 0));
                let m = ["starts_with", "contains", "ends_with"][self.r.below(3) as usize];
                format!("{s}.{m}({t})")
            }
            10 => {
                let l = self.list(cx, d1);
                let b = self.blambda(cx);
                format!("{l}.{}({b})", if self.r.chance(50) { "any" } else { "all" })
            }
            _ => {
                let t = self.any_ty();
                if matches!(t, T::Clo) {
                    return "true".into();
                }
                let (a, b) = (self.expr(cx, t, d1), self.expr(cx, t, d1));
                format!("({a} == {b})")
            }
        }
    }

    fn string(&mut self, cx: &mut Cx, d: u32) -> String {
        if d == 0 || self.r.chance(20) {
            if self.r.chance(50)
                && let Some(v) = self.var_of(cx, T::Str)
            {
                return v;
            }
            return STRS[self.r.below(STRS.len() as u64) as usize].to_string();
        }
        let d1 = d - 1;
        match self.r.below(15) {
            12 => {
                let f = self.float(cx, d1);
                [format!("{f}.str"), format!("{f}.fmt({})", self.r.range(0, 4)), format!("\"f{{{f}}}\"")][self.r.below(3) as usize].clone()
            }
            13 => {
                let (b, x, y) = (self.boolean(cx, 0), self.string(cx, d1), self.string(cx, d1));
                format!("{}_pick({b}, {x}, {y})", self.p)
            }
            14 => {
                let (i, s, k) = (self.int(cx, d1), self.string(cx, d1), self.int_atom(cx));
                if self.r.chance(50) { format!("{p}_swap({c}P{{fst: {i}, snd: {s}}}).fst", p = self.p, c = self.c) } else { format!("{p}_gnth({p}_gfrom([{s}, \"q\"]), {k}, \"d\")", p = self.p) }
            }
            0 => {
                let v = self.int_atom(cx);
                format!("\"n{{{v}}}\"")
            }
            1 => {
                let (a, b) = (self.string(cx, d1), self.string(cx, d1));
                format!("({a} + {b})")
            }
            2 => {
                let a = self.string(cx, d1);
                format!("{a}.{}", ["upper", "lower", "reverse", "trim"][self.r.below(4) as usize])
            }
            3 => {
                let (a, k) = (self.string(cx, d1), self.r.range(-1, 4));
                format!("{a}.{}({k})", if self.r.chance(50) { "take" } else { "drop" })
            }
            4 => {
                let l = self.list(cx, d1);
                format!("{l}.map(x => x.str).join(\",\")")
            }
            5 => {
                let a = self.int(cx, d1);
                format!("{a}.str")
            }
            6 => {
                let a = self.string(cx, d1);
                format!("{a}.replace(\"a\", \"xy\")")
            }
            7 => {
                let t = self.any_ty();
                if t == T::Clo {
                    return "\"c\"".into();
                }
                let e = self.expr(cx, t, d1);
                format!("{e}.str")
            }
            8 => {
                let (a, k) = (self.string(cx, d1), self.r.range(0, 3));
                format!("{a}.repeat({k})")
            }
            9 => {
                let (a, k) = (self.string(cx, d1), self.int_atom(cx));
                [
                    format!("{a}.split(\",\").join(\"-\")"),
                    format!("{a}.chars.reverse.join(\"\")"),
                    format!("{a}.get({k}).or(\"?\")"),
                    format!("{a}.pad_right(4, \"ab\")"),
                    format!("{a}.words.join(\"_\")"),
                ][self.r.below(5) as usize]
                    .clone()
            }
            _ => self.call(cx, T::Str, d1).unwrap_or_else(|| "\"s\"".into()),
        }
    }

    fn list(&mut self, cx: &mut Cx, d: u32) -> String {
        if d == 0 || self.r.chance(20) {
            if self.r.chance(60)
                && let Some(v) = self.var_of(cx, T::List)
            {
                return v;
            }
            let n = self.r.range(1, 5);
            let xs: Vec<String> = (0..n).map(|_| self.int_atom(cx)).collect();
            return format!("[{}]", xs.join(", "));
        }
        let d1 = d - 1;
        let p = self.p.clone();
        match self.r.below(20) {
            18 => {
                let (l, f) = (self.list(cx, d1), self.pure_lambda(cx));
                format!("{p}_gto({p}_gmap({p}_gfrom({l}), {f}))")
            }
            19 => {
                let (b, x, y) = (self.boolean(cx, 0), self.list(cx, d1), self.list(cx, d1));
                format!("{p}_pick({b}, {x}, {y})")
            }
            14 => {
                let l = self.list(cx, d1);
                format!("{l}.flat_map(x => [x, x + 1])")
            }
            15 => {
                let (l, f) = (self.list(cx, d1), self.blambda(cx));
                format!("{l}.{}({f})", ["take_while", "drop_while", "filter"][self.r.below(3) as usize])
            }
            16 => {
                let (l, k) = (self.list(cx, d1), self.int_atom(cx));
                format!("{l}.{}", [format!("rotate({k})"), format!("slice(1, {k})"), "unique.sort".to_string()][self.r.below(3) as usize])
            }
            17 => {
                let l = self.list(cx, d1);
                let f = self.lambda(cx);
                format!("{l}.sort_by({f})")
            }
            0 | 1 => {
                let (l, f) = (self.list(cx, d1), self.lambda(cx));
                format!("{l}.map({f})")
            }
            2 => {
                let (l, f) = (self.list(cx, d1), self.blambda(cx));
                format!("{l}.filter({f})")
            }
            3 => {
                let (a, b) = (self.r.range(-5, 5), self.r.range(-5, 15));
                let s = [1, 2, 3, -1, -2, 0][self.r.below(6) as usize];
                format!("range({a}, {b}, {s})")
            }
            4 => format!("(0..{})", self.r.range(0, 8)),
            5 => {
                let (l, a) = (self.list(cx, d1), self.int(cx, d1));
                format!("{l}.push({a})")
            }
            6 => {
                let (a, b) = (self.list(cx, d1), self.list(cx, d1));
                format!("{a}.concat({b})")
            }
            7 => {
                let l = self.list(cx, d1);
                format!("{l}.{}", ["sort", "reverse", "unique"][self.r.below(3) as usize])
            }
            8 => {
                let (l, k) = (self.list(cx, d1), self.r.range(-1, 4));
                format!("{l}.{}({k})", if self.r.chance(50) { "take" } else { "drop" })
            }
            9 => {
                let t = self.tree(cx, d1);
                format!("{p}_items({t})")
            }
            10 => {
                let m = self.map(cx, d1);
                format!("{m}.{}", if self.r.chance(50) { "keys" } else { "values" })
            }
            11 => {
                let l = self.list(cx, d1);
                format!("{l}.scan(0, (s, x) => s + x)")
            }
            12 => {
                let st = self.set(cx, d1);
                format!("{st}.items")
            }
            _ => self.call(cx, T::List, d1).unwrap_or_else(|| "[1]".into()),
        }
    }

    fn rec(&mut self, cx: &mut Cx, d: u32) -> String {
        if (d == 0 || self.r.chance(30))
            && let Some(v) = self.var_of(cx, T::Rec)
        {
            return v;
        }
        let d1 = d.saturating_sub(1);
        if d > 0 && self.r.chance(30) {
            if let Some(c) = self.call(cx, T::Rec, d1) {
                return c;
            }
            let r = self.rec(cx, d1);
            let e = self.int(cx, d1);
            return format!("({r} with {} := {e})", if self.r.chance(50) { "a" } else { "b" });
        }
        let (a, b) = (self.int(cx, d1), self.int(cx, d1));
        format!("{}R{{a: {a}, b: {b}}}", self.c)
    }

    fn sum(&mut self, cx: &mut Cx, d: u32) -> String {
        if (d == 0 || self.r.chance(30))
            && let Some(v) = self.var_of(cx, T::Sum)
        {
            return v;
        }
        let d1 = d.saturating_sub(1);
        let c = self.c.clone();
        match self.r.below(3) {
            0 => {
                let a = self.int(cx, d1);
                format!("{c}A{{x: {a}}}")
            }
            1 => {
                let (a, b) = (self.int(cx, d1), self.int(cx, d1));
                format!("{c}B{{x: {a}, y: {b}}}")
            }
            _ => format!("{c}C"),
        }
    }

    fn tree(&mut self, cx: &mut Cx, d: u32) -> String {
        if (d == 0 || self.r.chance(30))
            && let Some(v) = self.var_of(cx, T::Tree)
        {
            return v;
        }
        let d1 = d.saturating_sub(1);
        let p = self.p.clone();
        if d == 0 {
            return format!("{}Lf", self.c);
        }
        match self.r.below(4) {
            0 => {
                let l = self.list(cx, d1);
                format!("{p}_build({l})")
            }
            1 => {
                let (t, a) = (self.tree(cx, d1), self.int(cx, d1));
                format!("{p}_ins({t}, {a})")
            }
            2 => self.call(cx, T::Tree, d1).unwrap_or_else(|| format!("{}Lf", self.c)),
            _ => {
                let (t, a) = (self.tree(cx, d1), self.int(cx, d1));
                format!("{p}_inc({t}, {a})")
            }
        }
    }

    fn map(&mut self, cx: &mut Cx, d: u32) -> String {
        if (d == 0 || self.r.chance(30))
            && let Some(v) = self.var_of(cx, T::Map)
        {
            return v;
        }
        let d1 = d.saturating_sub(1);
        let p = self.p.clone();
        match self.r.below(3) {
            0 => {
                let (m, a, b) = (self.map(cx, d1), self.int(cx, d1), self.int(cx, d1));
                format!("{m}.put({a}, {b})")
            }
            1 if d > 0 => {
                let (m, a) = (self.map(cx, d1), self.int(cx, d1));
                format!("{m}.remove({a})")
            }
            _ => {
                let l = self.list(cx, d1);
                format!("{p}_mkmap({l})")
            }
        }
    }

    fn set(&mut self, cx: &mut Cx, d: u32) -> String {
        if (d == 0 || self.r.chance(30))
            && let Some(v) = self.var_of(cx, T::Set)
        {
            return v;
        }
        let d1 = d.saturating_sub(1);
        match self.r.below(4) {
            0 if d > 0 => {
                let (s, a) = (self.set(cx, d1), self.int(cx, d1));
                format!("{s}.add({a})")
            }
            1 if d > 0 => {
                let (s, t) = (self.set(cx, d1), self.set(cx, d1));
                format!("{s}.{}({t})", ["union", "inter", "diff"][self.r.below(3) as usize])
            }
            _ => {
                let l = self.list(cx, d1);
                format!("{l}.to_set")
            }
        }
    }

    fn hmap(&mut self, cx: &mut Cx, d: u32) -> String {
        if (d == 0 || self.r.chance(30))
            && let Some(v) = self.var_of(cx, T::HMap)
        {
            return v;
        }
        let d1 = d.saturating_sub(1);
        let p = self.p.clone();
        if d > 0 && self.r.chance(40) {
            let (h, s, a) = (self.hmap(cx, d1), self.string(cx, 0), self.int(cx, d1));
            return format!("{h}.put({s}, {a})");
        }
        let l = self.list(cx, d1);
        format!("{p}_mkhm({l})")
    }
}
