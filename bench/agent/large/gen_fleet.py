"""Generate the fleet codebase (start state) in SSPUR, Python, TypeScript and Go.

  python3 gen_fleet.py OUT   -> OUT/start.ssp, OUT/py/{fleet,tests}, OUT/ts/{fleet,tests,tsconfig.json}, OUT/go/{go.mod,fleet}

A logistics fleet: a common module (fuel as a sum type, costs per km, tolls, distances, travel
times, clock formatting, zones, surcharges), 20 vehicle modules (truck, van, bus, ...: a record
type, 22 functions and 4 tests each) and 16 delivery-job modules (courier, freight, ...: a
record type, 21 functions and 4 tests each). Five vehicle kinds haul by weight and four job
kinds run at night, so those common helpers have a handful of callers. The same definitions,
sample data and test values in every language.
"""
import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import GOMOD, camel, cap, fill, gofmt, q, ts_tsconfig, write  # noqa: E402

VEHICLES = ["truck", "van", "bus", "minibus", "tanker", "trailer", "pickup", "flatbed", "reefer", "tipper",
            "crane", "forklift", "tractor", "ambulance", "coach", "taxi", "scooter", "cargo_bike", "camper", "sweeper"]
JOBS = ["courier", "freight", "express", "bulk", "medical", "postal", "grocery", "furniture", "retail", "parcel",
        "pharma", "catering", "laundry", "waste", "recycling", "removal"]
MODULES = VEHICLES + JOBS
HAULERS = ["truck", "tanker", "flatbed", "reefer", "tipper"]
NIGHT = ["postal", "medical", "pharma", "waste"]
CLIENTS = ["Acme", "Birch", "Cobalt", "Delta", "Ember", "Fjord", "Granite", "Helix", "Iris", "Juniper", "Kestrel", "Lumen", "Maple", "Nimbus"]


def vehicle_params(i, d):
    p = d.replace("_", "")[:2].upper()
    # (id, plate, fuel, odometer, capacity, km driven since)
    rows = [(1, f"{p}-{300 + i}", "Diesel", 12000 + 100 * i, 10 + i % 5, 0),
            (2, f"{p}-{100 + i}", "Electric", 3000 + i, 6, 400),
            (3, f"{p}-{200 + i}", "Petrol", 40000, 8 + i % 3, 16000)]
    return rows


def job_params(i):
    speed = 40 + 10 * (i % 5)
    base = 500 + 50 * (i % 4)
    rate = 20 + 5 * (i % 3)
    a, b = CLIENTS[i % len(CLIENTS)], CLIENTS[(i + 5) % len(CLIENTS)]
    rows = [(1, b, 0, 120 + 10 * i, 480), (2, a, 300, 40, 600), (3, b, 50, 700, 1380)]
    return speed, base, rate, rows


def enum(lang, name):
    return {"sspur": name, "python": f"Fuel.{name.upper()}", "ts": q(name.lower()), "go": name}[lang]


def fname(lang, d, f):
    return f"{d}_{f}" if lang in ("sspur", "python") else camel(d) + cap(f)


def vehicle_rows(lang, d, rows):
    out = []
    for a, p, f, odo, capa, km in rows:
        e = f"{fname(lang, d, 'new')}({a}, {q(p)}, {enum(lang, f)}, {odo}, {capa})"
        if km:
            e = f"{fname(lang, d, 'drive')}({e}, {km})"
        out.append(e)
    return ", ".join(out)


def job_rows(lang, d, rows):
    return ", ".join(f"{fname(lang, d, 'new')}({a}, {q(c)}, {o}, {t}, {s})" for a, c, o, t, s in rows)


def strs(lang, xs):
    s = ", ".join(q(x) for x in xs)
    return "[]string{" + s + "}" if lang == "go" else "[" + s + "]"


def vkw(i, d):
    rows = vehicle_params(i, d)
    return rows, sorted(r[1] for r in rows)


def jkw(i):
    speed, base, rate, rows = job_params(i)
    return dict(speed=speed, base=base, rate=rate, price10=base + 10 * rate, night10=(base + 10 * rate) * 150 // 100,
                clients=sorted(set(r[1] for r in rows))), rows


# ---- SSPUR ------------------------------------------------------------------------------------

SSP_COMMON = '''type Fuel = Diesel | Petrol | Electric

fn fuel_name(f: Fuel) -> Str
= match f
  | Diesel => "diesel"
  | Petrol => "petrol"
  | Electric => "electric"

fn cost_per_km(f: Fuel) -> Int
= match f
  | Diesel => 12
  | Petrol => 15
  | Electric => 5

fn co2_per_km(f: Fuel) -> Int
= match f
  | Diesel => 180
  | Petrol => 210
  | Electric => 0

fn service_interval() -> Int
= 15000

fn toll(km: Int) -> Int
= km / 100 * 250

fn distance(a: Int, b: Int) -> Int
= (a - b).abs

fn eta_minutes(km: Int, speed: Int) -> Int
= if speed <= 0 then 0 else km * 60 / speed

fn hhmm(m: Int) -> Str
= "{m / 60}:" + (m % 60).str.pad_left(2, "0")

fn zone_of(pos: Int) -> Str
= if pos < 100 then "A" else if pos < 500 then "B" else "C"

fn weight_surcharge(tons: Int) -> Int
= if tons <= 10 then 0 else (tons - 10) * 400

fn night_rate(start: Int) -> Int
= if start < 360 or start >= 1320 then 150 else 100

fn km_str(km: Int) -> Str
= "{km} km"

fn join_lines(xs: List[Str]) -> Str
= xs.join("\\n")

fn fuels() -> List[Fuel]
= [Diesel, Petrol, Electric]

test toll_ok = toll(300) == 750 and toll(0) == 0

test eta_ok = eta_minutes(30, 60) == 30 and eta_minutes(5, 0) == 0

test hhmm_ok = hhmm(605) == "10:05" and hhmm(59) == "0:59"

test zone_ok = zone_of(99) == "A" and zone_of(100) == "B" and zone_of(900) == "C"
'''

