pub static GLOBALS: &[&str] = &[
    "log(msg: Str) -> Unit ! log",
    "some[A](x: A) -> Opt[A]",
    "ok[A, E](x: A) -> Res[A, E]",
    "err[A, E](e: E) -> Res[A, E]",
    "empty_map[K, V]() -> Map[K, V]",
    "min[A](a: A, b: A) -> A",
    "__cmp[A](a: A, b: A) -> Int",
    "__hash(s: Str) -> Int",
    "max[A](a: A, b: A) -> A",
    "secret[A](x: A) -> Secret[A]",
    "pii[A](x: A) -> Pii[A]",
    "untrusted[A](x: A) -> Untrusted[A]",
    "guess[A](x: A, conf: F64) -> Guess[A]",
    "atomic(x: Int) -> Atomic[Int] ! conc",
    "chan[A]() -> Chan[A] ! conc",
];

pub static METHODS: &[(&str, &str)] = &[
    ("List", "len[A](xs: List[A]) -> Int"),
    ("List", "is_empty[A](xs: List[A]) -> Bool"),
    ("List", "map[A, B, e](xs: List[A], f: A -> B ! e) -> List[B] ! e"),
    ("List", "flat_map[A, B, e](xs: List[A], f: A -> List[B] ! e) -> List[B] ! e"),
    ("List", "filter[A, e](xs: List[A], f: A -> Bool ! e) -> List[A] ! e"),
    ("List", "fold[A, B, e](xs: List[A], init: B, f: (B, A) -> B ! e) -> B ! e"),
    ("List", "any[A, e](xs: List[A], f: A -> Bool ! e) -> Bool ! e"),
    ("List", "all[A, e](xs: List[A], f: A -> Bool ! e) -> Bool ! e"),
    ("List", "find[A, e](xs: List[A], f: A -> Bool ! e) -> Opt[A] ! e"),
    ("List", "sort_by[A, K, e](xs: List[A], f: A -> K ! e) -> List[A] ! e"),
    ("List", "sum[A](xs: List[A]) -> A"),
    ("List", "push[A](xs: List[A], x: A) -> List[A]"),
    ("List", "concat[A](xs: List[A], ys: List[A]) -> List[A]"),
    ("List", "take[A](xs: List[A], n: Int) -> List[A]"),
    ("List", "drop[A](xs: List[A], n: Int) -> List[A]"),
    ("List", "reverse[A](xs: List[A]) -> List[A]"),
    ("List", "sort[A](xs: List[A]) -> List[A]"),
    ("List", "unique[A](xs: List[A]) -> List[A]"),
    ("List", "contains[A](xs: List[A], x: A) -> Bool"),
    ("List", "first[A](xs: List[A]) -> Opt[A]"),
    ("List", "last[A](xs: List[A]) -> Opt[A]"),
    ("List", "get[A](xs: List[A], i: Int) -> Opt[A]"),
    ("List", "min[A](xs: List[A]) -> Opt[A]"),
    ("List", "max[A](xs: List[A]) -> Opt[A]"),
    ("List", "counts[A](xs: List[A]) -> List[(A, Int)]"),
    ("List", "zip[A, B](xs: List[A], ys: List[B]) -> List[(A, B)]"),
    ("List", "enumerate[A](xs: List[A]) -> List[(Int, A)]"),
    ("List", "join(xs: List[Str], sep: Str) -> Str"),
    ("Str", "len(s: Str) -> Int"),
    ("Str", "is_empty(s: Str) -> Bool"),
    ("Str", "take(s: Str, n: Int) -> Str"),
    ("Str", "drop(s: Str, n: Int) -> Str"),
    ("Str", "reverse(s: Str) -> Str"),
    ("Str", "get(s: Str, i: Int) -> Opt[Str]"),
    ("Str", "first(s: Str) -> Opt[Str]"),
    ("Str", "last(s: Str) -> Opt[Str]"),
    ("Str", "lower(s: Str) -> Str"),
    ("Str", "upper(s: Str) -> Str"),
    ("Str", "trim(s: Str) -> Str"),
    ("Str", "split(s: Str, sep: Str) -> List[Str]"),
    ("Str", "words(s: Str) -> List[Str]"),
    ("Str", "chars(s: Str) -> List[Str]"),
    ("Str", "contains(s: Str, sub: Str) -> Bool"),
    ("Str", "starts_with(s: Str, p: Str) -> Bool"),
    ("Str", "ends_with(s: Str, p: Str) -> Bool"),
    ("Str", "replace(s: Str, from: Str, to: Str) -> Str"),
    ("Str", "repeat(s: Str, n: Int) -> Str"),
    ("Str", "to_int(s: Str) -> Opt[Int]"),
    ("Str", "is_alpha(s: Str) -> Bool"),
    ("Str", "byte_len(s: Str) -> Int"),
    ("Str", "byte(s: Str, i: Int) -> Int"),
    ("Opt", "or_[A](o: Opt[A], d: A) -> A"),
    ("Opt", "is_some[A](o: Opt[A]) -> Bool"),
    ("Opt", "get[A](o: Opt[A]) -> A"),
    ("Opt", "is_none[A](o: Opt[A]) -> Bool"),
    ("Opt", "map[A, B, e](o: Opt[A], f: A -> B ! e) -> Opt[B] ! e"),
    ("Opt", "ok_or[A, E](o: Opt[A], e: E) -> A ! fail[E]"),
    ("Res", "is_ok[A, E](r: Res[A, E]) -> Bool"),
    ("Res", "get[A, E](r: Res[A, E]) -> A ! fail[E]"),
    ("Map", "get[K, V](m: Map[K, V], k: K) -> Opt[V]"),
    ("Map", "put[K, V](m: Map[K, V], k: K, v: V) -> Map[K, V]"),
    ("Map", "remove[K, V](m: Map[K, V], k: K) -> Map[K, V]"),
    ("Map", "has[K, V](m: Map[K, V], k: K) -> Bool"),
    ("Map", "keys[K, V](m: Map[K, V]) -> List[K]"),
    ("Map", "values[K, V](m: Map[K, V]) -> List[V]"),
    ("Map", "items[K, V](m: Map[K, V]) -> List[(K, V)]"),
    ("Map", "len[K, V](m: Map[K, V]) -> Int"),
    ("Secret", "map[A, B](s: Secret[A], f: A -> B) -> Secret[B]"),
    ("Secret", "check[A](s: Secret[A], f: A -> Bool) -> Bool"),
    ("Secret", "expose[A](s: Secret[A], reason: Str) -> A"),
    ("Pii", "map[A, B](p: Pii[A], f: A -> B) -> Pii[B]"),
    ("Pii", "expose[A](p: Pii[A], reason: Str) -> A"),
    ("Untrusted", "validate[A, B](u: Untrusted[A], f: A -> Opt[B]) -> Opt[B]"),
    ("Untrusted", "trust[A](u: Untrusted[A], reason: Str) -> A"),
    ("Guess", "conf[A](g: Guess[A]) -> F64"),
    ("Guess", "map[A, B](g: Guess[A], f: A -> B) -> Guess[B]"),
    ("Guess", "verify[A](g: Guess[A], f: A -> Bool) -> Opt[A]"),
    ("Guess", "at_least[A](g: Guess[A], min: F64) -> Opt[A]"),
    ("Guess", "accept[A](g: Guess[A], reason: Str) -> A"),
    ("Int", "abs(n: Int) -> Int"),
    ("Int", "to_f64(n: Int) -> F64"),
    ("Int", "band(a: Int, b: Int) -> Int"),
    ("Int", "bor(a: Int, b: Int) -> Int"),
    ("Int", "bxor(a: Int, b: Int) -> Int"),
    ("Int", "shl(a: Int, n: Int) -> Int"),
    ("Int", "shr(a: Int, n: Int) -> Int"),
    ("F64", "abs(n: F64) -> F64"),
    ("F64", "round(n: F64) -> Int"),
    ("F64", "floor(n: F64) -> Int"),
    ("F64", "sqrt(n: F64) -> F64"),
    ("Atomic", "load(a: Atomic[Int]) -> Int ! conc"),
    ("Atomic", "store_(a: Atomic[Int], v: Int) -> Unit ! conc"),
    ("Atomic", "add(a: Atomic[Int], d: Int) -> Unit ! conc"),
    ("Atomic", "cas(a: Atomic[Int], expect: Int, value: Int) -> Bool ! conc"),
    ("Chan", "send[A](c: Chan[A], x: A) -> Unit ! conc"),
    ("Chan", "recv[A](c: Chan[A]) -> Opt[A] ! conc"),
    ("Chan", "close[A](c: Chan[A]) -> Unit ! conc"),
    ("*", "str[A](x: A) -> Str"),
];

