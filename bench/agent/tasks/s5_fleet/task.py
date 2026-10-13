"""s5_fleet: four targeted changes on the fleet codebase."""
import os, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "large"))
import gen_fleet as g  # noqa: E402
from lib import C, Eq, L, Nothing, Some, V  # noqa: E402

CODEBASE = "fleet"
TITLE = "Fleet: four targeted changes in a large codebase"
EXAMPLE = "eta_minutes is etaMinutes"
STEPS = [
    "Bug: eta_minutes rounds down, so 11 km at 40 km/h shows 16 minutes. It must round up to whole minutes (17); exact results stay as they are.",
    "Tolls are charged per started 100 km, not per full 100 km: 1 to 100 km cost 250, 101 to 200 km cost 500, and 0 km is free.",
    "Electric haulers pay half the weight surcharge: weight_surcharge gets a new last parameter fuel (a Fuel), and for Electric the surcharge is halved (rounded down). Every existing caller passes its vehicle's fuel.",
    "Add van_best_for(xs, km): the active van with the lowest van_trip_cost for km, the first one on a tie, or none when no van is active.",
    "Add a test for each change. All existing tests must still pass.",
]
IFACE = {
    "sspur": "fn weight_surcharge(tons: Int, fuel: Fuel) -> Int; fn van_best_for(xs: List[Van], km: Int) -> Opt[Van]; eta_minutes, toll and the other existing functions keep their signatures",
    "python": "fleet.common.weight_surcharge(tons, fuel) -> int; fleet.van.van_best_for(xs, km) -> Optional[Van] (None when no van is active); eta_minutes, toll and the other existing functions keep their modules and signatures",
    "ts": "fleet/common.ts: export function weightSurcharge(tons: number, fuel: Fuel): number; fleet/van.ts: export function vanBestFor(xs: readonly Van[], km: number): Van | undefined (undefined when no van is active); etaMinutes, toll and the other existing functions keep their modules and signatures",
    "go": "func weightSurcharge(tons int, fuel Fuel) int; func vanBestFor(xs []Van, km int) (Van, bool) (false when no van is active), all in package fleet; etaMinutes, toll and the other existing functions keep their signatures",
}

DIESEL, PETROL, ELECTRIC = (V("Fuel", n) for n in ("Diesel", "Petrol", "Electric"))


def v(d, *a):
    return C(d + "_new", *a)


VANS = L("Van", [C("van_retire", v("van", 9, "Z", ELECTRIC, 0, 5)), v("van", 1, "A", PETROL, 0, 5), v("van", 2, "B", DIESEL, 0, 5), v("van", 3, "C", DIESEL, 0, 9)])

HIDDEN = [
    ("hidden_eta_up", [Eq(C("eta_minutes", 11, 40), 17), Eq(C("eta_minutes", 30, 60), 30), Eq(C("eta_minutes", 1, 70), 1),
                       Eq(C("eta_minutes", 0, 50), 0), Eq(C("eta_minutes", 5, 0), 0)]),
    ("hidden_eta_used", [Eq(C("courier_minutes", C("courier_new", 1, "c", 0, 11, 0)), 17),
                         Eq(C("courier_arrival_str", C("courier_new", 1, "c", 0, 11, 480)), "8:17"),
                         Eq(C("freight_minutes", C("freight_new", 1, "c", 0, 7, 0)), 9)]),
    ("hidden_toll", [Eq(C("toll", 1), 250), Eq(C("toll", 100), 250), Eq(C("toll", 101), 500), Eq(C("toll", 250), 750), Eq(C("toll", 0), 0)]),
    ("hidden_toll_used", [Eq(C("van_trip_cost", v("van", 1, "V", DIESEL, 0, 5), 150), 2300),
                          Eq(C("bus_trip_cost", v("bus", 1, "B", PETROL, 0, 40), 50), 1000)]),
    ("hidden_surcharge", [Eq(C("weight_surcharge", 12, DIESEL), 800), Eq(C("weight_surcharge", 12, ELECTRIC), 400),
                          Eq(C("weight_surcharge", 13, ELECTRIC), 600), Eq(C("weight_surcharge", 9, ELECTRIC), 0),
                          Eq(C("weight_surcharge", 15, PETROL), 2000)]),
    ("hidden_haul_callers", [Eq(C("truck_haul_cost", v("truck", 1, "T", ELECTRIC, 0, 20), 100, 12), 1150),
                             Eq(C("tanker_haul_cost", v("tanker", 1, "T", DIESEL, 0, 20), 100, 12), 2250)]),
    ("hidden_haul_rest", [Eq(C("flatbed_haul_cost", v("flatbed", 1, "F", ELECTRIC, 0, 20), 200, 15), 2500),
                          Eq(C("reefer_haul_cost", v("reefer", 1, "R", PETROL, 0, 20), 100, 11), 2150),
                          Eq(C("tipper_haul_cost", v("tipper", 1, "T", ELECTRIC, 0, 20), 0, 20), 2000)]),
    ("hidden_best_van", [Some(C("van_best_for", VANS, 100), "id", 2),
                         Some(C("van_best_for", L("Van", [v("van", 4, "E", ELECTRIC, 0, 5), v("van", 5, "F", ELECTRIC, 0, 5)]), 100), "id", 4),
                         Some(C("van_best_for", L("Van", [C("van_retire", v("van", 6, "G", DIESEL, 0, 5)), v("van", 7, "H", PETROL, 0, 5), v("van", 8, "I", ELECTRIC, 0, 5)]), 0), "id", 7)]),
    ("hidden_best_van_none", [Nothing(C("van_best_for", L("Van", []), 10)),
                              Nothing(C("van_best_for", L("Van", [C("van_retire", v("van", 1, "A", DIESEL, 0, 5))]), 10))]),
    ("hidden_unchanged", [Eq(C("van_plates", C("van_sample")), sorted(r[1] for r in g.vehicle_params(1, "van"))),
                          Eq(C("hhmm", 605), "10:05"), Eq(C("courier_price", C("courier_new", 1, "c", 0, 10, 0)), 700)]),
]