SSP_VEHICLE = '''type @T@ = {id: Int, plate: Str, fuel: Fuel, odometer: Int, serviced: Int, capacity: Int, load: Int, active: Bool}

fn @d@_new(id: Int, plate: Str, fuel: Fuel, odometer: Int, capacity: Int) -> @T@
= @T@{id, plate, fuel, odometer, serviced: odometer, capacity, load: 0, active: true}

fn @d@_drive(x: @T@, km: Int) -> @T@
= x with odometer := x.odometer + km

fn @d@_service(x: @T@) -> @T@
= x with serviced := x.odometer

fn @d@_since_service(x: @T@) -> Int
= x.odometer - x.serviced

fn @d@_due_service(x: @T@) -> Bool
= @d@_since_service(x) >= service_interval()

fn @d@_load_up(x: @T@, n: Int) -> @T@
= x with load := min(x.capacity, x.load + n)

fn @d@_unload(x: @T@) -> @T@
= x with load := 0

fn @d@_free(x: @T@) -> Int
= x.capacity - x.load

fn @d@_is_full(x: @T@) -> Bool
= x.load >= x.capacity

fn @d@_trip_cost(x: @T@, km: Int) -> Int
= km * cost_per_km(x.fuel) + toll(km)

fn @d@_trip_co2(x: @T@, km: Int) -> Int
= km * co2_per_km(x.fuel)

fn @d@_label(x: @T@) -> Str
= "{x.plate} ({fuel_name(x.fuel)})"

fn @d@_line(x: @T@) -> Str
= "{@d@_label(x)}: {km_str(x.odometer)}, load {x.load}/{x.capacity}"

fn @d@_report(xs: List[@T@]) -> Str
= join_lines(xs.map(@d@_line))

fn @d@_capacity(xs: List[@T@]) -> Int
= xs.map(_.capacity).sum

fn @d@_active_count(xs: List[@T@]) -> Int
= xs.filter(_.active).len

fn @d@_find(xs: List[@T@], id: Int) -> Opt[@T@]
= xs.find(_.id == id)

fn @d@_plates(xs: List[@T@]) -> List[Str]
= xs.map(_.plate).sort

fn @d@_of_fuel(xs: List[@T@], f: Fuel) -> List[@T@]
= xs.filter(_.fuel == f)

fn @d@_due_list(xs: List[@T@]) -> List[@T@]
= xs.filter(@d@_due_service)

fn @d@_retire(x: @T@) -> @T@
= x with active := false

fn @d@_sample() -> List[@T@]
= [@rows@]

test @d@_drive_ok = @d@_since_service(@d@_drive(@d@_new(1, "P", Diesel, 1000, 10), 250)) == 250

test @d@_cost_ok = @d@_trip_cost(@d@_new(1, "P", Diesel, 0, 10), 200) == 2900

test @d@_due_ok = @d@_due_service(@d@_drive(@d@_new(1, "P", Petrol, 0, 5), 15000)) and not @d@_due_service(@d@_new(1, "P", Petrol, 0, 5))

test @d@_plates_ok = @d@_plates(@d@_sample()) == @plates@
'''

SSP_HAUL = '''fn @d@_haul_cost(x: @T@, km: Int, tons: Int) -> Int
= @d@_trip_cost(x, km) + weight_surcharge(tons)

test @d@_haul_ok = @d@_haul_cost(@d@_new(1, "P", Diesel, 0, 20), 100, 12) == 2250
'''

SSP_JOB = '''type @T@ = {id: Int, client: Str, origin: Int, dest: Int, start: Int, speed: Int, done: Bool}

fn @d@_new(id: Int, client: Str, origin: Int, dest: Int, start: Int) -> @T@
= @T@{id, client, origin, dest, start, speed: @speed@, done: false}

fn @d@_km(x: @T@) -> Int
= distance(x.origin, x.dest)

fn @d@_minutes(x: @T@) -> Int
= eta_minutes(@d@_km(x), x.speed)

fn @d@_arrival(x: @T@) -> Int
= x.start + @d@_minutes(x)

fn @d@_arrival_str(x: @T@) -> Str
= hhmm(@d@_arrival(x))

fn @d@_late(x: @T@, deadline: Int) -> Bool
= @d@_arrival(x) > deadline

fn @d@_price(x: @T@) -> Int
= @base@ + @d@_km(x) * @rate@

fn @d@_zone(x: @T@) -> Str
= zone_of(x.dest)

fn @d@_complete(x: @T@) -> @T@
= x with done := true

fn @d@_reschedule(x: @T@, start: Int) -> @T@
= x with start := start

fn @d@_label(x: @T@) -> Str
= "{x.client} #{x.id}"

fn @d@_line(x: @T@) -> Str
= "{@d@_label(x)}: {km_str(@d@_km(x))} to zone {@d@_zone(x)}, arrives {@d@_arrival_str(x)}"

fn @d@_board(xs: List[@T@]) -> Str
= join_lines(xs.map(@d@_line))

fn @d@_pending(xs: List[@T@]) -> List[@T@]
= xs.filter(x => not x.done)

fn @d@_total_km(xs: List[@T@]) -> Int
= xs.map(@d@_km).sum

fn @d@_revenue(xs: List[@T@]) -> Int
= xs.map(@d@_price).sum

fn @d@_find(xs: List[@T@], id: Int) -> Opt[@T@]
= xs.find(_.id == id)

fn @d@_clients(xs: List[@T@]) -> List[Str]
= xs.map(_.client).unique.sort

fn @d@_in_zone(xs: List[@T@], z: Str) -> List[@T@]
= xs.filter(x => @d@_zone(x) == z)

fn @d@_longest(xs: List[@T@]) -> Opt[@T@]
= xs.sort_by(x => -@d@_km(x)).first

fn @d@_sample() -> List[@T@]
= [@rows@]

test @d@_km_ok = @d@_km(@d@_new(1, "c", 300, 120, 0)) == 180

test @d@_arrival_ok = @d@_arrival_str(@d@_new(1, "c", 0, @speed@, 480)) == "9:00"

test @d@_price_ok = @d@_price(@d@_new(1, "c", 0, 10, 0)) == @price10@

test @d@_clients_ok = @d@_clients(@d@_sample()) == @clients@
'''

SSP_NIGHT = '''fn @d@_night_price(x: @T@) -> Int
= @d@_price(x) * night_rate(x.start) / 100

test @d@_night_ok = @d@_night_price(@d@_new(1, "c", 0, 10, 1400)) == @night10@ and @d@_night_price(@d@_new(1, "c", 0, 10, 600)) == @price10@
'''