pub static STD_TYPES: &[(&str, &str, usize)] = &[("Set", "#Set", 1), ("Heap", "#Heap", 1), ("StrBuf", "#StrBuf", 0), ("Time", "#Time", 0), ("Duration", "#Duration", 0), ("Bits", "#Bits", 0), ("HashMap", "#HashMap", 2), ("HashSet", "#HashSet", 1), ("BigInt", "#BigInt", 0), ("Dec", "#Dec", 0), ("Regex", "#Regex", 0), ("DevBuf", "#DevBuf", 1), ("Rng", "#Rng", 0), ("Complex", "#Complex", 0), ("FlatMap", "#FlatMap", 2), ("MdSpan", "#MdSpan", 1), ("Zone", "#Zone", 0), ("File", "#File", 0), ("View", "#View", 1), ("Ratio", "#Ratio", 0), ("Locale", "#Locale", 0), ("Request", "#Request", 0), ("Response", "#Response", 0), ("Server", "#Server", 0), ("HttpErr", "#HttpErr", 0)];

pub static STD_RECORDS: &[(&str, &str)] = &[("#Rng", "type Rng = {state: Int, gamma: Int}"), ("#Complex", "type Complex = {re: F64, im: F64}"), ("#FlatMap", "type FlatMap[K, V] = {keys: List[K], values: List[V]}"), ("#MdSpan", "type MdSpan[T] = {data: List[T], offset: Int, shape: List[Int], strides: List[Int]}"), ("#Zone", "type Zone = {name: Str, trans: List[Int], idx: List[Int], offs: List[Int], dst: List[Bool], abbrs: List[Str], tail: List[Int]}"), ("#File", "type File = {fd: Int, path: Str, dev: Int, ino: Int}"), ("#Ratio", "type Ratio = {num: BigInt, den: BigInt}"), ("#Locale", "type Locale = {tag: Str}"), ("#Request", "type Request = {method: Str, url: Str, headers: List[(Str, Str)], body: Str, timeout_ms: Int, retries: Int, redirects: Int}"), ("#Response", "type Response = {status: Int, headers: List[(Str, Str)], body: Str}"), ("#Server", "type Server = {port: Int, host: Str, workers: Int, max_body: Int, timeout_ms: Int, max_requests: Int, cors: List[Str], gzip: Bool, files: List[(Str, Str)], health: Str, cert: Str, key: Str, access_log: Bool}")];

/// Std records programs can build with a literal and update with `with`; the others are opaque.
pub static OPEN_STD_RECORDS: &[&str] = &["#Request", "#Response", "#Server"];

/// Std sum types, by internal name; their constructors are global unless a program defines its own.
pub static STD_SUMS: &[(&str, &str)] = &[("#HttpErr", "type HttpErr = Dns{msg: Str} | Connect{msg: Str} | Tls{msg: Str} | Timeout{msg: Str} | HttpStatus{code: Int, body: Str} | Io{msg: Str}")];

/// The database's types (ADR 0037), which the playground leaves out.
#[cfg(not(target_family = "wasm"))]
pub static LIB_TYPES: &[(&str, &str, usize)] = &[("Bytes", "#Bytes", 0), ("Db", "#Db", 0), ("DbErr", "#DbErr", 0), ("Proc", "#Proc", 0)];
#[cfg(not(target_family = "wasm"))]
pub static LIB_RECORDS: &[(&str, &str)] = &[("#Db", "type Db = {id: Int, path: Str}"), ("#Proc", "type Proc = {id: Int, pid: Int}")];
#[cfg(not(target_family = "wasm"))]
pub static LIB_SUMS: &[(&str, &str)] = &[("#DbErr", "type DbErr = Sql{msg: Str} | Constraint{msg: Str} | Busy{msg: Str} | NoRow | Mismatch{msg: Str}")];


