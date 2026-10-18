use serde_json::Value;
use sspur_deploy::local::{request, request_full, start, start_with, Local, Options};
use sspur_deploy::migrate::{compare, Kind};
use sspur_deploy::{analyze, build_host, plan, Service};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn src(name: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/crud").join(name)).unwrap()
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sspur-evolve-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn build(s: &str) -> (Service, PathBuf) {
    let svc = analyze(s).unwrap_or_else(|e| panic!("{e}"));
    plan(&svc).unwrap();
    let bin = build_host(&sspur_deploy::crt::generate_host(&svc).unwrap(), &tmp("bin")).unwrap();
    (svc, bin)
}

fn quiet() -> sspur_deploy::local::Sink {
    Arc::new(|_: &str| {})
}

fn json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| panic!("not JSON: {s}"))
}

fn version(port: u16, m: &str, p: &str, b: Option<&str>) -> (u16, String, Value) {
    let (s, h, body) = request_full(port, m, p, b).unwrap();
    let v = h.iter().find(|(k, _)| k == "x-sspur-version").map(|x| x.1.clone()).unwrap_or_default();
    (s, v, serde_json::from_str(&body).unwrap_or(Value::Null))
}

fn item(id: &str, qty: i64) -> String {
    format!(r#"{{"id":"{id}","name":"n{id}","qty":{qty},"tags":["t"]}}"#)
}

fn err(s: &str) -> String {
    match analyze(s) {
        Ok(_) => String::new(),
        Err(e) => e,
    }
}

#[test]
fn migrate_classifies_changes() {
    let v1 = analyze(&src("items.ssp")).unwrap();
    let v2 = analyze(&src("evolve/items_v2.ssp")).unwrap();
    let v3 = analyze(&src("evolve/items_v3.ssp")).unwrap();
    let r = compare(&v1.stores, &v2.stores);
    assert_eq!(r.stores[0].kind, Kind::Compatible, "{}", r.text());
    assert!(r.ok() && r.side_by_side());
    assert!(r.text().contains("field note: Opt[Str] added (optional)"), "{}", r.text());
    assert!(v2.backfills.is_empty(), "a compatible change needs no backfill");
    assert_eq!(compare(&v1.stores, &v1.stores).stores[0].kind, Kind::Same);
    let sum = "type C = Red | Blue\ntype T = {id: Str, c: C}\nstore S = table[Str, T]\nfn h(id: Str) -> Opt[T] ! db.read[S]\n= db.get(S, id)\nsvc s\n  ep get \"/t/{id}\" = h\n";
    let r = compare(&analyze(sum).unwrap().stores, &analyze(&sum.replace("Red | Blue", "Red | Blue | Green")).unwrap().stores);
    assert!(r.ok() && !r.side_by_side() && r.text().contains("variant c.Green added"), "{}", r.text());

    let r = compare(&v1.stores, &v3.stores);
    let c = &r.stores[0];
    assert_eq!((c.kind, c.fun.as_deref(), c.reverse.as_deref()), (Kind::Migration, Some("migrate_Items"), Some("unmigrate_Items")), "{}", r.text());
    assert_eq!(v3.stores[0].migs[0].sv, v1.stores[0].sv, "ItemV1 in the new file has the stored schema's hash");
    assert!(r.ok() && r.side_by_side());
    assert_eq!(v3.backfills.len(), 1);
    assert_eq!(sspur_deploy::iam::actions(&v3.backfills[0])["Items"], ["dynamodb:PutItem", "dynamodb:Scan"]);

    let without = |pat: &str| src("evolve/items_v3.ssp").split("\n\n").filter(|d| !d.contains(pat)).collect::<Vec<_>>().join("\n\n");
    let no_fns = without("migrate_Items(");
    let bare = analyze(&no_fns).unwrap();
    let r = compare(&v1.stores, &bare.stores);
    assert_eq!(r.stores[0].kind, Kind::Breaking);
    assert!(!r.ok() && r.text().contains("E_MIGRATE_MISSING") && r.text().contains("field qty removed"), "{}", r.text());

    let r = compare(&v1.stores, &analyze(&without("unmigrate_Items")).unwrap().stores);
    assert!(r.ok() && !r.side_by_side(), "{}", r.text());

    let v3s = src("evolve/items_v3.ssp");
    assert!(err(&v3s.replace("fn migrate_Items(old: ItemV1) -> Item\n", "fn migrate_Items(old: ItemV1) -> Item ! log\n")).contains("E_MIGRATE_SIG"));
    assert!(err(&v3s.replace("stock: Stock{on_hand: old.qty, unit: \"each\"}", "stock: Stock{on_hand: old.name, unit: \"each\"}")).contains("E_TYPE"), "migration functions are typechecked");
    let key = src("items.ssp").replace("store Items = table[ItemId, Item]", "store Items = table[Int, Item]").replace("db.get(Items, it.id)", "db.get(Items, 1)").replace("db.put(Items, it.id, it)", "db.put(Items, 1, it)").replace("ItemId(id)", "1");
    let r = compare(&v1.stores, &analyze(&key).unwrap_or_else(|e| panic!("{e}")).stores);
    assert!(r.text().contains("E_MIGRATE_KEY"), "{}", r.text());
}

#[test]
fn migration_fn_change_rolls_out_side_by_side_and_backfills() {
    let (v1, b1) = build(&src("items.ssp"));
    let (v3, b3) = build(&src("evolve/items_v3.ssp"));
    let local = start(&v1, &b1, 0, quiet()).unwrap();
    let port = local.port;
    for (id, q) in [("a", 3), ("b", 0), ("c", 7)] {
        assert_eq!(request(port, "POST", "/items", Some(&item(id, q))).unwrap().0, 201);
    }
    let rep = local.swap(&v3, &b3, 50).unwrap();
    assert_eq!(rep["migrate"]["stores"][0]["kind"], "migration");
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..12 {
        let (s, v, b) = version(port, "GET", "/items/a", None);
        assert_eq!(s, 200);
        if v == v3.hash {
            assert_eq!(b["stock"], json(r#"{"on_hand":3,"unit":"each"}"#), "lazy read-time migration");
        } else {
            assert_eq!(b["qty"], 3);
        }
        seen.insert(v);
    }
    assert_eq!(seen.len(), 2, "a 50% canary serves both versions");
    local.promote().unwrap();
    let (s, v, _) = version(port, "POST", "/items", Some(r#"{"id":"d","name":"nd","stock":{"on_hand":5,"unit":"box"},"tags":[]}"#));
    assert_eq!((s, v.as_str()), (201, v3.hash.as_str()));
    let (s, _, b) = version(port, "PUT", "/items/b", Some(r#"{"name":"nb","stock":{"on_hand":9,"unit":"kg"},"tags":[]}"#));
    assert_eq!((s, b["stock"]["unit"].clone()), (200, Value::from("kg")), "updating an old item through the migration");

    local.rollback().unwrap();
    let (s, v, b) = version(port, "GET", "/items/d", None);
    assert_eq!((s, v.as_str(), b["qty"].clone()), (200, v1.hash.as_str(), Value::from(5)), "the old version reads new items through the reverse copy");
    assert_eq!(version(port, "GET", "/items", None).2.as_array().unwrap().len(), 4, "state persists across swap and rollback");
    let (s, _, _) = version(port, "PUT", "/items/c", Some(r#"{"name":"nc","qty":8,"tags":[]}"#));
    assert_eq!(s, 200, "old version writes an old-schema item");
    local.rollback().unwrap();
    let (s, v, b) = version(port, "GET", "/items/c", None);
    assert_eq!((s, v.as_str(), b["stock"]["on_hand"].clone()), (200, v3.hash.as_str(), Value::from(8)), "rolling forward migrates it again");

    let table = local.table("Items");
    let before = local.dynamo.items(&table);
    let old_sv = &v1.stores[0].sv;
    let stale = before.values().filter(|it| it["sv"]["S"] != v3.stores[0].sv.as_str()).count();
    assert!(stale >= 2, "items a and c are still on the old schema: {before:?}");
    let r = local.backfill(None).unwrap();
    assert_eq!((r[0]["migrated"].as_u64(), r[0]["failed"].as_u64(), r[0]["scanned"].as_u64()), (Some(stale as u64), Some(0), Some(4)), "{r}");
    for it in local.dynamo.items(&table).values() {
        assert_eq!(it["sv"]["S"], v3.stores[0].sv.as_str());
        assert!(it[&format!("v_{old_sv}")]["S"].is_string(), "every item keeps a copy the old version can read: {it}");
    }
    let again = local.backfill(Some("Items")).unwrap();
    assert_eq!((again[0]["migrated"].as_u64(), again[0]["current"].as_u64()), (Some(0), Some(4)), "backfill is idempotent");
    let all = version(port, "GET", "/items", None).2;
    let units: Vec<&str> = all.as_array().unwrap().iter().map(|x| x["stock"]["unit"].as_str().unwrap()).collect();
    assert_eq!(units, ["each", "kg", "each", "box"]);
    local.rollback().unwrap();
    assert_eq!(version(port, "GET", "/items/a", None).2["qty"], 3, "rollback after backfill still reads every item");
    local.stop();
}

#[test]
fn hot_swap_under_concurrent_requests_drops_nothing() {
    let (v1, b1) = build(&src("items.ssp"));
    let (v2, b2) = build(&src("evolve/items_v2.ssp"));
    let local = Arc::new(start(&v1, &b1, 0, quiet()).unwrap());
    let port = local.port;
    assert_eq!(request(port, "POST", "/items", Some(&item("seed", 1))).unwrap().0, 201);

    local.dynamo.delay_ms.store(400, Ordering::Relaxed);
    let slow = std::thread::spawn(move || version(port, "GET", "/items/seed", None));
    while local.status()["stable"]["inflight"] != 1 {
        std::thread::sleep(Duration::from_millis(5));
    }
    local.swap(&v2, &b2, 100).unwrap();
    local.dynamo.delay_ms.store(0, Ordering::Relaxed);
    let (s, v, b) = slow.join().unwrap();
    assert_eq!((s, v.as_str()), (200, v1.hash.as_str()), "an in-flight request finishes on the old version");
    assert!(b.get("note").is_none());
    let (s, v, b) = version(port, "GET", "/items/seed", None);
    assert_eq!((s, v.as_str(), b["note"].clone()), (200, v2.hash.as_str(), Value::Null), "new requests go to the new version");

    local.rollback().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::new();
    for w in 0..4 {
        let stop = stop.clone();
        workers.push(std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut i = 0;
            while !stop.load(Ordering::Relaxed) || i < 10 {
                let id = format!("w{w}-{i}");
                out.push(("POST", request(port, "POST", "/items", Some(&item(&id, i))).map(|r| r.0)));
                out.push(("GET", request(port, "GET", &format!("/items/{id}"), None).map(|r| r.0)));
                if i % 5 == 0 {
                    out.push(("LIST", request(port, "GET", "/items", None).map(|r| r.0)));
                }
                i += 1;
            }
            out
        }));
    }
    std::thread::sleep(Duration::from_millis(200));
    local.swap(&v2, &b2, 100).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    local.rollback().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    local.rollback().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    stop.store(true, Ordering::Relaxed);
    let (mut total, mut posts) = (0, 0);
    for w in workers {
        for (what, r) in w.join().unwrap() {
            total += 1;
            posts += usize::from(what == "POST");
            let want = if what == "POST" { 201 } else { 200 };
            assert_eq!(r, Ok(want), "{what} failed during swap/rollback");
        }
    }
    assert!(total > 80, "{total} requests");
    let st = local.status();
    assert_eq!(st["stable"]["version"], v2.hash.as_str());
    assert_eq!(st["previous"]["version"], v1.hash.as_str());
    let n = version(port, "GET", "/items", None).2.as_array().unwrap().len();
    assert_eq!(n, 1 + posts, "state persists");
    local.stop();
}

#[test]
fn replay_gates_behavior_changes() {
    let (v1, b1) = build(&src("items.ssp"));
    let rec = tmp("rec").join("rec.jsonl");
    let _ = std::fs::remove_file(&rec);
    let local: Local = start_with(&v1, &b1, 0, quiet(), Options { record: Some(rec.clone()) }).unwrap();
    let port = local.port;
    for (m, p, b) in [
        ("POST", "/items", Some(item("a", 3))),
        ("POST", "/items", Some(item("b", 0))),
        ("POST", "/items", Some(item("a", 1))),
        ("GET", "/items/a", None),
        ("GET", "/items/zz", None),
        ("PUT", "/items/b", Some(r#"{"name":"bee","qty":4,"tags":[]}"#.to_string())),
        ("POST", "/items", Some(item("c", 2))),
        ("GET", "/items", None),
        ("DELETE", "/items/c", None),
        ("DELETE", "/items/c", None),
        ("POST", "/items", Some(r#"{"id":"x","name":"","qty":1,"tags":[]}"#.to_string())),
    ] {
        request(port, m, p, b.as_deref()).unwrap();
    }
    local.stop();
    let recording = std::fs::read_to_string(&rec).unwrap();
    assert_eq!(recording.lines().count(), 11);
    let first = json(recording.lines().next().unwrap());
    assert_eq!(first["db"][0]["op"], "GetItem", "db reads are recorded: {first}");

    let same = sspur_deploy::replay::replay(&recording, &v1, &b1).unwrap();
    assert_eq!((same.total, same.same, same.diffs.len()), (11, 11, 0), "{}", same.text());

    let (v2, b2) = build(&src("evolve/items_v2.ssp"));
    let ext = sspur_deploy::replay::replay(&recording, &v2, &b2).unwrap();
    assert!(ext.passed(false) && !ext.passed(true) && ext.extended > 0, "{}", ext.text());

    let changed = src("items.ssp").replace("= db.scan(Items).sort_by(_.id.raw)", "= db.scan(Items).sort_by(_.qty)").replace("= if not db.del(Items, ItemId(id)) then raise NotFound{id: id}", "= do\n  db.del(Items, ItemId(id))\n  ()");
    let (vc, bc) = build(&changed);
    let rep = sspur_deploy::replay::replay(&recording, &vc, &bc).unwrap();
    assert!(!rep.passed(false), "{}", rep.text());
    let what: Vec<(String, String)> = rep.diffs.iter().map(|d| (d.request.clone(), d.what.clone())).collect();
    assert!(what.contains(&("GET /items".into(), "response".into())), "{}", rep.text());
    assert!(what.contains(&("DELETE /items/c".into(), "status".into())), "{}", rep.text());
    assert_eq!(rep.same, 9, "{}", rep.text());
}

#[test]
fn rollout_and_backfill_artifacts() {
    let v3 = analyze(&src("evolve/items_v3.ssp")).unwrap();
    let p = plan(&v3).unwrap();
    for f in ["backfill.sh", "rollback.sh", "iam/backfill_Items.json"] {
        assert!(p.files.contains_key(f), "missing {f}");
    }
    let t: Value = serde_json::from_str(&p.files["template.json"]).unwrap();
    let bf = &t["Resources"]["BackfillItemsFunction"]["Properties"];
    assert_eq!(bf["Environment"]["Variables"], json(r#"{"SSPUR_BACKFILL":"Items","SSPUR_TABLE_Items":{"Ref":"ItemsTable"}}"#));
    assert!(t["Resources"].get("BackfillItemsAlias").is_none(), "the backfill function has no route and no traffic shift");
    let pol = json(&p.files["iam/backfill_Items.json"]);
    assert_eq!(pol["Statement"][0]["Action"], json(r#"["dynamodb:PutItem","dynamodb:Scan"]"#));
    assert!(p.files["rollback.sh"].contains("stop-deployment --deployment-id") && p.files["backfill.sh"].contains("BackfillItemsFunction"));
    let summary = json(&p.files["plan.json"]);
    assert_eq!(summary["stores"][0]["migrations"][0]["reverse"], "unmigrate_Items");
    if std::process::Command::new("cfn-lint").arg("--version").output().is_err() {
        return;
    }
    let f = tmp("lint3").join("template.json");
    std::fs::write(&f, &p.files["template.json"]).unwrap();
    let out = std::process::Command::new("cfn-lint").arg(&f).output().unwrap();
    assert!(out.status.success(), "cfn-lint failed:\n{}", String::from_utf8_lossy(&out.stdout));
}