def ssp():
    out = [SSP_COMMON]
    for i, d in enumerate(VEHICLES):
        rows, plates = vkw(i, d)
        out.append(fill(SSP_VEHICLE, T=cap(d), d=d, rows=vehicle_rows("sspur", d, rows), plates=strs("sspur", plates)))
        if d in HAULERS:
            out.append(fill(SSP_HAUL, T=cap(d), d=d))
    for i, d in enumerate(JOBS):
        kw, rows = jkw(i)
        clients = strs("sspur", kw.pop("clients"))
        out.append(fill(SSP_JOB, T=cap(d), d=d, rows=job_rows("sspur", d, rows), clients=clients, **kw))
        if d in NIGHT:
            out.append(fill(SSP_NIGHT, T=cap(d), d=d, price10=kw["price10"], night10=kw["night10"]))
    return "\n".join(out)


# ---- Python -----------------------------------------------------------------------------------

PY_COMMON = '''from enum import Enum
from typing import List


class Fuel(Enum):
    DIESEL = "diesel"
    PETROL = "petrol"
    ELECTRIC = "electric"


def fuel_name(f: Fuel) -> str:
    return f.value


def cost_per_km(f: Fuel) -> int:
    return {Fuel.DIESEL: 12, Fuel.PETROL: 15, Fuel.ELECTRIC: 5}[f]


def co2_per_km(f: Fuel) -> int:
    return {Fuel.DIESEL: 180, Fuel.PETROL: 210, Fuel.ELECTRIC: 0}[f]


def service_interval() -> int:
    return 15000


def toll(km: int) -> int:
    return km // 100 * 250


def distance(a: int, b: int) -> int:
    return abs(a - b)


def eta_minutes(km: int, speed: int) -> int:
    if speed <= 0:
        return 0
    return km * 60 // speed


def hhmm(m: int) -> str:
    return f"{m // 60}:{m % 60:02d}"


def zone_of(pos: int) -> str:
    if pos < 100:
        return "A"
    if pos < 500:
        return "B"
    return "C"


def weight_surcharge(tons: int) -> int:
    if tons <= 10:
        return 0
    return (tons - 10) * 400


def night_rate(start: int) -> int:
    return 150 if start < 360 or start >= 1320 else 100


def km_str(km: int) -> str:
    return f"{km} km"


def join_lines(xs: List[str]) -> str:
    return "\\n".join(xs)


def fuels() -> List[Fuel]:
    return list(Fuel)
'''

PY_COMMON_TEST = '''from fleet.common import *


def test_toll_ok():
    assert toll(300) == 750 and toll(0) == 0


def test_eta_ok():
    assert eta_minutes(30, 60) == 30 and eta_minutes(5, 0) == 0


def test_hhmm_ok():
    assert hhmm(605) == "10:05" and hhmm(59) == "0:59"


def test_zone_ok():
    assert zone_of(99) == "A" and zone_of(100) == "B" and zone_of(900) == "C"
'''

PY_VEHICLE = '''from dataclasses import dataclass, replace
from typing import List, Optional

from fleet.common import Fuel, co2_per_km, cost_per_km, fuel_name, join_lines, km_str, service_interval, toll@imports@


@dataclass(frozen=True)
class @T@:
    id: int
    plate: str
    fuel: Fuel
    odometer: int
    serviced: int
    capacity: int
    load: int
    active: bool


def @d@_new(id: int, plate: str, fuel: Fuel, odometer: int, capacity: int) -> @T@:
    return @T@(id, plate, fuel, odometer, odometer, capacity, 0, True)


def @d@_drive(x: @T@, km: int) -> @T@:
    return replace(x, odometer=x.odometer + km)


def @d@_service(x: @T@) -> @T@:
    return replace(x, serviced=x.odometer)


def @d@_since_service(x: @T@) -> int:
    return x.odometer - x.serviced


def @d@_due_service(x: @T@) -> bool:
    return @d@_since_service(x) >= service_interval()


def @d@_load_up(x: @T@, n: int) -> @T@:
    return replace(x, load=min(x.capacity, x.load + n))


def @d@_unload(x: @T@) -> @T@:
    return replace(x, load=0)


def @d@_free(x: @T@) -> int:
    return x.capacity - x.load


def @d@_is_full(x: @T@) -> bool:
    return x.load >= x.capacity


def @d@_trip_cost(x: @T@, km: int) -> int:
    return km * cost_per_km(x.fuel) + toll(km)


def @d@_trip_co2(x: @T@, km: int) -> int:
    return km * co2_per_km(x.fuel)


def @d@_label(x: @T@) -> str:
    return f"{x.plate} ({fuel_name(x.fuel)})"


def @d@_line(x: @T@) -> str:
    return f"{@d@_label(x)}: {km_str(x.odometer)}, load {x.load}/{x.capacity}"


def @d@_report(xs: List[@T@]) -> str:
    return join_lines([@d@_line(x) for x in xs])


def @d@_capacity(xs: List[@T@]) -> int:
    return sum(x.capacity for x in xs)


def @d@_active_count(xs: List[@T@]) -> int:
    return len([x for x in xs if x.active])


def @d@_find(xs: List[@T@], id: int) -> Optional[@T@]:
    return next((x for x in xs if x.id == id), None)


def @d@_plates(xs: List[@T@]) -> List[str]:
    return sorted(x.plate for x in xs)


def @d@_of_fuel(xs: List[@T@], f: Fuel) -> List[@T@]:
    return [x for x in xs if x.fuel == f]


def @d@_due_list(xs: List[@T@]) -> List[@T@]:
    return [x for x in xs if @d@_due_service(x)]


def @d@_retire(x: @T@) -> @T@:
    return replace(x, active=False)


def @d@_sample() -> List[@T@]:
    return [@rows@]
'''

PY_HAUL = '''

def @d@_haul_cost(x: @T@, km: int, tons: int) -> int:
    return @d@_trip_cost(x, km) + weight_surcharge(tons)
'''

PY_VEHICLE_TEST = '''from fleet.common import Fuel
from fleet.@d@ import *


def test_@d@_drive_ok():
    assert @d@_since_service(@d@_drive(@d@_new(1, "P", Fuel.DIESEL, 1000, 10), 250)) == 250


def test_@d@_cost_ok():
    assert @d@_trip_cost(@d@_new(1, "P", Fuel.DIESEL, 0, 10), 200) == 2900


def test_@d@_due_ok():
    assert @d@_due_service(@d@_drive(@d@_new(1, "P", Fuel.PETROL, 0, 5), 15000)) and not @d@_due_service(@d@_new(1, "P", Fuel.PETROL, 0, 5))


def test_@d@_plates_ok():
    assert @d@_plates(@d@_sample()) == @plates@
'''