/// One signature a line.
pub static STD_GLOBALS: &str = "\
dev_f32(xs: List[F64]) -> DevBuf[F32] ! dev\n\
dev_f64(xs: List[F64]) -> DevBuf[F64] ! dev\n\
dev_i32(xs: List[Int]) -> DevBuf[I32] ! dev\n\
dev_u32(xs: List[Int]) -> DevBuf[U32] ! dev\n\
dev_int(xs: List[Int]) -> DevBuf[Int] ! dev\n\
gpu_sync() -> Unit ! dev\n\
empty_set[A]() -> Set[A]\n\
empty_heap[A]() -> Heap[A]\n\
str_buf() -> StrBuf\n\
range(start: Int, end: Int, step: Int) -> List[Int]\n\
clamp[A](x: A, lo: A, hi: A) -> A\n\
rand(seed: Int) -> (Int, Int)\n\
rand_int(seed: Int, lo: Int, hi: Int) -> (Int, Int)\n\
rand_f64(seed: Int) -> (F64, Int)\n\
from_bytes(bs: List[Int]) -> Opt[Str]\n\
from_codes(cs: List[Int]) -> Opt[Str]\n\
read_file(path: Str) -> Res[Str, Str] ! fs\n\
write_file(path: Str, s: Str) -> Res[Unit, Str] ! fs\n\
append_file(path: Str, s: Str) -> Res[Unit, Str] ! fs\n\
list_dir(path: Str) -> Res[List[Str], Str] ! fs\n\
remove_file(path: Str) -> Res[Unit, Str] ! fs\n\
read_bytes(path: Str) -> Res[List[Int], Str] ! fs\n\
write_bytes(path: Str, bs: List[Int]) -> Res[Unit, Str] ! fs\n\
mkdir(path: Str) -> Res[Unit, Str] ! fs\n\
mkdir_all(path: Str) -> Res[Unit, Str] ! fs\n\
remove_dir(path: Str) -> Res[Unit, Str] ! fs\n\
rename(from: Str, to: Str) -> Res[Unit, Str] ! fs\n\
exists(path: Str) -> Bool ! fs\n\
is_dir(path: Str) -> Bool ! fs\n\
file_size(path: Str) -> Res[Int, Str] ! fs\n\
modified_ms(path: Str) -> Res[Int, Str] ! fs\n\
copy_file(from: Str, to: Str) -> Res[Unit, Str] ! fs\n\
symlink(target: Str, link: Str) -> Res[Unit, Str] ! fs\n\
read_link(path: Str) -> Res[Str, Str] ! fs\n\
is_symlink(path: Str) -> Bool ! fs\n\
file_mode(path: Str) -> Res[Int, Str] ! fs\n\
set_mode(path: Str, mode: Int) -> Res[Unit, Str] ! fs\n\
eprint(s: Str) -> Unit ! io\n\
run_cmd(prog: Str, args: List[Str], input: Str) -> Res[(Int, Str, Str), Str] ! proc\n\
exit(code: Int) -> Unit ! proc\n\
read_line() -> Opt[Str] ! io\n\
read_lines() -> List[Str] ! io\n\
now_ms() -> Int ! time\n\
mono_ns() -> Int ! time\n\
sleep_ms(ms: Int) -> Unit ! time\n\
env_var(name: Str) -> Opt[Str] ! env\n\
args() -> List[Str] ! env\n\
rand_normal(seed: Int, mean: F64, sd: F64) -> (F64, Int)\n\
rand_uniform(seed: Int, lo: F64, hi: F64) -> (F64, Int)\n\
rand_exp(seed: Int, rate: F64) -> (F64, Int)\n\
rand_bool(seed: Int, p: F64) -> (Bool, Int)\n\
rand_binomial(seed: Int, n: Int, p: F64) -> (Int, Int)\n\
rand_poisson(seed: Int, mean: F64) -> (Int, Int)\n\
rand_geometric(seed: Int, p: F64) -> (Int, Int)\n\
rand_gamma(seed: Int, shape: F64, scale: F64) -> (F64, Int)\n\
rand_beta(seed: Int, a: F64, b: F64) -> (F64, Int)\n\
rand_weighted(seed: Int, ws: List[F64]) -> (Int, Int)\n\
rng(seed: Int) -> Rng\n\
complex(re: F64, im: F64) -> Complex\n\
ratio(n: Int, d: Int) -> Ratio\n\
ratio_big(n: BigInt, d: BigInt) -> Ratio\n\
parse_ratio(s: Str) -> Opt[Ratio]\n\
locale(tag: Str) -> Opt[Locale]\n\
polar(r: F64, theta: F64) -> Complex\n\
empty_flat_map[K, V]() -> FlatMap[K, V]\n\
mdspan[T](data: List[T], shape: List[Int]) -> MdSpan[T]\n\
time_zone(name: Str) -> Res[Zone, Str] ! time\n\
local_zone() -> Res[Zone, Str] ! time\n\
fixed_zone(minutes: Int) -> Zone\n\
open_file(path: Str, mode: Str) -> Res[File, Str] ! fs\n\
iota(start: Int, end: Int) -> View[Int]\n\
iterate[A](x: A, f: A -> A, n: Int) -> View[A]\n\
with_file[A, e](path: Str, mode: Str, f: File -> A ! e) -> Res[A, Str] ! fs, e\n\
bits(n: Int) -> Bits\n\
regex(p: Str) -> Res[Regex, Str]\n\
big(n: Int) -> BigInt\n\
parse_big(s: Str) -> Opt[BigInt]\n\
decimal(s: Str) -> Opt[Dec]\n\
hash_map[K, V]() -> HashMap[K, V]\n\
hash_set[A]() -> HashSet[A]\n\
time_ms(ms: Int) -> Time\n\
date(y: Int, m: Int, d: Int) -> Opt[Time]\n\
datetime(y: Int, mo: Int, d: Int, h: Int, mi: Int, s: Int) -> Opt[Time]\n\
parse_time(s: Str) -> Opt[Time]\n\
now() -> Time ! time\n\
millis(n: Int) -> Duration\n\
secs(n: Int) -> Duration\n\
mins(n: Int) -> Duration\n\
hours(n: Int) -> Duration\n\
days(n: Int) -> Duration\n\
__net_fetch(method: Str, url: Str, headers: List[(Str, Str)], body: Str, timeout_ms: Int, retries: Int, redirects: Int, path: Str) -> (Int, Int, List[(Str, Str)], Str) ! net\n\
__net_serve[e](cfg: (Int, Str, Int, Int, Int, Int, List[Str], Bool, List[(Str, Str)], Str, Str, Str, Bool), h: (Str, Str, List[(Str, Str)], Str, Int) -> (Int, List[(Str, Str)], Str) ! e) -> Unit ! net, log, e\n\
pi() -> F64\n\
euler() -> F64\n\
inf() -> F64\n\
nan() -> F64";

