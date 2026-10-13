"""s6_fleet: four targeted changes on the fleet codebase (a different change set from s5)."""
import os, re, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "large"))
import gen_fleet as g  # noqa: E402
from lib import C, Eq, F, L, P, V  # noqa: E402

CODEBASE = "fleet"
TITLE = "Fleet, second task: four targeted changes in a large codebase"
EXAMPLE = "service_interval is serviceInterval"
STEPS = [
    "Add the fuel Hydrogen: 9 cents per km, no CO2, the name \"hydrogen\", and listed last by fuels().",
    "Electric vehicles are serviced every 30,000 km; every other fuel stays at 15,000. service_interval gets a parameter fuel (a Fuel), and every caller passes its vehicle's fuel.",
    "Urgent medical jobs: medical_new gets a new last parameter urgent (a Bool), stored in a new field urgent of Medical. Every existing caller passes false. An urgent job's medical_price is 50% higher (rounded down), and its medical_line starts with \"URGENT \".",
    "Add courier_quote(j, v): the courier_price of job j plus the van_trip_cost of van v over that job's courier_km.",
    "Add a test for each change. All existing tests must still pass.",
]
IFACE = {
    "sspur": "fn service_interval(fuel: Fuel) -> Int; Fuel gets the variant Hydrogen; fn medical_new(id: Int, client: Str, origin: Int, dest: Int, start: Int, urgent: Bool) -> Medical (Medical gets a field urgent: Bool); fn courier_quote(j: Courier, v: Van) -> Int; fuels and the other existing functions keep their signatures",
    "python": "fleet.common.service_interval(fuel) -> int; Fuel.HYDROGEN; fleet.medical.medical_new(id, client, origin, dest, start, urgent) -> Medical (a new field urgent); fleet.courier.courier_quote(j, v) -> int; fuels and the other existing functions keep their modules and signatures",
    "ts": "fleet/common.ts: export function serviceInterval(fuel: Fuel): number; Fuel gets \"hydrogen\"; fleet/medical.ts: export function medicalNew(id: number, client: string, origin: number, dest: number, start: number, urgent: boolean): Medical (Medical gets readonly urgent: boolean); fleet/courier.ts: export function courierQuote(j: Courier, v: Van): number; fuels and the other existing functions keep their modules and signatures",
    "go": "func serviceInterval(fuel Fuel) int; a Fuel constant Hydrogen after Electric; func medicalNew(id int, client string, origin int, dest int, start int, urgent bool) Medical (Medical gets a field urgent bool); func courierQuote(j Courier, v Van) int, all in package fleet; fuels and the other existing functions keep their signatures",
}

DIESEL, PETROL, ELECTRIC, HYDROGEN = (V("Fuel", n) for n in ("Diesel", "Petrol", "Electric", "Hydrogen"))


def v(d, *a):
    return C(d + "_new", *a)


def driven(d, fuel, km, i=1):
    return C(d + "_drive", v(d, i, "P", fuel, 0, 5), km)


def med(*a):
    return C("medical_new", *a)