PY_HAUL_TEST = '''

def test_@d@_haul_ok():
    assert @d@_haul_cost(@d@_new(1, "P", Fuel.DIESEL, 0, 20), 100, 12) == 2250
'''

PY_JOB = '''from dataclasses import dataclass, replace
from typing import List, Optional

from fleet.common import distance, eta_minutes, hhmm, join_lines, km_str, @imports@zone_of


@dataclass(frozen=True)
class @T@:
    id: int
    client: str
    origin: int
    dest: int
    start: int
    speed: int
    done: bool


def @d@_new(id: int, client: str, origin: int, dest: int, start: int) -> @T@:
    return @T@(id, client, origin, dest, start, @speed@, False)


def @d@_km(x: @T@) -> int:
    return distance(x.origin, x.dest)


def @d@_minutes(x: @T@) -> int:
    return eta_minutes(@d@_km(x), x.speed)


def @d@_arrival(x: @T@) -> int:
    return x.start + @d@_minutes(x)


def @d@_arrival_str(x: @T@) -> str:
    return hhmm(@d@_arrival(x))


def @d@_late(x: @T@, deadline: int) -> bool:
    return @d@_arrival(x) > deadline


def @d@_price(x: @T@) -> int:
    return @base@ + @d@_km(x) * @rate@


def @d@_zone(x: @T@) -> str:
    return zone_of(x.dest)


def @d@_complete(x: @T@) -> @T@:
    return replace(x, done=True)


def @d@_reschedule(x: @T@, start: int) -> @T@:
    return replace(x, start=start)


def @d@_label(x: @T@) -> str:
    return f"{x.client} #{x.id}"


def @d@_line(x: @T@) -> str:
    return f"{@d@_label(x)}: {km_str(@d@_km(x))} to zone {@d@_zone(x)}, arrives {@d@_arrival_str(x)}"


def @d@_board(xs: List[@T@]) -> str:
    return join_lines([@d@_line(x) for x in xs])


def @d@_pending(xs: List[@T@]) -> List[@T@]:
    return [x for x in xs if not x.done]


def @d@_total_km(xs: List[@T@]) -> int:
    return sum(@d@_km(x) for x in xs)


def @d@_revenue(xs: List[@T@]) -> int:
    return sum(@d@_price(x) for x in xs)


def @d@_find(xs: List[@T@], id: int) -> Optional[@T@]:
    return next((x for x in xs if x.id == id), None)


def @d@_clients(xs: List[@T@]) -> List[str]:
    return sorted(set(x.client for x in xs))


def @d@_in_zone(xs: List[@T@], z: str) -> List[@T@]:
    return [x for x in xs if @d@_zone(x) == z]


def @d@_longest(xs: List[@T@]) -> Optional[@T@]:
    return max(xs, key=@d@_km, default=None)


def @d@_sample() -> List[@T@]:
    return [@rows@]
'''

PY_NIGHT = '''

def @d@_night_price(x: @T@) -> int:
    return @d@_price(x) * night_rate(x.start) // 100
'''

PY_JOB_TEST = '''from fleet.@d@ import *


def test_@d@_km_ok():
    assert @d@_km(@d@_new(1, "c", 300, 120, 0)) == 180


def test_@d@_arrival_ok():
    assert @d@_arrival_str(@d@_new(1, "c", 0, @speed@, 480)) == "9:00"


def test_@d@_price_ok():
    assert @d@_price(@d@_new(1, "c", 0, 10, 0)) == @price10@


def test_@d@_clients_ok():
    assert @d@_clients(@d@_sample()) == @clients@
'''

PY_NIGHT_TEST = '''

def test_@d@_night_ok():
    assert @d@_night_price(@d@_new(1, "c", 0, 10, 1400)) == @night10@ and @d@_night_price(@d@_new(1, "c", 0, 10, 600)) == @price10@
'''


def py(root):
    write(os.path.join(root, "fleet", "__init__.py"), "")
    write(os.path.join(root, "fleet", "common.py"), PY_COMMON)
    write(os.path.join(root, "tests", "test_common.py"), PY_COMMON_TEST)
    for i, d in enumerate(VEHICLES):
        rows, plates = vkw(i, d)
        src = fill(PY_VEHICLE, T=cap(d), d=d, rows=vehicle_rows("python", d, rows), imports=", weight_surcharge" if d in HAULERS else "")
        test = fill(PY_VEHICLE_TEST, d=d, plates=strs("python", plates))
        if d in HAULERS:
            src += fill(PY_HAUL, T=cap(d), d=d)
            test += fill(PY_HAUL_TEST, d=d)
        write(os.path.join(root, "fleet", d + ".py"), src)
        write(os.path.join(root, "tests", f"test_{d}.py"), test)
    for i, d in enumerate(JOBS):
        kw, rows = jkw(i)
        clients = strs("python", kw.pop("clients"))
        src = fill(PY_JOB, T=cap(d), d=d, rows=job_rows("python", d, rows), speed=kw["speed"], base=kw["base"], rate=kw["rate"],
                   imports="night_rate, " if d in NIGHT else "")
        test = fill(PY_JOB_TEST, d=d, speed=kw["speed"], price10=kw["price10"], clients=clients)
        if d in NIGHT:
            src += fill(PY_NIGHT, T=cap(d), d=d)
            test += fill(PY_NIGHT_TEST, d=d, price10=kw["price10"], night10=kw["night10"])
        write(os.path.join(root, "fleet", d + ".py"), src)
        write(os.path.join(root, "tests", f"test_{d}.py"), test)


# ---- TypeScript -------------------------------------------------------------------------------

TS_COMMON = '''export type Fuel = "diesel" | "petrol" | "electric";

export function fuelName(f: Fuel): string {
  return f;
}

export function costPerKm(f: Fuel): number {
  switch (f) {
    case "diesel":
      return 12;
    case "petrol":
      return 15;
    case "electric":
      return 5;
  }
}

export function co2PerKm(f: Fuel): number {
  switch (f) {
    case "diesel":
      return 180;
    case "petrol":
      return 210;
    case "electric":
      return 0;
  }
}

export function serviceInterval(): number {
  return 15000;
}

export function toll(km: number): number {
  return Math.floor(km / 100) * 250;
}

export function distance(a: number, b: number): number {
  return Math.abs(a - b);
}

export function etaMinutes(km: number, speed: number): number {
  if (speed <= 0) return 0;
  return Math.floor((km * 60) / speed);
}

export function hhmm(m: number): string {
  return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}`;
}

export function zoneOf(pos: number): string {
  if (pos < 100) return "A";
  if (pos < 500) return "B";
  return "C";
}

export function weightSurcharge(tons: number): number {
  if (tons <= 10) return 0;
  return (tons - 10) * 400;
}

export function nightRate(start: number): number {
  return start < 360 || start >= 1320 ? 150 : 100;
}

export function kmStr(km: number): string {
  return `${km} km`;
}

export function joinLines(xs: readonly string[]): string {
  return xs.join("\\n");
}

export function fuels(): Fuel[] {
  return ["diesel", "petrol", "electric"];
}
'''