/// `Bytes` and the primitives of the library prelude (ADR 0037), which the playground leaves out.
#[cfg(not(target_family = "wasm"))]
pub static LIB_GLOBALS: &str = "bytes(xs: List[Int]) -> Bytes\nfrom_hex(s: Str) -> Opt[Bytes]\nfrom_base64(s: Str) -> Opt[Bytes]\nfrom_base64url(s: Str) -> Opt[Bytes]\nread_bin(path: Str) -> Res[Bytes, Str] ! fs\nwrite_bin(path: Str, b: Bytes) -> Res[Unit, Str] ! fs\n__crypto_hash(alg: Int, data: Bytes) -> Bytes\n__crypto_hmac(alg: Int, key: Bytes, data: Bytes) -> Bytes\n__crypto_random(n: Int) -> Bytes ! rand\n__crypto_same(a: Bytes, b: Bytes) -> Bool\n__crypto_aead(seal: Bool, key: Bytes, nonce: Bytes, ad: Bytes, data: Bytes) -> Opt[Bytes]\n__crypto_argon2(alg: Int, pw: Bytes, salt: Bytes, key: Bytes, ad: Bytes, m: Int, t: Int, p: Int, n: Int) -> Opt[Bytes]\n__crypto_ed_public(seed: Bytes) -> Bytes\n__crypto_ed_sign(seed: Bytes, data: Bytes) -> Bytes\n__crypto_ed_verify(pk: Bytes, data: Bytes, sig: Bytes) -> Bool\n__zip_pack(fmt: Int, level: Int, data: Bytes) -> Bytes\n__zip_unpack(fmt: Int, data: Bytes) -> (Int, Bytes, Str)\n__zip_file(op: Int, src: Str, dst: Str) -> Str ! fs\n__csv_rows(text: Str) -> (Int, Str, List[List[Str]])\n__csv_text(rows: List[List[Str]]) -> Str\n__csv_open(path: Str) -> (Int, Str) ! fs\n__csv_next(h: Int, n: Int) -> (Int, Str, List[List[Str]]) ! fs\n__csv_close(h: Int) -> Unit ! fs\n__db_connect(path: Str) -> (Int, Int, Str) ! db\n__db_disconnect(id: Int) -> Unit ! db\n__db_exec(id: Int, sql: Str, ps: List[(Int, Int, F64, Str, Bytes)], want: Int) -> (Int, Str, List[Str], List[List[(Int, Int, F64, Str, Bytes)]], Int) ! db\n__db_tx(id: Int, op: Int) -> (Int, Str) ! db\n__proc_op(op: Int, id: Int, arg: Str) -> (Int, Str) ! proc";

/// `Bytes` (ADR 0037), which the playground leaves out with the libraries that need it.
#[cfg(not(target_family = "wasm"))]
pub static LIB_METHODS: &str = "Str to_bytes(s: Str) -> Bytes\n#Bytes len(b: Bytes) -> Int\n#Bytes is_empty(b: Bytes) -> Bool\n#Bytes get(b: Bytes, i: Int) -> Opt[Int]\n#Bytes slice(b: Bytes, from: Int, to: Int) -> Bytes\n#Bytes concat(b: Bytes, c: Bytes) -> Bytes\n#Bytes to_list(b: Bytes) -> List[Int]\n#Bytes to_str(b: Bytes) -> Opt[Str]\n#Bytes hex(b: Bytes) -> Str\n#Bytes base64(b: Bytes) -> Str\n#Bytes base64url(b: Bytes) -> Str\n#Request form(r: Request, name: Str) -> Opt[Str]\n#Request cookie(r: Request, name: Str) -> Opt[Str]\n#Proc line(p: Proc, timeout_ms: Int) -> Opt[Str] ! proc\n#Proc stop(p: Proc) -> Int ! proc\n#Proc wait(p: Proc) -> Int ! proc\n#Proc running(p: Proc) -> Bool ! proc";

