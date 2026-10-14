pub mod cfn;
pub mod crt;
pub mod iam;
pub mod local;
pub mod migrate;
pub mod replay;
pub mod schema;
mod validate;

use sspur_check::{expr_key, CheckOutput, Type};
use sspur_native::cgen::CProgram;
use sspur_native::nval::Layouts;
use sspur_syntax::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

type Ops = BTreeSet<(String, String)>;

#[derive(Clone, Debug)]
pub struct Store {
    pub name: String,
    pub key: Type,
    pub val: Type,
    pub sv: String,
    pub shape: schema::Shape,
    pub key_shape: schema::Shape,
    pub migs: Vec<Mig>,
    pub backs: Vec<Mig>,
}

#[derive(Clone, Debug)]
pub struct Mig {
    pub fun: String,
    pub sv: String,
    pub ty: Type,
    pub shape: schema::Shape,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Path,
    Body,
}

#[derive(Clone, Debug)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub handler: String,
}

impl Route {
    pub fn key(&self) -> String {
        format!("{} {}", self.method.to_uppercase(), self.path)
    }
}

#[derive(Clone, Debug)]
pub struct Handler {
    pub name: String,
    pub params: Vec<(String, Type, Source)>,
    pub ret: Type,
    pub row: Vec<String>,
    pub db: BTreeSet<(String, String)>,
    pub validator: Option<String>,
}

impl Handler {
    pub fn stores(&self) -> BTreeSet<String> {
        self.db.iter().map(|(s, _)| s.clone()).collect()
    }
}

pub struct Service {
    pub name: String,
    pub stores: Vec<Store>,
    pub routes: Vec<Route>,
    pub handlers: Vec<Handler>,
    pub backfills: Vec<Handler>,
    pub program: CProgram,
    pub host_src: Option<String>,
    pub layouts: Layouts,
    pub source: String,
    pub hash: String,
}