TS_COMMON_TEST = '''import { test } from "node:test";
import assert from "node:assert/strict";
import { etaMinutes, hhmm, toll, zoneOf } from "../fleet/common";

test("toll ok", () => {
  assert.ok(toll(300) === 750 && toll(0) === 0);
});

test("eta ok", () => {
  assert.ok(etaMinutes(30, 60) === 30 && etaMinutes(5, 0) === 0);
});

test("hhmm ok", () => {
  assert.ok(hhmm(605) === "10:05" && hhmm(59) === "0:59");
});

test("zone ok", () => {
  assert.ok(zoneOf(99) === "A" && zoneOf(100) === "B" && zoneOf(900) === "C");
});
'''

TS_VEHICLE = '''import { Fuel, co2PerKm, costPerKm, fuelName, joinLines, kmStr, serviceInterval, toll@imports@ } from "./common";

export interface @T@ {
  readonly id: number;
  readonly plate: string;
  readonly fuel: Fuel;
  readonly odometer: number;
  readonly serviced: number;
  readonly capacity: number;
  readonly load: number;
  readonly active: boolean;
}

export function @c@New(id: number, plate: string, fuel: Fuel, odometer: number, capacity: number): @T@ {
  return { id, plate, fuel, odometer, serviced: odometer, capacity, load: 0, active: true };
}

export function @c@Drive(x: @T@, km: number): @T@ {
  return { ...x, odometer: x.odometer + km };
}

export function @c@Service(x: @T@): @T@ {
  return { ...x, serviced: x.odometer };
}

export function @c@SinceService(x: @T@): number {
  return x.odometer - x.serviced;
}

export function @c@DueService(x: @T@): boolean {
  return @c@SinceService(x) >= serviceInterval();
}

export function @c@LoadUp(x: @T@, n: number): @T@ {
  return { ...x, load: Math.min(x.capacity, x.load + n) };
}

export function @c@Unload(x: @T@): @T@ {
  return { ...x, load: 0 };
}

export function @c@Free(x: @T@): number {
  return x.capacity - x.load;
}

export function @c@IsFull(x: @T@): boolean {
  return x.load >= x.capacity;
}

export function @c@TripCost(x: @T@, km: number): number {
  return km * costPerKm(x.fuel) + toll(km);
}

export function @c@TripCo2(x: @T@, km: number): number {
  return km * co2PerKm(x.fuel);
}

export function @c@Label(x: @T@): string {
  return `${x.plate} (${fuelName(x.fuel)})`;
}

export function @c@Line(x: @T@): string {
  return `${@c@Label(x)}: ${kmStr(x.odometer)}, load ${x.load}/${x.capacity}`;
}

export function @c@Report(xs: readonly @T@[]): string {
  return joinLines(xs.map(@c@Line));
}

export function @c@Capacity(xs: readonly @T@[]): number {
  return xs.reduce((s, x) => s + x.capacity, 0);
}

export function @c@ActiveCount(xs: readonly @T@[]): number {
  return xs.filter((x) => x.active).length;
}

export function @c@Find(xs: readonly @T@[], id: number): @T@ | undefined {
  return xs.find((x) => x.id === id);
}

export function @c@Plates(xs: readonly @T@[]): string[] {
  return xs.map((x) => x.plate).sort();
}

export function @c@OfFuel(xs: readonly @T@[], f: Fuel): @T@[] {
  return xs.filter((x) => x.fuel === f);
}

export function @c@DueList(xs: readonly @T@[]): @T@[] {
  return xs.filter(@c@DueService);
}

export function @c@Retire(x: @T@): @T@ {
  return { ...x, active: false };
}

export function @c@Sample(): @T@[] {
  return [@rows@];
}
'''

TS_HAUL = '''
export function @c@HaulCost(x: @T@, km: number, tons: number): number {
  return @c@TripCost(x, km) + weightSurcharge(tons);
}
'''

TS_VEHICLE_TEST = '''import { test } from "node:test";
import assert from "node:assert/strict";
import { @imports@ } from "../fleet/@file@";

test("@c@ drive ok", () => {
  assert.equal(@c@SinceService(@c@Drive(@c@New(1, "P", "diesel", 1000, 10), 250)), 250);
});

test("@c@ cost ok", () => {
  assert.equal(@c@TripCost(@c@New(1, "P", "diesel", 0, 10), 200), 2900);
});

test("@c@ due ok", () => {
  assert.ok(@c@DueService(@c@Drive(@c@New(1, "P", "petrol", 0, 5), 15000)) && !@c@DueService(@c@New(1, "P", "petrol", 0, 5)));
});

test("@c@ plates ok", () => {
  assert.deepEqual(@c@Plates(@c@Sample()), @plates@);
});
'''

TS_HAUL_TEST = '''
test("@c@ haul ok", () => {
  assert.equal(@c@HaulCost(@c@New(1, "P", "diesel", 0, 20), 100, 12), 2250);
});
'''