/// `receiver signature` lines: text rather than a table of pointers, which the dynamic linker
/// would fix up at every start.
pub static STD_METHODS: &str = "\
#DevBuf len[T](b: DevBuf[T]) -> Int\n\
List binary_search[A](xs: List[A], x: A) -> Opt[Int]\n\
List lower_bound[A](xs: List[A], x: A) -> Int\n\
List sort_with[A, e](xs: List[A], f: (A, A) -> Int ! e) -> List[A] ! e\n\
List chunks[A](xs: List[A], n: Int) -> List[List[A]]\n\
List windows[A](xs: List[A], n: Int) -> List[List[A]]\n\
List group_by[A, K, e](xs: List[A], f: A -> K ! e) -> List[(K, List[A])] ! e\n\
List partition[A, e](xs: List[A], f: A -> Bool ! e) -> (List[A], List[A]) ! e\n\
List scan[A, B, e](xs: List[A], init: B, f: (B, A) -> B ! e) -> List[B] ! e\n\
List flatten[A](xs: List[List[A]]) -> List[A]\n\
List index_of[A](xs: List[A], x: A) -> Opt[Int]\n\
List find_index[A, e](xs: List[A], f: A -> Bool ! e) -> Opt[Int] ! e\n\
List slice[A](xs: List[A], from: Int, to: Int) -> List[A]\n\
List push_front[A](xs: List[A], x: A) -> List[A]\n\
List pop_front[A](xs: List[A]) -> Opt[(A, List[A])]\n\
List pop_back[A](xs: List[A]) -> Opt[(A, List[A])]\n\
List to_set[A](xs: List[A]) -> Set[A]\n\
List to_heap[A](xs: List[A]) -> Heap[A]\n\
List shuffle[A](xs: List[A], seed: Int) -> List[A]\n\
List take_while[A, e](xs: List[A], f: A -> Bool ! e) -> List[A] ! e\n\
List drop_while[A, e](xs: List[A], f: A -> Bool ! e) -> List[A] ! e\n\
List upper_bound[A](xs: List[A], x: A) -> Int\n\
List rotate[A](xs: List[A], k: Int) -> List[A]\n\
List merge[A](xs: List[A], ys: List[A]) -> List[A]\n\
List next_perm[A](xs: List[A]) -> Opt[List[A]]\n\
List choice[A](xs: List[A], seed: Int) -> Opt[A]\n\
Res is_err[A, E](r: Res[A, E]) -> Bool\n\
Res or_[A, E](r: Res[A, E], d: A) -> A\n\
Res map[A, B, E, e](r: Res[A, E], f: A -> B ! e) -> Res[B, E] ! e\n\
#Set add[A](s: Set[A], x: A) -> Set[A]\n\
#Set remove[A](s: Set[A], x: A) -> Set[A]\n\
#Set has[A](s: Set[A], x: A) -> Bool\n\
#Set len[A](s: Set[A]) -> Int\n\
#Set is_empty[A](s: Set[A]) -> Bool\n\
#Set items[A](s: Set[A]) -> List[A]\n\
#Set union[A](s: Set[A], t: Set[A]) -> Set[A]\n\
#Set inter[A](s: Set[A], t: Set[A]) -> Set[A]\n\
#Set diff[A](s: Set[A], t: Set[A]) -> Set[A]\n\
#Set min[A](s: Set[A]) -> Opt[A]\n\
#Set max[A](s: Set[A]) -> Opt[A]\n\
#Heap push[A](h: Heap[A], x: A) -> Heap[A]\n\
#Heap pop[A](h: Heap[A]) -> Opt[(A, Heap[A])]\n\
#Heap peek[A](h: Heap[A]) -> Opt[A]\n\
#Heap len[A](h: Heap[A]) -> Int\n\
#Heap is_empty[A](h: Heap[A]) -> Bool\n\
#Heap items[A](h: Heap[A]) -> List[A]\n\
#StrBuf add(b: StrBuf, s: Str) -> StrBuf\n\
#StrBuf byte_len(b: StrBuf) -> Int\n\
Str split_once(s: Str, sep: Str) -> Opt[(Str, Str)]\n\
Str index_of(s: Str, sub: Str) -> Opt[Int]\n\
Str pad_left(s: Str, n: Int, fill: Str) -> Str\n\
Str pad_right(s: Str, n: Int, fill: Str) -> Str\n\
Str to_f64(s: Str) -> Opt[F64]\n\
Str bytes(s: Str) -> List[Int]\n\
Str codes(s: Str) -> List[Int]\n\
F64 fmt(x: F64, digits: Int) -> Str\n\
F64 pow(x: F64, y: F64) -> F64\n\
F64 exp(x: F64) -> F64\n\
F64 ln(x: F64) -> F64\n\
F64 log2(x: F64) -> F64\n\
F64 log10(x: F64) -> F64\n\
F64 sin(x: F64) -> F64\n\
F64 cos(x: F64) -> F64\n\
F64 tan(x: F64) -> F64\n\
F64 asin(x: F64) -> F64\n\
F64 acos(x: F64) -> F64\n\
F64 atan(x: F64) -> F64\n\
F64 atan2(y: F64, x: F64) -> F64\n\
F64 hypot(x: F64, y: F64) -> F64\n\
F64 ceil(x: F64) -> Int\n\
F64 trunc(x: F64) -> Int\n\
F64 checked_round(x: F64) -> Opt[Int]\n\
F64 checked_floor(x: F64) -> Opt[Int]\n\
F64 checked_ceil(x: F64) -> Opt[Int]\n\
F64 checked_trunc(x: F64) -> Opt[Int]\n\
F64 is_nan(x: F64) -> Bool\n\
F64 is_finite(x: F64) -> Bool\n\
Int gcd(a: Int, b: Int) -> Int\n\
Int lcm(a: Int, b: Int) -> Int\n\
Int wrapping_add(a: Int, b: Int) -> Int\n\
Int wrapping_sub(a: Int, b: Int) -> Int\n\
Int wrapping_mul(a: Int, b: Int) -> Int\n\
Int checked_add(a: Int, b: Int) -> Opt[Int]\n\
Int checked_sub(a: Int, b: Int) -> Opt[Int]\n\
Int checked_mul(a: Int, b: Int) -> Opt[Int]\n\
Int checked_div(a: Int, b: Int) -> Opt[Int]\n\
Int bnot(a: Int) -> Int\n\
Int popcount(a: Int) -> Int\n\
Int clz(a: Int) -> Int\n\
Int ctz(a: Int) -> Int\n\
Int format(n: Int, spec: Str) -> Str\n\
F64 format(x: F64, spec: Str) -> Str\n\
Str format(s: Str, spec: Str) -> Str\n\
Str fold_case(s: Str) -> Str\n\
Str compare_ci(s: Str, t: Str) -> Int\n\
#HashMap get[K, V](m: HashMap[K, V], k: K) -> Opt[V]\n\
#HashMap put[K, V](m: HashMap[K, V], k: K, v: V) -> HashMap[K, V]\n\
#HashMap remove[K, V](m: HashMap[K, V], k: K) -> HashMap[K, V]\n\
#HashMap has[K, V](m: HashMap[K, V], k: K) -> Bool\n\
#HashMap keys[K, V](m: HashMap[K, V]) -> List[K]\n\
#HashMap values[K, V](m: HashMap[K, V]) -> List[V]\n\
#HashMap items[K, V](m: HashMap[K, V]) -> List[(K, V)]\n\
#HashMap len[K, V](m: HashMap[K, V]) -> Int\n\
#HashMap is_empty[K, V](m: HashMap[K, V]) -> Bool\n\
#HashSet add[A](s: HashSet[A], x: A) -> HashSet[A]\n\
#HashSet remove[A](s: HashSet[A], x: A) -> HashSet[A]\n\
#HashSet has[A](s: HashSet[A], x: A) -> Bool\n\
#HashSet len[A](s: HashSet[A]) -> Int\n\
#HashSet is_empty[A](s: HashSet[A]) -> Bool\n\
#HashSet items[A](s: HashSet[A]) -> List[A]\n\
#HashSet union[A](s: HashSet[A], t: HashSet[A]) -> HashSet[A]\n\
#HashSet inter[A](s: HashSet[A], t: HashSet[A]) -> HashSet[A]\n\
#HashSet diff[A](s: HashSet[A], t: HashSet[A]) -> HashSet[A]\n\
List to_hash_set[A](xs: List[A]) -> HashSet[A]\n\
List to_hash_map[K, V](xs: List[(K, V)]) -> HashMap[K, V]\n\
#BigInt add(a: BigInt, b: BigInt) -> BigInt\n\
#BigInt sub(a: BigInt, b: BigInt) -> BigInt\n\
#BigInt mul(a: BigInt, b: BigInt) -> BigInt\n\
#BigInt div(a: BigInt, b: BigInt) -> BigInt\n\
#BigInt rem(a: BigInt, b: BigInt) -> BigInt\n\
#BigInt divmod(a: BigInt, b: BigInt) -> (BigInt, BigInt)\n\
#BigInt pow(a: BigInt, e: Int) -> BigInt\n\
#BigInt neg(a: BigInt) -> BigInt\n\
#BigInt abs(a: BigInt) -> BigInt\n\
#BigInt sign(a: BigInt) -> Int\n\
#BigInt to_int(a: BigInt) -> Opt[Int]\n\
#BigInt to_f64(a: BigInt) -> F64\n\
#BigInt to_dec(a: BigInt) -> Dec\n\
#Dec add(a: Dec, b: Dec) -> Dec\n\
#Dec sub(a: Dec, b: Dec) -> Dec\n\
#Dec mul(a: Dec, b: Dec) -> Dec\n\
#Dec div(a: Dec, b: Dec, scale: Int) -> Dec\n\
#Dec round(a: Dec, scale: Int) -> Dec\n\
#Dec scale(a: Dec) -> Int\n\
#Dec neg(a: Dec) -> Dec\n\
#Dec abs(a: Dec) -> Dec\n\
#Dec sign(a: Dec) -> Int\n\
#Dec to_f64(a: Dec) -> F64\n\
#Regex is_match(r: Regex, s: Str) -> Bool\n\
#Regex find(r: Regex, s: Str) -> Opt[Str]\n\
#Regex span(r: Regex, s: Str) -> Opt[(Int, Int)]\n\
#Regex find_all(r: Regex, s: Str) -> List[Str]\n\
#Regex captures(r: Regex, s: Str) -> Opt[List[Str]]\n\
#Regex replace(r: Regex, s: Str, rep: Str) -> Str\n\
#Regex split(r: Regex, s: Str) -> List[Str]\n\
#Bits has(b: Bits, i: Int) -> Bool\n\
#Bits set(b: Bits, i: Int) -> Bits\n\
#Bits clear(b: Bits, i: Int) -> Bits\n\
#Bits flip(b: Bits, i: Int) -> Bits\n\
#Bits len(b: Bits) -> Int\n\
#Bits count(b: Bits) -> Int\n\
#Bits union(b: Bits, c: Bits) -> Bits\n\
#Bits inter(b: Bits, c: Bits) -> Bits\n\
#Bits diff(b: Bits, c: Bits) -> Bits\n\
#Bits xor(b: Bits, c: Bits) -> Bits\n\
#Bits flip_all(b: Bits) -> Bits\n\
#Bits items(b: Bits) -> List[Int]\n\
List to_bits(xs: List[Int], n: Int) -> Bits\n\
#Time unix_ms(t: Time) -> Int\n\
#Time year(t: Time) -> Int\n\
#Time month(t: Time) -> Int\n\
#Time day(t: Time) -> Int\n\
#Time hour(t: Time) -> Int\n\
#Time minute(t: Time) -> Int\n\
#Time second(t: Time) -> Int\n\
#Time milli(t: Time) -> Int\n\
#Time weekday(t: Time) -> Int\n\
#Time yday(t: Time) -> Int\n\
#Time date(t: Time) -> Time\n\
#Time iso(t: Time) -> Str\n\
#Time format(t: Time, pat: Str) -> Str\n\
#Time add(t: Time, d: Duration) -> Time\n\
#Time sub(t: Time, d: Duration) -> Time\n\
#Time since(t: Time, u: Time) -> Duration\n\
#Time add_months(t: Time, n: Int) -> Time\n\
#Time add_years(t: Time, n: Int) -> Time\n\
#Duration ms(d: Duration) -> Int\n\
#Duration secs(d: Duration) -> Int\n\
#Duration mins(d: Duration) -> Int\n\
#Duration hours(d: Duration) -> Int\n\
#Duration days(d: Duration) -> Int\n\
#Duration add(d: Duration, e: Duration) -> Duration\n\
#Duration sub(d: Duration, e: Duration) -> Duration\n\
#Duration mul(d: Duration, k: Int) -> Duration\n\
#Duration div(d: Duration, k: Int) -> Duration\n\
#Duration neg(d: Duration) -> Duration\n\
#Duration abs(d: Duration) -> Duration\n\
Int rotl(a: Int, n: Int) -> Int\n\
Int rotr(a: Int, n: Int) -> Int\n\
Int byteswap(a: Int) -> Int\n\
Int saturating_add(a: Int, b: Int) -> Int\n\
Int saturating_sub(a: Int, b: Int) -> Int\n\
Int saturating_mul(a: Int, b: Int) -> Int\n\
F64 sinh(x: F64) -> F64\n\
F64 cosh(x: F64) -> F64\n\
F64 tanh(x: F64) -> F64\n\
F64 asinh(x: F64) -> F64\n\
F64 acosh(x: F64) -> F64\n\
F64 atanh(x: F64) -> F64\n\
F64 cbrt(x: F64) -> F64\n\
F64 exp2(x: F64) -> F64\n\
F64 expm1(x: F64) -> F64\n\
F64 log1p(x: F64) -> F64\n\
F64 erf(x: F64) -> F64\n\
F64 erfc(x: F64) -> F64\n\
F64 gamma(x: F64) -> F64\n\
F64 lgamma(x: F64) -> F64\n\
F64 fmod(x: F64, y: F64) -> F64\n\
F64 remainder(x: F64, y: F64) -> F64\n\
F64 copysign(x: F64, y: F64) -> F64\n\
F64 nextafter(x: F64, y: F64) -> F64\n\
F64 fdim(x: F64, y: F64) -> F64\n\
F64 fma(x: F64, y: F64, z: F64) -> F64\n\
F64 is_inf(x: F64) -> Bool\n\
#Rng next(r: Rng) -> (Int, Rng)\n\
#Rng int(r: Rng, lo: Int, hi: Int) -> (Int, Rng)\n\
#Rng f64(r: Rng) -> (F64, Rng)\n\
#Rng normal(r: Rng, mean: F64, sd: F64) -> (F64, Rng)\n\
#Rng uniform(r: Rng, lo: F64, hi: F64) -> (F64, Rng)\n\
#Rng exp(r: Rng, rate: F64) -> (F64, Rng)\n\
#Rng bool(r: Rng, p: F64) -> (Bool, Rng)\n\
#Rng binomial(r: Rng, n: Int, p: F64) -> (Int, Rng)\n\
#Rng poisson(r: Rng, mean: F64) -> (Int, Rng)\n\
#Rng geometric(r: Rng, p: F64) -> (Int, Rng)\n\
#Rng gamma(r: Rng, shape: F64, scale: F64) -> (F64, Rng)\n\
#Rng beta(r: Rng, a: F64, b: F64) -> (F64, Rng)\n\
#Rng weighted(r: Rng, ws: List[F64]) -> (Int, Rng)\n\
#Rng split(r: Rng) -> (Rng, Rng)\n\
#Rng stream(r: Rng, k: Int) -> Rng\n\
#Complex add(z: Complex, w: Complex) -> Complex\n\
#Complex sub(z: Complex, w: Complex) -> Complex\n\
#Complex mul(z: Complex, w: Complex) -> Complex\n\
#Complex div(z: Complex, w: Complex) -> Complex\n\
#Complex pow(z: Complex, w: Complex) -> Complex\n\
#Complex neg(z: Complex) -> Complex\n\
#Complex conj(z: Complex) -> Complex\n\
#Complex exp(z: Complex) -> Complex\n\
#Complex ln(z: Complex) -> Complex\n\
#Complex sqrt(z: Complex) -> Complex\n\
#Complex sin(z: Complex) -> Complex\n\
#Complex cos(z: Complex) -> Complex\n\
#Complex tan(z: Complex) -> Complex\n\
#Complex sinh(z: Complex) -> Complex\n\
#Complex cosh(z: Complex) -> Complex\n\
#Complex tanh(z: Complex) -> Complex\n\
#Complex asin(z: Complex) -> Complex\n\
#Complex acos(z: Complex) -> Complex\n\
#Complex atan(z: Complex) -> Complex\n\
#Complex asinh(z: Complex) -> Complex\n\
#Complex acosh(z: Complex) -> Complex\n\
#Complex atanh(z: Complex) -> Complex\n\
#Complex abs(z: Complex) -> F64\n\
#Complex arg(z: Complex) -> F64\n\
#Complex norm(z: Complex) -> F64\n\
#Complex scale(z: Complex, k: F64) -> Complex\n\
#Ratio add(a: Ratio, b: Ratio) -> Ratio\n\
#Ratio sub(a: Ratio, b: Ratio) -> Ratio\n\
#Ratio mul(a: Ratio, b: Ratio) -> Ratio\n\
#Ratio div(a: Ratio, b: Ratio) -> Ratio\n\
#Ratio neg(a: Ratio) -> Ratio\n\
#Ratio abs(a: Ratio) -> Ratio\n\
#Ratio inv(a: Ratio) -> Ratio\n\
#Ratio pow(a: Ratio, e: Int) -> Ratio\n\
#Ratio sign(a: Ratio) -> Int\n\
#Ratio is_int(a: Ratio) -> Bool\n\
#Ratio floor(a: Ratio) -> BigInt\n\
#Ratio ceil(a: Ratio) -> BigInt\n\
#Ratio trunc(a: Ratio) -> BigInt\n\
#Ratio round(a: Ratio) -> BigInt\n\
#Ratio to_f64(a: Ratio) -> F64\n\
#Ratio to_dec(a: Ratio, scale: Int) -> Dec\n\
#Locale compare(l: Locale, a: Str, b: Str) -> Int\n\
#Locale sort(l: Locale, xs: List[Str]) -> List[Str]\n\
#Locale format_int(l: Locale, n: Int) -> Str\n\
#Locale format_f64(l: Locale, x: F64, digits: Int) -> Str\n\
#Locale format_dec(l: Locale, d: Dec) -> Str\n\
#Locale format_date(l: Locale, t: Time) -> Str\n\
#Locale format_date_long(l: Locale, t: Time) -> Str\n\
#Locale format_time(l: Locale, t: Time) -> Str\n\
List to_flat_map[K, V](xs: List[(K, V)]) -> FlatMap[K, V]\n\
#FlatMap get[K, V](m: FlatMap[K, V], k: K) -> Opt[V]\n\
#FlatMap has[K, V](m: FlatMap[K, V], k: K) -> Bool\n\
#FlatMap put[K, V](m: FlatMap[K, V], k: K, v: V) -> FlatMap[K, V]\n\
#FlatMap remove[K, V](m: FlatMap[K, V], k: K) -> FlatMap[K, V]\n\
#FlatMap len[K, V](m: FlatMap[K, V]) -> Int\n\
#FlatMap is_empty[K, V](m: FlatMap[K, V]) -> Bool\n\
#FlatMap items[K, V](m: FlatMap[K, V]) -> List[(K, V)]\n\
#FlatMap lower_bound[K, V](m: FlatMap[K, V], k: K) -> Int\n\
#FlatMap between[K, V](m: FlatMap[K, V], lo: K, hi: K) -> FlatMap[K, V]\n\
#MdSpan rank[T](s: MdSpan[T]) -> Int\n\
#MdSpan size[T](s: MdSpan[T]) -> Int\n\
#MdSpan get[T](s: MdSpan[T], idx: List[Int]) -> T\n\
#MdSpan set[T](s: MdSpan[T], idx: List[Int], x: T) -> MdSpan[T]\n\
#MdSpan slice[T](s: MdSpan[T], dim: Int, lo: Int, hi: Int) -> MdSpan[T]\n\
#MdSpan sub[T](s: MdSpan[T], i: Int) -> MdSpan[T]\n\
#MdSpan transpose[T](s: MdSpan[T]) -> MdSpan[T]\n\
#MdSpan to_list[T](s: MdSpan[T]) -> List[T]\n\
#Zone offset(z: Zone, t: Time) -> Duration\n\
#Zone abbr(z: Zone, t: Time) -> Str\n\
#Zone is_dst(z: Zone, t: Time) -> Bool\n\
#Zone local(z: Zone, t: Time) -> Time\n\
#Zone utc(z: Zone, wall: Time) -> Opt[Time]\n\
#Time format_in(t: Time, z: Zone, pat: Str) -> Str\n\
#Time iso_in(t: Time, z: Zone) -> Str\n\
List view[A](xs: List[A]) -> View[A]\n\
#View map[A, B](v: View[A], f: A -> B) -> View[B]\n\
#View filter[A](v: View[A], f: A -> Bool) -> View[A]\n\
#View take[A](v: View[A], n: Int) -> View[A]\n\
#View drop[A](v: View[A], n: Int) -> View[A]\n\
#View take_while[A](v: View[A], f: A -> Bool) -> View[A]\n\
#View drop_while[A](v: View[A], f: A -> Bool) -> View[A]\n\
#View enumerate[A](v: View[A]) -> View[(Int, A)]\n\
#View zip[A, B](v: View[A], w: View[B]) -> View[(A, B)]\n\
#View chain[A](v: View[A], w: View[A]) -> View[A]\n\
#View to_list[A](v: View[A]) -> List[A]\n\
#View fold[A, B](v: View[A], init: B, f: (B, A) -> B) -> B\n\
#View len[A](v: View[A]) -> Int\n\
#View sum(v: View[Int]) -> Int\n\
#View first[A](v: View[A]) -> Opt[A]\n\
#View find[A](v: View[A], f: A -> Bool) -> Opt[A]\n\
#View any[A](v: View[A], f: A -> Bool) -> Bool\n\
#View all[A](v: View[A], f: A -> Bool) -> Bool\n\
#File read(f: File, n: Int) -> Res[List[Int], Str] ! fs\n\
#File read_line(f: File) -> Res[Opt[Str], Str] ! fs\n\
#File read_all(f: File) -> Res[Str, Str] ! fs\n\
#File lines(f: File) -> Res[List[Str], Str] ! fs\n\
#File fold_lines[A, e](f: File, init: A, g: (A, Str) -> A ! e) -> Res[A, Str] ! fs, e\n\
#File write(f: File, s: Str) -> Res[Unit, Str] ! fs\n\
#File write_bytes(f: File, bs: List[Int]) -> Res[Unit, Str] ! fs\n\
#File seek(f: File, pos: Int) -> Res[Unit, Str] ! fs\n\
#File tell(f: File) -> Res[Int, Str] ! fs\n\
#File size(f: File) -> Res[Int, Str] ! fs\n\
#File close(f: File) -> Res[Unit, Str] ! fs\n\
#Request header(r: Request, name: Str) -> Opt[Str]\n\
#Request path(r: Request) -> Str\n\
#Request query(r: Request, name: Str) -> Opt[Str]\n\
#Response header(r: Response, name: Str) -> Opt[Str]";

