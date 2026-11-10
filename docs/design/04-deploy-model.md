# 04. Deploy Model

A product is a set of nodes. Infrastructure, permissions, migrations, and monitoring are derived from those nodes, not written separately.

## 1. Constructs

```
store Orders = table[OrderId, Order] keys(id) index(by_user: _.user)
store Stock  = table[Str, Int]
queue Ship   = fifo[Order] dlq(after: 5)

svc shop
  ep post "/orders" (o: Order) -> Order = place(o)
  ep get  "/orders/{id}" (id: OrderId) -> Opt[Order] = db.get(Orders, id)
  on Ship (o: Order) = ship(o)
  sched "rate(5m)" = reconcile()
  scale cpu 60%, min 2, max 50
  slo p99 < 200ms, avail 99.95%
```

| Construct | Meaning |
|---|---|
| `store` | Durable state: `table`, `blob`, `kv`, `log` |
| `queue` | Async messaging: `fifo`, `std`, `topic` |
| `svc` | Deployable unit of endpoints, consumers, and schedules |
| `ep` | Typed endpoint. Request and response codecs come from `derive Json` or `derive Proto` |
| `on` | Queue consumer |
| `sched` | Timer |
| `scale`, `slo` | Capacity policy and objectives. Alarms are generated from `slo` |

## 2. Effects are permissions

The deployer computes each handler's transitive effect row and emits exactly those grants. For the `post "/orders"` endpoint above, that's:

```
db.write[Orders], db.read[Stock], log
-> dynamodb:PutItem on Orders, dynamodb:GetItem on Stock, logs:PutLogEvents
```

Nothing more is ever granted. Widening a permission requires changing code, which is visible in the graph and in the audit view.

## 3. Targets

`sspur deploy plan` lowers a `svc` to a target backend. There is one backend, **AWS**: Lambda behind an HTTP API, DynamoDB, IAM, CloudWatch and CodeDeploy (ADR 0016). The backend interface is pluggable for other clouds and bare metal.

## 4. Typed evolution

- Every `store` schema is a type hash. A change that old items don't decode as (anything beyond new `Opt` fields and variants) requires a pure migration function, `fn migrate_Orders(old: OrderV1) -> Order`, matched to stored items by the schema hash of `OrderV1`. An optional `unmigrate_Orders` lets old and new handlers share the table. Handlers migrate old items on read, and a generated backfill rewrites them (ADR 0019).
- Every `ep` contract is versioned by hash. The compiler classifies each change as compatible or breaking, and breaking changes require a new path or an explicit `deprecate`.
- During rollout, old and new handlers run side by side, each pinned to its own root hash.

## 5. Hot swap and rollout

Deploying means pointing an environment at a new root. Rollout is staged by policy (canary percentage, bake time, SLO gates), and rollback means pointing back at the previous root, which is instant because the old artifacts are still cached. On AWS this is a retained Lambda version per root behind a `live` alias that CodeDeploy shifts and rolls back on alarms. `sspur deploy local` does the same in-process with `swap`, `promote` and `rollback` (ADR 0019).

## 6. Observability and replay

- Every effect invocation is logged with node hash, inputs (redacted by type annotation), and timing.
- `trace(req)` reconstructs a request as a path through nodes.
- `replay(req)` re-runs it locally under log handlers and is bit-for-bit deterministic (pillar 18). `sspur deploy replay` re-runs a `deploy local --record` file against a new version and reports every behavior difference, as a gate before a swap.
- Measured cost and latency flow back into node `meta.cost`.