TS_JOB = '''import { distance, etaMinutes, hhmm, joinLines, kmStr, @imports@zoneOf } from "./common";

export interface @T@ {
  readonly id: number;
  readonly client: string;
  readonly origin: number;
  readonly dest: number;
  readonly start: number;
  readonly speed: number;
  readonly done: boolean;
}

export function @c@New(id: number, client: string, origin: number, dest: number, start: number): @T@ {
  return { id, client, origin, dest, start, speed: @speed@, done: false };
}

export function @c@Km(x: @T@): number {
  return distance(x.origin, x.dest);
}

export function @c@Minutes(x: @T@): number {
  return etaMinutes(@c@Km(x), x.speed);
}

export function @c@Arrival(x: @T@): number {
  return x.start + @c@Minutes(x);
}

export function @c@ArrivalStr(x: @T@): string {
  return hhmm(@c@Arrival(x));
}

export function @c@Late(x: @T@, deadline: number): boolean {
  return @c@Arrival(x) > deadline;
}

export function @c@Price(x: @T@): number {
  return @base@ + @c@Km(x) * @rate@;
}

export function @c@Zone(x: @T@): string {
  return zoneOf(x.dest);
}

export function @c@Complete(x: @T@): @T@ {
  return { ...x, done: true };
}

export function @c@Reschedule(x: @T@, start: number): @T@ {
  return { ...x, start };
}

export function @c@Label(x: @T@): string {
  return `${x.client} #${x.id}`;
}

export function @c@Line(x: @T@): string {
  return `${@c@Label(x)}: ${kmStr(@c@Km(x))} to zone ${@c@Zone(x)}, arrives ${@c@ArrivalStr(x)}`;
}

export function @c@Board(xs: readonly @T@[]): string {
  return joinLines(xs.map(@c@Line));
}

export function @c@Pending(xs: readonly @T@[]): @T@[] {
  return xs.filter((x) => !x.done);
}

export function @c@TotalKm(xs: readonly @T@[]): number {
  return xs.reduce((s, x) => s + @c@Km(x), 0);
}

export function @c@Revenue(xs: readonly @T@[]): number {
  return xs.reduce((s, x) => s + @c@Price(x), 0);
}

export function @c@Find(xs: readonly @T@[], id: number): @T@ | undefined {
  return xs.find((x) => x.id === id);
}

export function @c@Clients(xs: readonly @T@[]): string[] {
  return [...new Set(xs.map((x) => x.client))].sort();
}

export function @c@InZone(xs: readonly @T@[], z: string): @T@[] {
  return xs.filter((x) => @c@Zone(x) === z);
}

export function @c@Longest(xs: readonly @T@[]): @T@ | undefined {
  return xs.reduce<@T@ | undefined>((best, x) => (best === undefined || @c@Km(x) > @c@Km(best) ? x : best), undefined);
}

export function @c@Sample(): @T@[] {
  return [@rows@];
}
'''

TS_NIGHT = '''
export function @c@NightPrice(x: @T@): number {
  return Math.floor((@c@Price(x) * nightRate(x.start)) / 100);
}
'''

TS_JOB_TEST = '''import { test } from "node:test";
import assert from "node:assert/strict";
import { @imports@ } from "../fleet/@file@";

test("@c@ km ok", () => {
  assert.equal(@c@Km(@c@New(1, "c", 300, 120, 0)), 180);
});

test("@c@ arrival ok", () => {
  assert.equal(@c@ArrivalStr(@c@New(1, "c", 0, @speed@, 480)), "9:00");
});

test("@c@ price ok", () => {
  assert.equal(@c@Price(@c@New(1, "c", 0, 10, 0)), @price10@);
});

test("@c@ clients ok", () => {
  assert.deepEqual(@c@Clients(@c@Sample()), @clients@);
});
'''

TS_NIGHT_TEST = '''
test("@c@ night ok", () => {
  assert.ok(@c@NightPrice(@c@New(1, "c", 0, 10, 1400)) === @night10@ && @c@NightPrice(@c@New(1, "c", 0, 10, 600)) === @price10@);
});
'''


def ts(root):
    write(os.path.join(root, "tsconfig.json"), ts_tsconfig("fleet"))
    write(os.path.join(root, "fleet", "common.ts"), TS_COMMON)
    write(os.path.join(root, "tests", "common.test.ts"), TS_COMMON_TEST)
    for i, d in enumerate(VEHICLES):
        rows, plates = vkw(i, d)
        c = camel(d)
        src = fill(TS_VEHICLE, T=cap(d), c=c, rows=vehicle_rows("ts", d, rows), imports=", weightSurcharge" if d in HAULERS else "")
        imports = [c + x for x in ("Drive", "DueService", "New", "Plates", "Sample", "SinceService", "TripCost")]
        test_extra = ""
        if d in HAULERS:
            src += fill(TS_HAUL, T=cap(d), c=c)
            imports.append(c + "HaulCost")
            test_extra = fill(TS_HAUL_TEST, c=c)
        test = fill(TS_VEHICLE_TEST, c=c, file=d, imports=", ".join(sorted(imports)), plates=strs("ts", plates)) + test_extra
        write(os.path.join(root, "fleet", d + ".ts"), src)
        write(os.path.join(root, "tests", f"{d}.test.ts"), test)
    for i, d in enumerate(JOBS):
        kw, rows = jkw(i)
        c = camel(d)
        clients = strs("ts", kw.pop("clients"))
        src = fill(TS_JOB, T=cap(d), c=c, rows=job_rows("ts", d, rows), speed=kw["speed"], base=kw["base"], rate=kw["rate"],
                   imports="nightRate, " if d in NIGHT else "")
        imports = [c + x for x in ("ArrivalStr", "Clients", "Km", "New", "Price", "Sample")]
        test_extra = ""
        if d in NIGHT:
            src += fill(TS_NIGHT, T=cap(d), c=c)
            imports.append(c + "NightPrice")
            test_extra = fill(TS_NIGHT_TEST, c=c, price10=kw["price10"], night10=kw["night10"])
        test = fill(TS_JOB_TEST, c=c, file=d, imports=", ".join(sorted(imports)), speed=kw["speed"], price10=kw["price10"], clients=clients) + test_extra
        write(os.path.join(root, "fleet", d + ".ts"), src)
        write(os.path.join(root, "tests", f"{d}.test.ts"), test)


# ---- Go ---------------------------------------------------------------------------------------

GO_COMMON = '''package fleet

import (
	"fmt"
	"strings"
)

type Fuel int

const (
	Diesel Fuel = iota
	Petrol
	Electric
)

func fuelName(f Fuel) string {
	switch f {
	case Diesel:
		return "diesel"
	case Petrol:
		return "petrol"
	case Electric:
		return "electric"
	}
	panic(fmt.Sprintf("unknown fuel %d", int(f)))
}

func costPerKm(f Fuel) int {
	switch f {
	case Diesel:
		return 12
	case Petrol:
		return 15
	case Electric:
		return 5
	}
	panic(fmt.Sprintf("unknown fuel %d", int(f)))
}

func co2PerKm(f Fuel) int {
	switch f {
	case Diesel:
		return 180
	case Petrol:
		return 210
	case Electric:
		return 0
	}
	panic(fmt.Sprintf("unknown fuel %d", int(f)))
}

func serviceInterval() int {
	return 15000
}

func toll(km int) int {
	return km / 100 * 250
}

func distance(a, b int) int {
	if a > b {
		return a - b
	}
	return b - a
}

func etaMinutes(km, speed int) int {
	if speed <= 0 {
		return 0
	}
	return km * 60 / speed
}

func hhmm(m int) string {
	return fmt.Sprintf("%d:%02d", m/60, m%60)
}

func zoneOf(pos int) string {
	if pos < 100 {
		return "A"
	}
	if pos < 500 {
		return "B"
	}
	return "C"
}

func weightSurcharge(tons int) int {
	if tons <= 10 {
		return 0
	}
	return (tons - 10) * 400
}

func nightRate(start int) int {
	if start < 360 || start >= 1320 {
		return 150
	}
	return 100
}

func kmStr(km int) string {
	return fmt.Sprintf("%d km", km)
}

func joinLines(xs []string) string {
	return strings.Join(xs, "\\n")
}

func fuels() []Fuel {
	return []Fuel{Diesel, Petrol, Electric}
}
'''