pub static STD_GLOBAL_NAMES: &[&str] = &[
    "empty_set", "empty_heap", "str_buf", "range", "clamp", "rand", "rand_int", "rand_f64", "rand_binomial", "rand_poisson", "rand_geometric", "rand_gamma", "rand_beta", "rand_weighted", "rng", "complex", "polar", "ratio", "ratio_big", "parse_ratio", "locale", "empty_flat_map", "mdspan", "time_zone", "local_zone", "fixed_zone", "open_file", "with_file", "iota", "iterate", "from_bytes", "from_codes", "read_file", "write_file", "append_file", "list_dir", "remove_file", "read_line", "read_lines", "now_ms", "mono_ns", "sleep_ms", "env_var", "args", "pi", "euler", "inf", "nan", "rand_normal", "rand_uniform", "rand_exp", "rand_bool", "read_bytes", "write_bytes", "mkdir", "mkdir_all", "remove_dir", "rename", "exists", "is_dir", "file_size", "modified_ms", "copy_file", "symlink", "read_link", "is_symlink", "file_mode", "set_mode", "eprint", "run_cmd", "exit", "regex", "bits", "big", "parse_big", "decimal", "hash_map", "hash_set", "time_ms", "date", "datetime", "parse_time", "now", "millis", "secs", "mins", "hours", "days", "dev_f32", "dev_f64", "dev_i32", "dev_u32", "dev_int", "gpu_sync", "__net_fetch", "__net_serve", 
];