def patches():
    sp = [
        ("start.ssp", "= if speed <= 0 then 0 else km * 60 / speed", "= if speed <= 0 then 0 else (km * 60 + speed - 1) / speed"),
        ("start.ssp", "= km / 100 * 250", "= (km + 99) / 100 * 250"),
        ("start.ssp", "fn weight_surcharge(tons: Int) -> Int\n= if tons <= 10 then 0 else (tons - 10) * 400",
         "fn weight_surcharge(tons: Int, fuel: Fuel) -> Int\n= do\n  s = if tons <= 10 then 0 else (tons - 10) * 400\n  if fuel == Electric then s / 2 else s"),
        ("start.ssp", "+ weight_surcharge(tons)", "+ weight_surcharge(tons, x.fuel)", 5),
        ("start.ssp", None, "fn van_best_for(xs: List[Van], km: Int) -> Opt[Van]\n= xs.filter(_.active).sort_by(x => van_trip_cost(x, km)).first\n\n"
         "test s5_eta = eta_minutes(11, 40) == 17\n\n"
         "test s5_toll = toll(101) == 500 and toll(1) == 250\n\n"
         "test s5_surcharge = truck_haul_cost(truck_new(1, \"T\", Electric, 0, 20), 100, 12) == 1150\n\n"
         "test s5_best_van = van_best_for([van_new(1, \"A\", Petrol, 0, 5), van_new(2, \"B\", Electric, 0, 5)], 10).map(_.id) == some(2)"),
    ]
    py = [
        ("fleet/common.py", "    return km * 60 // speed", "    return (km * 60 + speed - 1) // speed"),
        ("fleet/common.py", "    return km // 100 * 250", "    return (km + 99) // 100 * 250"),
        ("fleet/common.py", "def weight_surcharge(tons: int) -> int:\n    if tons <= 10:\n        return 0\n    return (tons - 10) * 400",
         "def weight_surcharge(tons: int, fuel: Fuel) -> int:\n    s = 0 if tons <= 10 else (tons - 10) * 400\n    return s // 2 if fuel == Fuel.ELECTRIC else s"),
        ("fleet/van.py", None, "\ndef van_best_for(xs: List[Van], km: int) -> Optional[Van]:\n"
         "    return min((x for x in xs if x.active), key=lambda x: van_trip_cost(x, km), default=None)"),
    ]
    ts = [
        ("fleet/common.ts", "return Math.floor((km * 60) / speed);", "return Math.ceil((km * 60) / speed);"),
        ("fleet/common.ts", "return Math.floor(km / 100) * 250;", "return Math.ceil(km / 100) * 250;"),
        ("fleet/common.ts", "export function weightSurcharge(tons: number): number {\n  if (tons <= 10) return 0;\n  return (tons - 10) * 400;",
         'export function weightSurcharge(tons: number, fuel: Fuel): number {\n  const s = tons <= 10 ? 0 : (tons - 10) * 400;\n  return fuel === "electric" ? Math.floor(s / 2) : s;'),
        ("fleet/van.ts", None, "export function vanBestFor(xs: readonly Van[], km: number): Van | undefined {\n"
         "  return xs\n    .filter((x) => x.active)\n    .reduce<Van | undefined>((best, x) => (best === undefined || vanTripCost(x, km) < vanTripCost(best, km) ? x : best), undefined);\n}"),
    ]
    go = [
        ("fleet/common.go", "\treturn km * 60 / speed", "\treturn (km*60 + speed - 1) / speed"),
        ("fleet/common.go", "\treturn km / 100 * 250", "\treturn (km + 99) / 100 * 250"),
        ("fleet/common.go", "func weightSurcharge(tons int) int {\n\tif tons <= 10 {\n\t\treturn 0\n\t}\n\treturn (tons - 10) * 400",
         "func weightSurcharge(tons int, fuel Fuel) int {\n\ts := 0\n\tif tons > 10 {\n\t\ts = (tons - 10) * 400\n\t}\n\tif fuel == Electric {\n\t\treturn s / 2\n\t}\n\treturn s"),
        ("fleet/van.go", None, "func vanBestFor(xs []Van, km int) (Van, bool) {\n\tbest, found := Van{}, false\n\tfor _, x := range xs {\n"
         "\t\tif x.active && (!found || vanTripCost(x, km) < vanTripCost(best, km)) {\n\t\t\tbest, found = x, true\n\t\t}\n\t}\n\treturn best, found\n}"),
    ]
    for d in g.HAULERS:
        py.append((f"fleet/{d}.py", "+ weight_surcharge(tons)", "+ weight_surcharge(tons, x.fuel)"))
        ts.append((f"fleet/{d}.ts", "+ weightSurcharge(tons);", "+ weightSurcharge(tons, x.fuel);"))
        go.append((f"fleet/{d}.go", "+ weightSurcharge(tons)", "+ weightSurcharge(tons, x.fuel)"))
    return {"sspur": sp, "python": py, "ts": ts, "go": go}