HIDDEN = [
    ("hidden_hydrogen", [Eq(C("cost_per_km", HYDROGEN), 9), Eq(C("co2_per_km", HYDROGEN), 0), Eq(C("fuel_name", HYDROGEN), "hydrogen")]),
    ("hidden_fuels", [Eq(C("fuels"), L("Fuel", [DIESEL, PETROL, ELECTRIC, HYDROGEN]))]),
    ("hidden_hydrogen_used", [Eq(C("bus_trip_cost", v("bus", 1, "H", HYDROGEN, 0, 40), 100), 1150),
                              Eq(C("bus_label", v("bus", 1, "H-1", HYDROGEN, 0, 40)), "H-1 (hydrogen)"),
                              Eq(C("taxi_trip_co2", v("taxi", 1, "H", HYDROGEN, 0, 4), 50), 0)]),
    ("hidden_interval", [Eq(C("service_interval", ELECTRIC), 30000), Eq(C("service_interval", DIESEL), 15000),
                         Eq(C("service_interval", PETROL), 15000), Eq(C("service_interval", HYDROGEN), 15000)]),
    ("hidden_due_callers", [Eq(C("truck_due_service", driven("truck", ELECTRIC, 20000)), False),
                            Eq(C("truck_due_service", driven("truck", ELECTRIC, 30000)), True),
                            Eq(C("van_due_service", driven("van", DIESEL, 15000)), True),
                            Eq(P(C("sweeper_due_list", L("Sweeper", [driven("sweeper", ELECTRIC, 20000, 1), driven("sweeper", PETROL, 20000, 2)])), "Sweeper", "id", "Int"), [2])]),
    ("hidden_due_rest", [Eq(C("cargo_bike_due_service", driven("cargo_bike", ELECTRIC, 16000)), False),
                         Eq(C("ambulance_due_service", driven("ambulance", PETROL, 15000)), True),
                         Eq(C("scooter_due_service", driven("scooter", HYDROGEN, 15000)), True)]),
    ("hidden_urgent", [Eq(F(med(1, "c", 0, 10, 600, True), "urgent"), True),
                       Eq(C("medical_price", med(1, "c", 0, 10, 600, True)), 1125),
                       Eq(C("medical_price", med(1, "c", 0, 10, 600, False)), 750)]),
    ("hidden_urgent_line", [Eq(C("medical_line", med(3, "Iris", 0, 120, 480, True)), "URGENT Iris #3: 120 km to zone B, arrives 9:30"),
                            Eq(C("medical_line", med(3, "Iris", 0, 120, 480, False)), "Iris #3: 120 km to zone B, arrives 9:30")]),
    ("hidden_urgent_rest", [Eq(P(C("medical_sample"), "Medical", "urgent", "Bool"), [False, False, False]),
                            Eq(C("medical_night_price", med(1, "c", 0, 10, 1400, True)), 1687),
                            Eq(C("medical_night_price", med(1, "c", 0, 10, 1400, False)), 1125)]),
    ("hidden_quote", [Eq(C("courier_quote", C("courier_new", 1, "c", 0, 100, 480), v("van", 1, "V", DIESEL, 0, 5)), 3950),
                      Eq(C("courier_quote", C("courier_new", 2, "c", 300, 150, 0), v("van", 2, "W", ELECTRIC, 0, 5)), 4500)]),
    ("hidden_unchanged", [Eq(C("van_plates", C("van_sample")), sorted(r[1] for r in g.vehicle_params(1, "van"))),
                          Eq(C("toll", 300), 750), Eq(C("courier_price", C("courier_new", 1, "c", 0, 10, 0)), 700)]),
]

MED_NEW = re.compile(r'(medical_new|medicalNew)\((\d+, "[^"]*", \d+, \d+, \d+)\)')