/// The names of `LIB_GLOBALS`.
#[cfg(not(target_family = "wasm"))]
pub static LIB_GLOBAL_NAMES: &str = "bytes from_hex from_base64 from_base64url read_bin write_bin __crypto_hash __crypto_hmac __crypto_random __crypto_same __crypto_aead __crypto_argon2 __crypto_ed_public __crypto_ed_sign __crypto_ed_verify __zip_pack __zip_unpack __zip_file __csv_rows __csv_text __csv_open __csv_next __csv_close __db_connect __db_disconnect __db_exec __db_tx __proc_op";

/// Whether `n` is a std global function's name.
pub fn is_std_global(n: &str) -> bool {
    #[cfg(not(target_family = "wasm"))]
    if LIB_GLOBAL_NAMES.split(' ').any(|x| x == n) {
        return true;
    }
    STD_GLOBAL_NAMES.contains(&n)
}

pub static SYS_GLOBALS: &[&str] = &[
    "alloc[T](n: Int, init: T) -> Ptr[T] ! unsafe",
    "free[T](p: Ptr[T]) -> Unit ! unsafe",
    "null[T]() -> Ptr[T]",
    "drop[A](x: A) -> Unit",
    "leak[A](x: A) -> Unit",
];

pub static SYS_METHODS: &[(&str, &str)] = &[
    ("Ptr", "load[T](p: Ptr[T], i: Int) -> T ! unsafe"),
    ("Ptr", "store_[T](p: Ptr[T], i: Int, x: T) -> Unit ! unsafe"),
    ("Ptr", "offset[T](p: Ptr[T], n: Int) -> Ptr[T] ! unsafe"),
    ("Ptr", "is_null[T](p: Ptr[T]) -> Bool"),
];