GO_COMMON_TEST = '''package fleet

import "testing"

func TestTollOk(t *testing.T) {
	if !(toll(300) == 750 && toll(0) == 0) {
		t.Fatal("toll")
	}
}

func TestEtaOk(t *testing.T) {
	if !(etaMinutes(30, 60) == 30 && etaMinutes(5, 0) == 0) {
		t.Fatal("etaMinutes")
	}
}

func TestHhmmOk(t *testing.T) {
	if !(hhmm(605) == "10:05" && hhmm(59) == "0:59") {
		t.Fatal("hhmm")
	}
}

func TestZoneOk(t *testing.T) {
	if !(zoneOf(99) == "A" && zoneOf(100) == "B" && zoneOf(900) == "C") {
		t.Fatal("zoneOf")
	}
}
'''

GO_VEHICLE = '''package fleet

import (
	"fmt"
	"sort"
)

type @T@ struct {
	id       int
	plate    string
	fuel     Fuel
	odometer int
	serviced int
	capacity int
	load     int
	active   bool
}

func @c@New(id int, plate string, fuel Fuel, odometer int, capacity int) @T@ {
	return @T@{id, plate, fuel, odometer, odometer, capacity, 0, true}
}

func @c@Drive(x @T@, km int) @T@ {
	x.odometer += km
	return x
}

func @c@Service(x @T@) @T@ {
	x.serviced = x.odometer
	return x
}

func @c@SinceService(x @T@) int {
	return x.odometer - x.serviced
}

func @c@DueService(x @T@) bool {
	return @c@SinceService(x) >= serviceInterval()
}

func @c@LoadUp(x @T@, n int) @T@ {
	x.load = min(x.capacity, x.load+n)
	return x
}

func @c@Unload(x @T@) @T@ {
	x.load = 0
	return x
}

func @c@Free(x @T@) int {
	return x.capacity - x.load
}

func @c@IsFull(x @T@) bool {
	return x.load >= x.capacity
}

func @c@TripCost(x @T@, km int) int {
	return km*costPerKm(x.fuel) + toll(km)
}

func @c@TripCo2(x @T@, km int) int {
	return km * co2PerKm(x.fuel)
}

func @c@Label(x @T@) string {
	return fmt.Sprintf("%s (%s)", x.plate, fuelName(x.fuel))
}

func @c@Line(x @T@) string {
	return fmt.Sprintf("%s: %s, load %d/%d", @c@Label(x), kmStr(x.odometer), x.load, x.capacity)
}

func @c@Report(xs []@T@) string {
	lines := []string{}
	for _, x := range xs {
		lines = append(lines, @c@Line(x))
	}
	return joinLines(lines)
}

func @c@Capacity(xs []@T@) int {
	s := 0
	for _, x := range xs {
		s += x.capacity
	}
	return s
}

func @c@ActiveCount(xs []@T@) int {
	n := 0
	for _, x := range xs {
		if x.active {
			n++
		}
	}
	return n
}

func @c@Find(xs []@T@, id int) (@T@, bool) {
	for _, x := range xs {
		if x.id == id {
			return x, true
		}
	}
	return @T@{}, false
}

func @c@Plates(xs []@T@) []string {
	out := []string{}
	for _, x := range xs {
		out = append(out, x.plate)
	}
	sort.Strings(out)
	return out
}

func @c@OfFuel(xs []@T@, f Fuel) []@T@ {
	out := []@T@{}
	for _, x := range xs {
		if x.fuel == f {
			out = append(out, x)
		}
	}
	return out
}

func @c@DueList(xs []@T@) []@T@ {
	out := []@T@{}
	for _, x := range xs {
		if @c@DueService(x) {
			out = append(out, x)
		}
	}
	return out
}

func @c@Retire(x @T@) @T@ {
	x.active = false
	return x
}

func @c@Sample() []@T@ {
	return []@T@{@rows@}
}
'''

GO_HAUL = '''
func @c@HaulCost(x @T@, km, tons int) int {
	return @c@TripCost(x, km) + weightSurcharge(tons)
}
'''

GO_VEHICLE_TEST = '''package fleet

import (
	"reflect"
	"testing"
)

func Test@T@DriveOk(t *testing.T) {
	if got := @c@SinceService(@c@Drive(@c@New(1, "P", Diesel, 1000, 10), 250)); got != 250 {
		t.Fatalf("got %d", got)
	}
}

func Test@T@CostOk(t *testing.T) {
	if got := @c@TripCost(@c@New(1, "P", Diesel, 0, 10), 200); got != 2900 {
		t.Fatalf("got %d", got)
	}
}

func Test@T@DueOk(t *testing.T) {
	if !(@c@DueService(@c@Drive(@c@New(1, "P", Petrol, 0, 5), 15000)) && !@c@DueService(@c@New(1, "P", Petrol, 0, 5))) {
		t.Fatal("due service")
	}
}

func Test@T@PlatesOk(t *testing.T) {
	if got := @c@Plates(@c@Sample()); !reflect.DeepEqual(got, @plates@) {
		t.Fatalf("got %v", got)
	}
}
'''

GO_HAUL_TEST = '''
func Test@T@HaulOk(t *testing.T) {
	if got := @c@HaulCost(@c@New(1, "P", Diesel, 0, 20), 100, 12); got != 2250 {
		t.Fatalf("got %d", got)
	}
}
'''