def patches():
    sp = [
        ("start.ssp", "type Fuel = Diesel | Petrol | Electric", "type Fuel = Diesel | Petrol | Electric | Hydrogen"),
        ("start.ssp", '  | Electric => "electric"\n', '  | Electric => "electric"\n  | Hydrogen => "hydrogen"\n'),
        ("start.ssp", "  | Electric => 5\n", "  | Electric => 5\n  | Hydrogen => 9\n"),
        ("start.ssp", "  | Electric => 0\n", "  | Electric => 0\n  | Hydrogen => 0\n"),
        ("start.ssp", "= [Diesel, Petrol, Electric]", "= [Diesel, Petrol, Electric, Hydrogen]"),
        ("start.ssp", "fn service_interval() -> Int\n= 15000", "fn service_interval(fuel: Fuel) -> Int\n= if fuel == Electric then 30000 else 15000"),
        ("start.ssp", ">= service_interval()", ">= service_interval(x.fuel)", 20),
        ("start.ssp", "type Medical = {id: Int, client: Str, origin: Int, dest: Int, start: Int, speed: Int, done: Bool}",
         "type Medical = {id: Int, client: Str, origin: Int, dest: Int, start: Int, speed: Int, done: Bool, urgent: Bool}"),
        ("start.ssp", "fn medical_new(id: Int, client: Str, origin: Int, dest: Int, start: Int) -> Medical\n= Medical{id, client, origin, dest, start, speed: 80, done: false}",
         "fn medical_new(id: Int, client: Str, origin: Int, dest: Int, start: Int, urgent: Bool) -> Medical\n= Medical{id, client, origin, dest, start, speed: 80, done: false, urgent}"),
        ("start.ssp", MED_NEW, r"\1(\2, false)", 8),
        ("start.ssp", "fn medical_price(x: Medical) -> Int\n= 500 + medical_km(x) * 25",
         "fn medical_price(x: Medical) -> Int\n= do\n  p = 500 + medical_km(x) * 25\n  if x.urgent then p * 3 / 2 else p"),
        ("start.ssp", re.compile(r'(fn medical_line\(x: Medical\) -> Str\n= )(".*")'), r'\1(if x.urgent then "URGENT " else "") + \2'),
        ("start.ssp", None, "fn courier_quote(j: Courier, v: Van) -> Int\n= courier_price(j) + van_trip_cost(v, courier_km(j))\n\n"
         "test s6_hydrogen = cost_per_km(Hydrogen) == 9 and fuel_name(Hydrogen) == \"hydrogen\"\n\n"
         "test s6_interval = not truck_due_service(truck_drive(truck_new(1, \"T\", Electric, 0, 10), 20000))\n\n"
         "test s6_urgent = medical_price(medical_new(1, \"c\", 0, 10, 600, true)) == 1125\n\n"
         "test s6_quote = courier_quote(courier_new(1, \"c\", 0, 100, 480), van_new(1, \"V\", Diesel, 0, 5)) == 3950"),
    ]
    py = [
        ("fleet/common.py", '    ELECTRIC = "electric"\n', '    ELECTRIC = "electric"\n    HYDROGEN = "hydrogen"\n'),
        ("fleet/common.py", "Fuel.ELECTRIC: 5}", "Fuel.ELECTRIC: 5, Fuel.HYDROGEN: 9}"),
        ("fleet/common.py", "Fuel.ELECTRIC: 0}", "Fuel.ELECTRIC: 0, Fuel.HYDROGEN: 0}"),
        ("fleet/common.py", "def service_interval() -> int:\n    return 15000",
         "def service_interval(fuel: Fuel) -> int:\n    return 30000 if fuel == Fuel.ELECTRIC else 15000"),
        ("fleet/medical.py", "    done: bool\n", "    done: bool\n    urgent: bool\n"),
        ("fleet/medical.py", "def medical_new(id: int, client: str, origin: int, dest: int, start: int) -> Medical:\n    return Medical(id, client, origin, dest, start, 80, False)",
         "def medical_new(id: int, client: str, origin: int, dest: int, start: int, urgent: bool) -> Medical:\n    return Medical(id, client, origin, dest, start, 80, False, urgent)"),
        ("fleet/medical.py", MED_NEW, r"\1(\2, False)", 3),
        ("tests/test_medical.py", MED_NEW, r"\1(\2, False)", 5),
        ("fleet/medical.py", "def medical_price(x: Medical) -> int:\n    return 500 + medical_km(x) * 25",
         "def medical_price(x: Medical) -> int:\n    p = 500 + medical_km(x) * 25\n    return p * 3 // 2 if x.urgent else p"),
        ("fleet/medical.py", '    return f"{medical_label(x)}: ', '    return ("URGENT " if x.urgent else "") + f"{medical_label(x)}: '),
        ("fleet/courier.py", "from fleet.common import distance, eta_minutes, hhmm, join_lines, km_str, zone_of\n",
         "from fleet.common import distance, eta_minutes, hhmm, join_lines, km_str, zone_of\nfrom fleet.van import Van, van_trip_cost\n"),
        ("fleet/courier.py", None, "\ndef courier_quote(j: Courier, v: Van) -> int:\n    return courier_price(j) + van_trip_cost(v, courier_km(j))"),
    ]
    ts = [
        ("fleet/common.ts", 'export type Fuel = "diesel" | "petrol" | "electric";', 'export type Fuel = "diesel" | "petrol" | "electric" | "hydrogen";'),
        ("fleet/common.ts", '    case "electric":\n      return 5;\n', '    case "electric":\n      return 5;\n    case "hydrogen":\n      return 9;\n'),
        ("fleet/common.ts", '    case "electric":\n      return 0;\n', '    case "electric":\n      return 0;\n    case "hydrogen":\n      return 0;\n'),
        ("fleet/common.ts", 'return ["diesel", "petrol", "electric"];', 'return ["diesel", "petrol", "electric", "hydrogen"];'),
        ("fleet/common.ts", "export function serviceInterval(): number {\n  return 15000;",
         'export function serviceInterval(fuel: Fuel): number {\n  return fuel === "electric" ? 30000 : 15000;'),
        ("fleet/medical.ts", "  readonly done: boolean;\n", "  readonly done: boolean;\n  readonly urgent: boolean;\n"),
        ("fleet/medical.ts", "export function medicalNew(id: number, client: string, origin: number, dest: number, start: number): Medical {\n  return { id, client, origin, dest, start, speed: 80, done: false };",
         "export function medicalNew(id: number, client: string, origin: number, dest: number, start: number, urgent: boolean): Medical {\n  return { id, client, origin, dest, start, speed: 80, done: false, urgent };"),
        ("fleet/medical.ts", MED_NEW, r"\1(\2, false)", 3),
        ("tests/medical.test.ts", MED_NEW, r"\1(\2, false)", 5),
        ("fleet/medical.ts", "  return 500 + medicalKm(x) * 25;", "  const p = 500 + medicalKm(x) * 25;\n  return x.urgent ? Math.floor((p * 3) / 2) : p;"),
        ("fleet/medical.ts", "  return `${medicalLabel(x)}: ", '  return (x.urgent ? "URGENT " : "") + `${medicalLabel(x)}: '),
        ("fleet/courier.ts", ' } from "./common";\n', ' } from "./common";\nimport { Van, vanTripCost } from "./van";\n'),
        ("fleet/courier.ts", None, "export function courierQuote(j: Courier, v: Van): number {\n  return courierPrice(j) + vanTripCost(v, courierKm(j));\n}"),
    ]
    go = [
        ("fleet/common.go", "\tElectric\n)", "\tElectric\n\tHydrogen\n)"),
        ("fleet/common.go", '\tcase Electric:\n\t\treturn "electric"\n', '\tcase Electric:\n\t\treturn "electric"\n\tcase Hydrogen:\n\t\treturn "hydrogen"\n'),
        ("fleet/common.go", "\tcase Electric:\n\t\treturn 5\n", "\tcase Electric:\n\t\treturn 5\n\tcase Hydrogen:\n\t\treturn 9\n"),
        ("fleet/common.go", "\tcase Electric:\n\t\treturn 0\n", "\tcase Electric:\n\t\treturn 0\n\tcase Hydrogen:\n\t\treturn 0\n"),
        ("fleet/common.go", "[]Fuel{Diesel, Petrol, Electric}", "[]Fuel{Diesel, Petrol, Electric, Hydrogen}"),
        ("fleet/common.go", "func serviceInterval() int {\n\treturn 15000",
         "func serviceInterval(fuel Fuel) int {\n\tif fuel == Electric {\n\t\treturn 30000\n\t}\n\treturn 15000"),
        ("fleet/medical.go", "\tdone   bool\n}", "\tdone   bool\n\turgent bool\n}"),
        ("fleet/medical.go", "func medicalNew(id int, client string, origin int, dest int, start int) Medical {\n\treturn Medical{id, client, origin, dest, start, 80, false}",
         "func medicalNew(id int, client string, origin int, dest int, start int, urgent bool) Medical {\n\treturn Medical{id, client, origin, dest, start, 80, false, urgent}"),
        ("fleet/medical.go", MED_NEW, r"\1(\2, false)", 3),
        ("fleet/medical_test.go", MED_NEW, r"\1(\2, false)", 5),
        ("fleet/medical.go", "\treturn 500 + medicalKm(x)*25", "\tp := 500 + medicalKm(x)*25\n\tif x.urgent {\n\t\treturn p * 3 / 2\n\t}\n\treturn p"),
        ("fleet/medical.go", re.compile(r'\treturn (fmt\.Sprintf\("%s: %s to zone %s, arrives %s", medicalLabel.*\))\n'),
         r'\ts := \1\n\tif x.urgent {\n\t\ts = "URGENT " + s\n\t}\n\treturn s\n'),
        ("fleet/courier.go", None, "func courierQuote(j Courier, v Van) int {\n\treturn courierPrice(j) + vanTripCost(v, courierKm(j))\n}"),
    ]
    for d in g.VEHICLES:
        py.append((f"fleet/{d}.py", ">= service_interval()", ">= service_interval(x.fuel)"))
        ts.append((f"fleet/{d}.ts", ">= serviceInterval();", ">= serviceInterval(x.fuel);"))
        go.append((f"fleet/{d}.go", ">= serviceInterval()", ">= serviceInterval(x.fuel)"))
    return {"sspur": sp, "python": py, "ts": ts, "go": go}