fn diag_text(src: &str, check: &CheckOutput) -> String {
    check
        .diags
        .iter()
        .filter(|d| d.is_error())
        .map(|d| {
            let (l, c) = line_col(src, d.span[0]);
            format!("{l}:{c} {} {}", d.code, d.msg)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn load(src: &str) -> Result<(Module, CheckOutput), String> {
    let m = parse(src).map_err(|e| {
        let (l, c) = line_col(src, e.span.start);
        format!("{l}:{c} {} {}", e.code, e.msg)
    })?;
    let check = sspur_check::check(&m);
    if check.has_errors() {
        return Err(diag_text(src, &check));
    }
    Ok((m, check))
}

fn db_ops(f: &FnDef, check: &CheckOutput, fns: &HashSet<&str>, ops: &mut BTreeSet<(String, String)>, calls: &mut BTreeSet<String>) {
    let mut exprs: Vec<&Expr> = vec![&f.body];
    exprs.extend(f.params.iter().filter_map(|p| p.refine.as_ref()));
    for e in exprs {
        visit::walk_expr(e, &mut |x| {
            match &x.kind {
                ExprKind::Method { recv, name, args, .. } if matches!(&recv.kind, ExprKind::Name(n) if n == "db") && !check.expr_types.contains_key(&expr_key(recv)) => {
                    if let (Some(ExprKind::Name(s)), Some(_)) = (args.first().map(|a| &a.kind), sspur_check::db_op(name)) {
                        ops.insert((s.clone(), name.clone()));
                    }
                }
                ExprKind::Method { name, .. } | ExprKind::Name(name) if fns.contains(name.as_str()) => {
                    calls.insert(name.clone());
                }
                _ => {}
            }
            true
        });
    }
}

/// What only `http.serve` supports, or no deploy target provides: effects without an AWS
/// mapping, query string parameters, and raw `Request` and `Response` handlers.
fn deploy_rules(src: &str, svc: &SvcDef, fns: &HashMap<&str, &FnDef>, check: &CheckOutput) -> Result<(), String> {
    let mut errs = Vec::new();
    for ep in &svc.eps {
        let Some(f) = fns.get(ep.handler.as_str()) else { continue };
        let (l, c) = line_col(src, ep.span.start);
        let bad: Vec<String> = f.effects.iter().filter(|e| !matches!(e.name.as_str(), "log" | "div" | "time" | "env" | "fail" | "db.read" | "db.write")).map(printer::effect).collect();
        if !bad.is_empty() {
            errs.push(format!("{l}:{c} E_EP_EFFECT {} performs {}, which no deploy target provides", ep.handler, bad.join(", ")));
        }
        let Some((ptys, ret)) = check.fn_types.get(&f.name) else { continue };
        let path = ep.path_params();
        for (p, t) in f.params.iter().zip(ptys) {
            if matches!(t, Type::Con(n, _) if n == "#Request") {
                errs.push(format!("{l}:{c} E_EP_TYPE {} takes the raw Request, which only http.serve provides", ep.handler));
            } else if !path.contains(&p.name) && matches!(ep.method.as_str(), "get" | "delete") {
                errs.push(format!("{l}:{c} E_EP_BODY {} {} binds '{}' from the query string, which only http.serve supports; a deployed GET or DELETE takes path parameters", ep.method, ep.path, p.name));
            }
        }
        if matches!(ret, Type::Con(n, _) if n == "#Response") {
            errs.push(format!("{l}:{c} E_EP_TYPE {} returns a raw Response, which only http.serve sends", ep.handler));
        }
    }
    if errs.is_empty() { Ok(()) } else { Err(errs.join("\n")) }
}

pub fn analyze(src: &str) -> Result<Service, String> {
    let (m, check) = load(src)?;
    let svcs: Vec<&SvcDef> = m.defs.iter().filter_map(|d| if let Def::Svc(s) = d { Some(s) } else { None }).collect();
    let [svc] = svcs.as_slice() else {
        return Err(format!("a deployable program declares exactly one svc, found {}", svcs.len()));
    };
    let fn_defs: HashMap<&str, &FnDef> = m.defs.iter().filter_map(|d| if let Def::Fn(f) = d { Some((f.name.as_str(), f)) } else { None }).collect();
    deploy_rules(src, svc, &fn_defs, &check)?;
    let fn_names: HashSet<&str> = fn_defs.keys().copied().collect();
    let mut direct: HashMap<&str, (Ops, BTreeSet<String>)> = HashMap::new();
    for (n, f) in &fn_defs {
        let mut ops = BTreeSet::new();
        let mut calls = BTreeSet::new();
        db_ops(f, &check, &fn_names, &mut ops, &mut calls);
        direct.insert(n, (ops, calls));
    }
    let mut vgen = validate::Gen::new(&m);
    let mut handlers: Vec<Handler> = Vec::new();
    let routes: Vec<Route> = svc.eps.iter().map(|e| Route { method: e.method.clone(), path: e.path.clone(), handler: e.handler.clone() }).collect();
    for ep in &svc.eps {
        if handlers.iter().any(|h| h.name == ep.handler) {
            continue;
        }
        let f = fn_defs[ep.handler.as_str()];
        let (ptys, ret) = check.fn_types[&f.name].clone();
        let path: HashSet<String> = svc.eps.iter().filter(|e| e.handler == ep.handler).flat_map(|e| e.path_params()).collect();
        let params = f.params.iter().zip(ptys).map(|(p, t)| (p.name.clone(), t, if path.contains(&p.name) { Source::Path } else { Source::Body })).collect();
        let row: Vec<String> = f.effects.iter().map(printer::effect).collect();
        let mut seen = BTreeSet::new();
        let mut stack = vec![f.name.clone()];
        let mut db = BTreeSet::new();
        while let Some(n) = stack.pop() {
            if !seen.insert(n.clone()) {
                continue;
            }
            if let Some((ops, calls)) = direct.get(n.as_str()) {
                db.extend(ops.iter().cloned());
                stack.extend(calls.iter().cloned());
            }
        }
        db.retain(|(s, op)| {
            let access = sspur_check::db_op(op).map_or("", |a| a.0);
            row.contains(&format!("db.{access}[{s}]"))
        });
        let validator = vgen.entry(f, &path)?;
        handlers.push(Handler { name: f.name.clone(), params, ret, row, db, validator });
    }
    let extra = vgen.source();
    let source = if extra.is_empty() { src.to_string() } else { format!("{}\n\n{extra}\n", src.trim_end()) };
    let (m2, check2) = load(&source).map_err(|e| format!("generated validators do not check (compiler bug):\n{e}"))?;
    let program = sspur_native::cgen::c_program(&m2, &check2)?;
    let host_src = if sspur_native::cgen::win::on() { Some(sspur_native::cgen::c_program_host(&m2, &check2)?.src) } else { None };
    let stores = stores_of(&m, &check, &fn_defs)?;
    let migfns: Vec<&String> = stores.iter().flat_map(|s| s.migs.iter().chain(&s.backs)).map(|g| &g.fun).collect();
    for h in &handlers {
        for n in std::iter::once(&h.name).chain(h.validator.iter()).chain(migfns.iter().copied()) {
            if !program.fns.contains_key(n) {
                let why = program.skipped.get(n).cloned().unwrap_or_else(|| "is not compiled".into());
                return Err(format!("handler {n} cannot be compiled to native code: {why}"));
            }
        }
    }
    let backfills = stores
        .iter()
        .filter(|s| !s.migs.is_empty())
        .map(|s| Handler {
            name: format!("backfill_{}", s.name),
            params: vec![],
            ret: Type::unit(),
            row: vec![format!("db.read[{}]", s.name), format!("db.write[{}]", s.name)],
            db: BTreeSet::from([(s.name.clone(), "scan".to_string()), (s.name.clone(), "put".to_string())]),
            validator: None,
        })
        .collect();
    let hash = blake3::hash(source.as_bytes()).to_hex()[..16].to_string();
    Ok(Service { name: svc.name.clone(), stores, routes, handlers, backfills, layouts: Layouts::from_check(&check2), program, host_src, source, hash })
}

fn stores_of(m: &Module, check: &CheckOutput, fns: &HashMap<&str, &FnDef>) -> Result<Vec<Store>, String> {
    let mut sh = schema::Shapes::new(m);
    let mut out = Vec::new();
    for d in &m.defs {
        let Def::Store(sd) = d else { continue };
        let (key, val) = check.stores[&sd.name].clone();
        let sv = sh.version(&sd.val);
        let mut migs = Vec::new();
        let mut backs = Vec::new();
        let mut names: Vec<&&str> = fns.keys().collect();
        names.sort();
        for n in names {
            let f = fns[*n];
            let fwd = n.strip_prefix("migrate_").is_some_and(|r| r == sd.name || r.starts_with(&format!("{}_", sd.name)));
            let back = n.strip_prefix("unmigrate_").is_some_and(|r| r == sd.name || r.starts_with(&format!("{}_", sd.name)));
            if !fwd && !back {
                continue;
            }
            let (ptys, ret) = &check.fn_types[*n];
            let vt = printer::ty(&sd.val);
            let bad = |want: &str| Err(format!("E_MIGRATE_SIG {n} must be `fn {n}({want}` with no effects"));
            let (old_ty, old_ast) = if fwd {
                if ptys.len() != 1 || *ret != val || !f.effects.is_empty() || ptys[0] == val {
                    return bad(&format!("old: OldT) -> {vt}`, OldT the previous value type,"));
                }
                (ptys[0].clone(), &f.params[0].ty)
            } else {
                let Some(r) = f.ret.as_ref() else { return bad(&format!("new: {vt}) -> OldT`")) };
                if ptys.len() != 1 || ptys[0] != val || !f.effects.is_empty() || *ret == val {
                    return bad(&format!("new: {vt}) -> OldT`"));
                }
                (ret.clone(), r)
            };
            let g = Mig { fun: n.to_string(), sv: sh.version(old_ast), ty: old_ty, shape: sh.ty(old_ast) };
            if fwd { migs.push(g) } else { backs.push(g) }
        }
        for b in &backs {
            if !migs.iter().any(|g| g.sv == b.sv) {
                return Err(format!("E_MIGRATE_SIG {} returns a type that no migrate_{} reads", b.fun, sd.name));
            }
        }
        out.push(Store { name: sd.name.clone(), key, val, sv, shape: sh.ty(&sd.val), key_shape: sh.ty(&sd.key), migs, backs });
    }
    Ok(out)
}

pub struct Plan {
    pub files: BTreeMap<String, String>,
}

pub fn plan(svc: &Service) -> Result<Plan, String> {
    let mut files = BTreeMap::new();
    let template = cfn::template(svc);
    files.insert("template.json".to_string(), format!("{}\n", serde_json::to_string_pretty(&template).unwrap()));
    for h in &svc.handlers {
        files.insert(format!("iam/{}.json", h.name), format!("{}\n", serde_json::to_string_pretty(&iam::policy(svc, h)).unwrap()));
    }
    files.insert("plan.json".to_string(), format!("{}\n", serde_json::to_string_pretty(&iam::summary(svc)).unwrap()));
    files.insert("bootstrap.c".to_string(), crt::generate(svc)?);
    files.insert("service.ssp".to_string(), svc.source.clone());
    files.insert("build.sh".to_string(), cfn::build_script(svc));
    files.insert("deploy.sh".to_string(), cfn::deploy_script(svc));
    files.insert("rollback.sh".to_string(), cfn::rollback_script(svc));
    if !svc.backfills.is_empty() {
        files.insert("backfill.sh".to_string(), cfn::backfill_script(svc));
        for b in &svc.backfills {
            files.insert(format!("iam/{}.json", b.name), format!("{}\n", serde_json::to_string_pretty(&iam::policy(svc, b)).unwrap()));
        }
    }
    Ok(Plan { files })
}

pub fn write_plan(p: &Plan, out: &Path) -> Result<(), String> {
    for (name, body) in &p.files {
        let path = out.join(name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
        #[cfg(unix)]
        if name.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
    Ok(())
}

/// Builds the service's bootstrap for this machine, for `sspur deploy local`.
pub fn build_local(svc: &Service) -> Result<PathBuf, String> {
    build_host(&crt::generate_host(svc)?, &cache_dir())
}

pub fn build_host(c_src: &str, dir: &Path) -> Result<PathBuf, String> {
    let key = blake3::hash(c_src.as_bytes()).to_hex()[..24].to_string();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let bin = dir.join(format!("bootstrap-{key}{}", std::env::consts::EXE_SUFFIX));
    if bin.exists() {
        return Ok(bin);
    }
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let uniq = format!("{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let c = dir.join(format!("bootstrap-{key}-{uniq}.c"));
    std::fs::write(&c, c_src).map_err(|e| e.to_string())?;
    let cc = if cfg!(windows) { sspur_native::cgen::flags::cc() } else { std::env::var("CC").unwrap_or_else(|_| "cc".into()) };
    let tmp = dir.join(format!("bootstrap-{key}-{uniq}.tmp"));
    let libs: Vec<String> = if cfg!(windows) { std::iter::once("-lws2_32".to_string()).chain(sspur_native::cgen::flags::sys_libs()).collect() } else { ["-lcurl", "-lpthread", "-lm"].map(String::from).to_vec() };
    let out = std::process::Command::new(&cc).args(["-O2", "-w", "-o"]).arg(&tmp).arg(&c).args(libs).output().map_err(|e| format!("cannot run {cc}: {e}"))?;
    sspur_native::cgen::flags::drop_link_leftovers(&tmp);
    let _ = std::fs::remove_file(&c);
    if !out.status.success() {
        return Err(format!("{cc} failed:\n{}", String::from_utf8_lossy(&out.stderr).lines().take(20).collect::<Vec<_>>().join("\n")));
    }
    sspur_native::cgen::flags::publish(&tmp, &bin)?;
    Ok(bin)
}

pub fn cache_dir() -> PathBuf {
    sspur_syntax::cache_root().unwrap_or_else(std::env::temp_dir).join("deploy")
}

pub fn pascal(s: &str) -> String {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect()
}