GO_JOB = '''package fleet

import (
	"fmt"
	"sort"
)

type @T@ struct {
	id     int
	client string
	origin int
	dest   int
	start  int
	speed  int
	done   bool
}

func @c@New(id int, client string, origin int, dest int, start int) @T@ {
	return @T@{id, client, origin, dest, start, @speed@, false}
}

func @c@Km(x @T@) int {
	return distance(x.origin, x.dest)
}

func @c@Minutes(x @T@) int {
	return etaMinutes(@c@Km(x), x.speed)
}

func @c@Arrival(x @T@) int {
	return x.start + @c@Minutes(x)
}

func @c@ArrivalStr(x @T@) string {
	return hhmm(@c@Arrival(x))
}

func @c@Late(x @T@, deadline int) bool {
	return @c@Arrival(x) > deadline
}

func @c@Price(x @T@) int {
	return @base@ + @c@Km(x)*@rate@
}

func @c@Zone(x @T@) string {
	return zoneOf(x.dest)
}

func @c@Complete(x @T@) @T@ {
	x.done = true
	return x
}

func @c@Reschedule(x @T@, start int) @T@ {
	x.start = start
	return x
}

func @c@Label(x @T@) string {
	return fmt.Sprintf("%s #%d", x.client, x.id)
}

func @c@Line(x @T@) string {
	return fmt.Sprintf("%s: %s to zone %s, arrives %s", @c@Label(x), kmStr(@c@Km(x)), @c@Zone(x), @c@ArrivalStr(x))
}

func @c@Board(xs []@T@) string {
	lines := []string{}
	for _, x := range xs {
		lines = append(lines, @c@Line(x))
	}
	return joinLines(lines)
}

func @c@Pending(xs []@T@) []@T@ {
	out := []@T@{}
	for _, x := range xs {
		if !x.done {
			out = append(out, x)
		}
	}
	return out
}

func @c@TotalKm(xs []@T@) int {
	s := 0
	for _, x := range xs {
		s += @c@Km(x)
	}
	return s
}

func @c@Revenue(xs []@T@) int {
	s := 0
	for _, x := range xs {
		s += @c@Price(x)
	}
	return s
}

func @c@Find(xs []@T@, id int) (@T@, bool) {
	for _, x := range xs {
		if x.id == id {
			return x, true
		}
	}
	return @T@{}, false
}

func @c@Clients(xs []@T@) []string {
	seen := map[string]bool{}
	out := []string{}
	for _, x := range xs {
		if !seen[x.client] {
			seen[x.client] = true
			out = append(out, x.client)
		}
	}
	sort.Strings(out)
	return out
}

func @c@InZone(xs []@T@, z string) []@T@ {
	out := []@T@{}
	for _, x := range xs {
		if @c@Zone(x) == z {
			out = append(out, x)
		}
	}
	return out
}

func @c@Longest(xs []@T@) (@T@, bool) {
	if len(xs) == 0 {
		return @T@{}, false
	}
	best := xs[0]
	for _, x := range xs[1:] {
		if @c@Km(x) > @c@Km(best) {
			best = x
		}
	}
	return best, true
}

func @c@Sample() []@T@ {
	return []@T@{@rows@}
}
'''

GO_NIGHT = '''
func @c@NightPrice(x @T@) int {
	return @c@Price(x) * nightRate(x.start) / 100
}
'''

GO_JOB_TEST = '''package fleet

import (
	"reflect"
	"testing"
)

func Test@T@KmOk(t *testing.T) {
	if got := @c@Km(@c@New(1, "c", 300, 120, 0)); got != 180 {
		t.Fatalf("got %d", got)
	}
}

func Test@T@ArrivalOk(t *testing.T) {
	if got := @c@ArrivalStr(@c@New(1, "c", 0, @speed@, 480)); got != "9:00" {
		t.Fatalf("got %q", got)
	}
}

func Test@T@PriceOk(t *testing.T) {
	if got := @c@Price(@c@New(1, "c", 0, 10, 0)); got != @price10@ {
		t.Fatalf("got %d", got)
	}
}

func Test@T@ClientsOk(t *testing.T) {
	if got := @c@Clients(@c@Sample()); !reflect.DeepEqual(got, @clients@) {
		t.Fatalf("got %v", got)
	}
}
'''

GO_NIGHT_TEST = '''
func Test@T@NightOk(t *testing.T) {
	if !(@c@NightPrice(@c@New(1, "c", 0, 10, 1400)) == @night10@ && @c@NightPrice(@c@New(1, "c", 0, 10, 600)) == @price10@) {
		t.Fatal("night price")
	}
}
'''


def go(root):
    write(os.path.join(root, "go.mod"), GOMOD)
    write(os.path.join(root, "fleet", "common.go"), GO_COMMON)
    write(os.path.join(root, "fleet", "common_test.go"), GO_COMMON_TEST)
    for i, d in enumerate(VEHICLES):
        rows, plates = vkw(i, d)
        c, T = camel(d), cap(d)
        src = fill(GO_VEHICLE, T=T, c=c, rows=vehicle_rows("go", d, rows))
        test = fill(GO_VEHICLE_TEST, T=T, c=c, plates=strs("go", plates))
        if d in HAULERS:
            src += fill(GO_HAUL, T=T, c=c)
            test += fill(GO_HAUL_TEST, T=T, c=c)
        write(os.path.join(root, "fleet", d + ".go"), src)
        write(os.path.join(root, "fleet", f"{d}_test.go"), test)
    for i, d in enumerate(JOBS):
        kw, rows = jkw(i)
        c, T = camel(d), cap(d)
        clients = strs("go", kw.pop("clients"))
        src = fill(GO_JOB, T=T, c=c, rows=job_rows("go", d, rows), speed=kw["speed"], base=kw["base"], rate=kw["rate"])
        test = fill(GO_JOB_TEST, T=T, c=c, speed=kw["speed"], price10=kw["price10"], clients=clients)
        if d in NIGHT:
            src += fill(GO_NIGHT, T=T, c=c)
            test += fill(GO_NIGHT_TEST, T=T, c=c, price10=kw["price10"], night10=kw["night10"])
        write(os.path.join(root, "fleet", d + ".go"), src)
        write(os.path.join(root, "fleet", f"{d}_test.go"), test)
    gofmt(os.path.join(root, "fleet"))


def generate(out):
    os.makedirs(out, exist_ok=True)
    write(os.path.join(out, "start.ssp"), ssp())
    py(os.path.join(out, "py"))
    ts(os.path.join(out, "ts"))
    go(os.path.join(out, "go"))


if __name__ == "__main__":
    generate(sys.argv[1])