pub static BARE_GLOBALS: &[&str] = &[
    "halt(code: Int) -> Unit ! mmio",
    "wait_irq() -> Unit ! mmio",
    "irq_enable(n: Int) -> Unit ! mmio",
    "timer_start(ticks: Int) -> Unit ! mmio",
    "ticks() -> Int ! mmio",
    "tick_hz() -> Int",
    "start_core(id: Int) -> Bool ! mmio",
    "core_id() -> Int ! mmio",
    "arch() -> Str",
];

pub static BARE_METHODS: &[(&str, &str)] = &[
    ("Mmio", "read[W](r: Mmio[W]) -> Int ! mmio"),
    ("Mmio", "write[W](r: Mmio[W], v: Int) -> Unit ! mmio"),
];

pub static BARE_NAMES: &[&str] = &["halt", "wait_irq", "irq_enable", "timer_start", "ticks", "tick_hz", "arch", "start_core", "core_id"];

type Tys = (&'static str, &'static str, usize);
type Pair = (&'static str, &'static str);

/// The std types, with the libraries' outside the playground.
#[cfg(not(target_family = "wasm"))]
pub fn std_types() -> impl Iterator<Item = &'static Tys> + Clone {
    STD_TYPES.iter().chain(LIB_TYPES)
}

#[cfg(target_family = "wasm")]
pub fn std_types() -> std::slice::Iter<'static, Tys> {
    STD_TYPES.iter()
}

/// The std sums, with the libraries' outside the playground.
#[cfg(not(target_family = "wasm"))]
pub fn std_sums() -> impl Iterator<Item = &'static Pair> {
    STD_SUMS.iter().chain(LIB_SUMS)
}

#[cfg(target_family = "wasm")]
pub fn std_sums() -> std::slice::Iter<'static, Pair> {
    STD_SUMS.iter()
}

/// The std methods, with `Bytes` outside the playground. (The library tables are text, so they
/// add no pointers for the dynamic linker to fix up at every start.)
#[cfg(not(target_family = "wasm"))]
pub fn std_methods() -> impl Iterator<Item = (&'static str, &'static str)> {
    STD_METHODS.lines().chain(LIB_METHODS.lines()).filter_map(|l| l.split_once(' '))
}

#[cfg(target_family = "wasm")]
pub fn std_methods() -> impl Iterator<Item = (&'static str, &'static str)> {
    STD_METHODS.lines().filter_map(|l| l.split_once(' '))
}

/// The std globals, with `Bytes` and the libraries' primitives outside the playground.
#[cfg(not(target_family = "wasm"))]
pub fn std_globals() -> impl Iterator<Item = &'static str> {
    STD_GLOBALS.lines().chain(LIB_GLOBALS.lines())
}

#[cfg(target_family = "wasm")]
pub fn std_globals() -> impl Iterator<Item = &'static str> {
    STD_GLOBALS.lines()
}
