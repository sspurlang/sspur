"""Generate the library codebase (start state) in SSPUR, Python, TypeScript and Go.

  python3 gen_library.py OUT   -> OUT/start.ssp, OUT/py/{library,tests}, OUT/ts/{library,tests,tsconfig.json}, OUT/go/{go.mod,library}

A lending library: a common module (formats as a sum type, loan days, fines, money and shelf
formatting, member limits), 22 catalog item modules (book, journal, dvd, ...: a record type,
21 functions and 4 tests each) and 16 member modules (student, staff, ...: a record type,
20 functions and 4 tests each). Five item kinds can be renewed and four member kinds get
fine reminders, so a few common helpers have a handful of callers instead of dozens. The
same definitions, sample data and test values in every language.
"""
import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import GOMOD, camel, cap, fill, gofmt, q, ts_tsconfig, write  # noqa: E402

ITEMS = ["book", "journal", "magazine", "dvd", "cd", "vinyl", "map_sheet", "atlas", "score", "thesis", "manuscript",
         "comic", "newspaper", "poster", "photo", "game", "kit", "pamphlet", "report", "globe", "zine", "letter"]
MEMBERS = ["student", "staff", "faculty", "alumni", "guest", "child", "senior", "researcher", "visitor", "partner",
           "volunteer", "donor", "trustee", "intern", "teacher", "family"]
MODULES = ITEMS + MEMBERS
RENEWABLE = ["book", "journal", "score", "comic", "atlas"]
REMINDED = ["student", "staff", "faculty", "alumni"]
FORMATS = ["Print", "Ebook", "Audio"]

ADJ = ["Silent", "Golden", "Hidden", "Broken", "Northern", "Quiet", "Bright", "Final", "Lost", "Second", "Little", "Open", "Winter"]
NOUN = ["River", "Garden", "Archive", "Voyage", "Harbor", "Letters", "Atlas", "Engine", "Forest", "Island", "Tower", "Lantern"]
FIRST = ["Ada", "Ben", "Cleo", "Dev", "Eva", "Finn", "Gia", "Hugo", "Ines", "Jon", "Kai", "Lena", "Milo", "Nora", "Omar", "Pia", "Ravi", "Sara"]


def item_params(i, d):
    sec = d.replace("_", "")[:3]
    cut = 1950 + 5 * (i % 6)
    busy = 50 + 10 * (i % 4)
    t = [f"{ADJ[(i + k * 4) % len(ADJ)]} {NOUN[(i * 5 + k * 3) % len(NOUN)]}" for k in range(3)]
    # (id, title, year, copies, format, times checked out)
    rows = [(1, t[0], 1960 + i, 2 + i % 3, "Print", 1), (2, t[1], 1990 + i % 20, 1, "Ebook", 0), (3, t[2], 1931 + i, 4, "Audio", 2)]
    return sec, cut, busy, rows


def member_params(i):
    bonus = i % 3
    rows = [(1, FIRST[i % 18], 1, 0), (2, FIRST[(i + 5) % 18], 2, 150 + 10 * i), (3, FIRST[(i + 11) % 18], 3, 0)]
    return bonus, rows


def title_key(t):
    return t.strip().lower()


def enum(lang, name):
    return {"sspur": name, "python": f"Format.{name.upper()}", "ts": q(name.lower()), "go": name}[lang]


def item_rows(lang, d, rows):
    c = d if lang in ("sspur", "python") else camel(d)
    out = []
    for a, t, y, n, f, k in rows:
        e = f"{c}_new({a}, {q(t)}, {y}, {n}, {enum(lang, f)})" if lang in ("sspur", "python") else f"{c}New({a}, {q(t)}, {y}, {n}, {enum(lang, f)})"
        for _ in range(k):
            e = f"{c}_checkout({e})" if lang in ("sspur", "python") else f"{c}Checkout({e})"
        out.append(e)
    return ", ".join(out)


def member_rows(lang, d, rows):
    snake = lang in ("sspur", "python")
    c = d if snake else camel(d)
    out = []
    for a, n, tier, fines in rows:
        e = f"{c}_new({a}, {q(n)}, {tier})" if snake else f"{c}New({a}, {q(n)}, {tier})"
        if fines:
            e = f"{c}_charge({e}, {fines})" if snake else f"{c}Charge({e}, {fines})"
        out.append(e)
    return ", ".join(out)


def strs(lang, xs):
    s = ", ".join(q(x) for x in xs)
    return "[]string{" + s + "}" if lang == "go" else "[" + s + "]"


# ---- SSPUR ------------------------------------------------------------------------------------

SSP_COMMON = '''type Format = Print | Ebook | Audio

fn format_name(f: Format) -> Str
= match f
  | Print => "print"
  | Ebook => "ebook"
  | Audio => "audio"

fn loan_days(f: Format) -> Int
= match f
  | Print => 21
  | Ebook => 14
  | Audio => 14

fn daily_fine(f: Format) -> Int
= match f
  | Print => 25
  | Ebook => 0
  | Audio => 10

fn fine_cap() -> Int
= 500

fn fine_for(f: Format, late: Int) -> Int
= if late <= 0 then 0 else min(daily_fine(f) * late, fine_cap())

fn due_day(start: Int, f: Format) -> Int
= start + loan_days(f)

fn days_late(due: Int, today: Int) -> Int
= max(0, today - due)

fn renewal(due: Int, f: Format) -> Int
= due + loan_days(f) / 2

fn cents(c: Int) -> Str
= "$" + (c / 100).str + "." + (c % 100).str.pad_left(2, "0")

fn shelf_code(section: Str, n: Int) -> Str
= "{section.upper}-{n}"

fn title_key(t: Str) -> Str
= t.trim.lower

fn max_loans(tier: Int) -> Int
= if tier >= 3 then 10 else if tier == 2 then 5 else 3

fn fine_block() -> Int
= 1000

fn badge(name: Str, tier: Int) -> Str
= "{name} (tier {tier})"

fn notice(name: Str, amount: Int) -> Str
= "Dear {name}, you owe {cents(amount)}."

fn join_lines(xs: List[Str]) -> Str
= xs.join("\\n")

fn formats() -> List[Format]
= [Print, Ebook, Audio]

test cents_ok = cents(1234) == "$12.34" and cents(5) == "$0.05"

test fine_ok = fine_for(Print, 10) == 250 and fine_for(Ebook, 30) == 0 and fine_for(Audio, 0) == 0

test due_ok = due_day(10, Ebook) == 24 and days_late(24, 20) == 0

test loans_ok = max_loans(1) == 3 and max_loans(2) == 5
'''

SSP_ITEM = '''type @T@ = {id: Int, title: Str, year: Int, copies: Int, out: Int, format: Format, section: Str}

fn @d@_new(id: Int, title: Str, year: Int, copies: Int, format: Format) -> @T@
= @T@{id, title, year, copies, out: 0, format, section: "@sec@"}

fn @d@_available(x: @T@) -> Int
= x.copies - x.out

fn @d@_can_lend(x: @T@) -> Bool
= @d@_available(x) > 0

fn @d@_checkout(x: @T@) -> @T@
= if @d@_can_lend(x) then x with out := x.out + 1 else x

fn @d@_checkin(x: @T@) -> @T@
= if x.out > 0 then x with out := x.out - 1 else x

fn @d@_add_copies(x: @T@, n: Int) -> @T@
= x with copies := x.copies + n

fn @d@_shelf(x: @T@) -> Str
= shelf_code(x.section, x.id)

fn @d@_label(x: @T@) -> Str
= "{x.title} ({x.year})"

fn @d@_due(x: @T@, start: Int) -> Int
= due_day(start, x.format)

fn @d@_fine(x: @T@, start: Int, today: Int) -> Int
= fine_for(x.format, days_late(@d@_due(x, start), today))

fn @d@_is_classic(x: @T@) -> Bool
= x.year < @cut@

fn @d@_usage(x: @T@) -> Int
= if x.copies == 0 then 0 else x.out * 100 / x.copies

fn @d@_line(x: @T@) -> Str
= "{@d@_label(x)} [{@d@_shelf(x)}] {@d@_available(x)}/{x.copies} {format_name(x.format)}"

fn @d@_catalog(xs: List[@T@]) -> Str
= join_lines(xs.map(@d@_line))

fn @d@_copies(xs: List[@T@]) -> Int
= xs.map(_.copies).sum

fn @d@_on_loan(xs: List[@T@]) -> Int
= xs.map(_.out).sum

fn @d@_of_format(xs: List[@T@], f: Format) -> List[@T@]
= xs.filter(_.format == f)

fn @d@_find(xs: List[@T@], id: Int) -> Opt[@T@]
= xs.find(_.id == id)

fn @d@_titles(xs: List[@T@]) -> List[Str]
= xs.map(x => title_key(x.title)).sort

fn @d@_oldest(xs: List[@T@]) -> Opt[@T@]
= xs.sort_by(_.year).first

fn @d@_busy(xs: List[@T@]) -> List[@T@]
= xs.filter(x => @d@_usage(x) >= @busy@)

fn @d@_sample() -> List[@T@]
= [@rows@]

test @d@_available_ok = @d@_available(@d@_checkout(@d@_new(1, "a", 2000, 2, Print))) == 1

test @d@_fine_ok = @d@_fine(@d@_new(1, "a", 2000, 1, Print), 0, 31) == 250

test @d@_titles_ok = @d@_titles(@d@_sample()) == @titles@

test @d@_line_ok = @d@_line(@d@_new(7, "Atlas", 1999, 3, Audio)) == "Atlas (1999) [@SEC@-7] 3/3 audio"
'''

SSP_RENEW = '''fn @d@_renewed_due(x: @T@, start: Int) -> Int
= renewal(@d@_due(x, start), x.format)

test @d@_renew_ok = @d@_renewed_due(@d@_new(1, "a", 2000, 1, Print), 0) == 31
'''

SSP_MEMBER = '''type @T@ = {id: Int, name: Str, tier: Int, loans: Int, fines: Int, active: Bool}

fn @d@_new(id: Int, name: Str, tier: Int) -> @T@
= @T@{id, name, tier, loans: 0, fines: 0, active: true}

fn @d@_limit(x: @T@) -> Int
= max_loans(x.tier) + @bonus@

fn @d@_can_borrow(x: @T@) -> Bool
= x.active and x.loans < @d@_limit(x) and x.fines < fine_block()

fn @d@_borrow(x: @T@) -> @T@
= if @d@_can_borrow(x) then x with loans := x.loans + 1 else x

fn @d@_give_back(x: @T@) -> @T@
= if x.loans > 0 then x with loans := x.loans - 1 else x

fn @d@_charge(x: @T@, c: Int) -> @T@
= x with fines := x.fines + c

fn @d@_pay(x: @T@, c: Int) -> @T@
= x with fines := x.fines - min(c, x.fines)

fn @d@_suspend(x: @T@) -> @T@
= x with active := false

fn @d@_upgrade(x: @T@) -> @T@
= x with tier := min(x.tier + 1, 3)

fn @d@_badge(x: @T@) -> Str
= badge(x.name, x.tier)

fn @d@_balance(x: @T@) -> Str
= cents(x.fines)

fn @d@_line(x: @T@) -> Str
= "{@d@_badge(x)}: {x.loans}/{@d@_limit(x)} loans, owes {@d@_balance(x)}"

fn @d@_roster(xs: List[@T@]) -> Str
= join_lines(xs.map(@d@_line))

fn @d@_owing(xs: List[@T@]) -> List[@T@]
= xs.filter(_.fines > 0)

fn @d@_total_fines(xs: List[@T@]) -> Int
= xs.map(_.fines).sum

fn @d@_active_count(xs: List[@T@]) -> Int
= xs.filter(_.active).len

fn @d@_find(xs: List[@T@], id: Int) -> Opt[@T@]
= xs.find(_.id == id)

fn @d@_names(xs: List[@T@]) -> List[Str]
= xs.map(_.name).sort

fn @d@_of_tier(xs: List[@T@], t: Int) -> List[@T@]
= xs.filter(_.tier == t)

fn @d@_sample() -> List[@T@]
= [@rows@]

test @d@_limit_ok = @d@_limit(@d@_new(1, "a", 2)) == @limit2@

test @d@_borrow_ok = @d@_borrow(@d@_charge(@d@_new(1, "a", 1), 1000)).loans == 0 and @d@_borrow(@d@_new(1, "a", 1)).loans == 1

test @d@_line_ok = @d@_line(@d@_charge(@d@_new(3, "Ann", 1), 250)) == "Ann (tier 1): 0/@limit1@ loans, owes $2.50"

test @d@_names_ok = @d@_names(@d@_sample()) == @names@
'''

SSP_REMIND = '''fn @d@_reminder(x: @T@) -> Str
= notice(x.name, x.fines)

test @d@_reminder_ok = @d@_reminder(@d@_charge(@d@_new(1, "Bo", 1), 1205)) == "Dear Bo, you owe $12.05."
'''


def ssp():
    out = [SSP_COMMON]
    for i, d in enumerate(ITEMS):
        sec, cut, busy, rows = item_params(i, d)
        kw = dict(T=cap(d), d=d, sec=sec, SEC=sec.upper(), cut=cut, busy=busy, rows=item_rows("sspur", d, rows),
                  titles=strs("sspur", sorted(title_key(r[1]) for r in rows)))
        out.append(fill(SSP_ITEM, **kw))
        if d in RENEWABLE:
            out.append(fill(SSP_RENEW, T=cap(d), d=d))
    for i, d in enumerate(MEMBERS):
        bonus, rows = member_params(i)
        kw = dict(T=cap(d), d=d, bonus=bonus, rows=member_rows("sspur", d, rows), limit1=3 + bonus, limit2=5 + bonus,
                  names=strs("sspur", sorted(r[1] for r in rows)))
        out.append(fill(SSP_MEMBER, **kw))
        if d in REMINDED:
            out.append(fill(SSP_REMIND, T=cap(d), d=d))
    return "\n".join(out)


# ---- Python -----------------------------------------------------------------------------------

PY_COMMON = '''from enum import Enum
from typing import List


class Format(Enum):
    PRINT = "print"
    EBOOK = "ebook"
    AUDIO = "audio"


def format_name(f: Format) -> str:
    return f.value


def loan_days(f: Format) -> int:
    return {Format.PRINT: 21, Format.EBOOK: 14, Format.AUDIO: 14}[f]


def daily_fine(f: Format) -> int:
    return {Format.PRINT: 25, Format.EBOOK: 0, Format.AUDIO: 10}[f]


def fine_cap() -> int:
    return 500


def fine_for(f: Format, late: int) -> int:
    if late <= 0:
        return 0
    return min(daily_fine(f) * late, fine_cap())


def due_day(start: int, f: Format) -> int:
    return start + loan_days(f)


def days_late(due: int, today: int) -> int:
    return max(0, today - due)


def renewal(due: int, f: Format) -> int:
    return due + loan_days(f) // 2


def cents(c: int) -> str:
    return f"${c // 100}.{c % 100:02d}"


def shelf_code(section: str, n: int) -> str:
    return f"{section.upper()}-{n}"


def title_key(t: str) -> str:
    return t.strip().lower()


def max_loans(tier: int) -> int:
    if tier >= 3:
        return 10
    if tier == 2:
        return 5
    return 3


def fine_block() -> int:
    return 1000


def badge(name: str, tier: int) -> str:
    return f"{name} (tier {tier})"


def notice(name: str, amount: int) -> str:
    return f"Dear {name}, you owe {cents(amount)}."


def join_lines(xs: List[str]) -> str:
    return "\\n".join(xs)


def formats() -> List[Format]:
    return list(Format)
'''

PY_COMMON_TEST = '''from library.common import *


def test_cents_ok():
    assert cents(1234) == "$12.34" and cents(5) == "$0.05"


def test_fine_ok():
    assert fine_for(Format.PRINT, 10) == 250 and fine_for(Format.EBOOK, 30) == 0 and fine_for(Format.AUDIO, 0) == 0


def test_due_ok():
    assert due_day(10, Format.EBOOK) == 24 and days_late(24, 20) == 0


def test_loans_ok():
    assert max_loans(1) == 3 and max_loans(2) == 5
'''

PY_ITEM = '''from dataclasses import dataclass, replace
from typing import List, Optional

from library.common import Format, days_late, due_day, fine_for, format_name, join_lines, @imports@shelf_code, title_key


@dataclass(frozen=True)
class @T@:
    id: int
    title: str
    year: int
    copies: int
    out: int
    format: Format
    section: str


def @d@_new(id: int, title: str, year: int, copies: int, format: Format) -> @T@:
    return @T@(id, title, year, copies, 0, format, "@sec@")


def @d@_available(x: @T@) -> int:
    return x.copies - x.out


def @d@_can_lend(x: @T@) -> bool:
    return @d@_available(x) > 0


def @d@_checkout(x: @T@) -> @T@:
    return replace(x, out=x.out + 1) if @d@_can_lend(x) else x


def @d@_checkin(x: @T@) -> @T@:
    return replace(x, out=x.out - 1) if x.out > 0 else x


def @d@_add_copies(x: @T@, n: int) -> @T@:
    return replace(x, copies=x.copies + n)


def @d@_shelf(x: @T@) -> str:
    return shelf_code(x.section, x.id)


def @d@_label(x: @T@) -> str:
    return f"{x.title} ({x.year})"


def @d@_due(x: @T@, start: int) -> int:
    return due_day(start, x.format)


def @d@_fine(x: @T@, start: int, today: int) -> int:
    return fine_for(x.format, days_late(@d@_due(x, start), today))


def @d@_is_classic(x: @T@) -> bool:
    return x.year < @cut@


def @d@_usage(x: @T@) -> int:
    return 0 if x.copies == 0 else x.out * 100 // x.copies


def @d@_line(x: @T@) -> str:
    return f"{@d@_label(x)} [{@d@_shelf(x)}] {@d@_available(x)}/{x.copies} {format_name(x.format)}"


def @d@_catalog(xs: List[@T@]) -> str:
    return join_lines([@d@_line(x) for x in xs])


def @d@_copies(xs: List[@T@]) -> int:
    return sum(x.copies for x in xs)


def @d@_on_loan(xs: List[@T@]) -> int:
    return sum(x.out for x in xs)


def @d@_of_format(xs: List[@T@], f: Format) -> List[@T@]:
    return [x for x in xs if x.format == f]


def @d@_find(xs: List[@T@], id: int) -> Optional[@T@]:
    return next((x for x in xs if x.id == id), None)


def @d@_titles(xs: List[@T@]) -> List[str]:
    return sorted(title_key(x.title) for x in xs)


def @d@_oldest(xs: List[@T@]) -> Optional[@T@]:
    return min(xs, key=lambda x: x.year, default=None)


def @d@_busy(xs: List[@T@]) -> List[@T@]:
    return [x for x in xs if @d@_usage(x) >= @busy@]


def @d@_sample() -> List[@T@]:
    return [@rows@]
'''

PY_RENEW = '''

def @d@_renewed_due(x: @T@, start: int) -> int:
    return renewal(@d@_due(x, start), x.format)
'''

PY_ITEM_TEST = '''from library.common import Format
from library.@d@ import *


def test_@d@_available_ok():
    assert @d@_available(@d@_checkout(@d@_new(1, "a", 2000, 2, Format.PRINT))) == 1


def test_@d@_fine_ok():
    assert @d@_fine(@d@_new(1, "a", 2000, 1, Format.PRINT), 0, 31) == 250


def test_@d@_titles_ok():
    assert @d@_titles(@d@_sample()) == @titles@


def test_@d@_line_ok():
    assert @d@_line(@d@_new(7, "Atlas", 1999, 3, Format.AUDIO)) == "Atlas (1999) [@SEC@-7] 3/3 audio"
'''

PY_RENEW_TEST = '''

def test_@d@_renew_ok():
    assert @d@_renewed_due(@d@_new(1, "a", 2000, 1, Format.PRINT), 0) == 31
'''

PY_MEMBER = '''from dataclasses import dataclass, replace
from typing import List, Optional

from library.common import badge, cents, fine_block, join_lines, max_loans@imports@


@dataclass(frozen=True)
class @T@:
    id: int
    name: str
    tier: int
    loans: int
    fines: int
    active: bool


def @d@_new(id: int, name: str, tier: int) -> @T@:
    return @T@(id, name, tier, 0, 0, True)


def @d@_limit(x: @T@) -> int:
    return max_loans(x.tier) + @bonus@


def @d@_can_borrow(x: @T@) -> bool:
    return x.active and x.loans < @d@_limit(x) and x.fines < fine_block()


def @d@_borrow(x: @T@) -> @T@:
    return replace(x, loans=x.loans + 1) if @d@_can_borrow(x) else x


def @d@_give_back(x: @T@) -> @T@:
    return replace(x, loans=x.loans - 1) if x.loans > 0 else x


def @d@_charge(x: @T@, c: int) -> @T@:
    return replace(x, fines=x.fines + c)


def @d@_pay(x: @T@, c: int) -> @T@:
    return replace(x, fines=x.fines - min(c, x.fines))


def @d@_suspend(x: @T@) -> @T@:
    return replace(x, active=False)


def @d@_upgrade(x: @T@) -> @T@:
    return replace(x, tier=min(x.tier + 1, 3))


def @d@_badge(x: @T@) -> str:
    return badge(x.name, x.tier)


def @d@_balance(x: @T@) -> str:
    return cents(x.fines)


def @d@_line(x: @T@) -> str:
    return f"{@d@_badge(x)}: {x.loans}/{@d@_limit(x)} loans, owes {@d@_balance(x)}"


def @d@_roster(xs: List[@T@]) -> str:
    return join_lines([@d@_line(x) for x in xs])


def @d@_owing(xs: List[@T@]) -> List[@T@]:
    return [x for x in xs if x.fines > 0]


def @d@_total_fines(xs: List[@T@]) -> int:
    return sum(x.fines for x in xs)


def @d@_active_count(xs: List[@T@]) -> int:
    return len([x for x in xs if x.active])


def @d@_find(xs: List[@T@], id: int) -> Optional[@T@]:
    return next((x for x in xs if x.id == id), None)


def @d@_names(xs: List[@T@]) -> List[str]:
    return sorted(x.name for x in xs)


def @d@_of_tier(xs: List[@T@], t: int) -> List[@T@]:
    return [x for x in xs if x.tier == t]


def @d@_sample() -> List[@T@]:
    return [@rows@]
'''

PY_REMIND = '''

def @d@_reminder(x: @T@) -> str:
    return notice(x.name, x.fines)
'''

PY_MEMBER_TEST = '''from library.@d@ import *


def test_@d@_limit_ok():
    assert @d@_limit(@d@_new(1, "a", 2)) == @limit2@


def test_@d@_borrow_ok():
    assert @d@_borrow(@d@_charge(@d@_new(1, "a", 1), 1000)).loans == 0 and @d@_borrow(@d@_new(1, "a", 1)).loans == 1


def test_@d@_line_ok():
    assert @d@_line(@d@_charge(@d@_new(3, "Ann", 1), 250)) == "Ann (tier 1): 0/@limit1@ loans, owes $2.50"


def test_@d@_names_ok():
    assert @d@_names(@d@_sample()) == @names@
'''

PY_REMIND_TEST = '''

def test_@d@_reminder_ok():
    assert @d@_reminder(@d@_charge(@d@_new(1, "Bo", 1), 1205)) == "Dear Bo, you owe $12.05."
'''


def py(root):
    write(os.path.join(root, "library", "__init__.py"), "")
    write(os.path.join(root, "library", "common.py"), PY_COMMON)
    write(os.path.join(root, "tests", "test_common.py"), PY_COMMON_TEST)
    for i, d in enumerate(ITEMS):
        sec, cut, busy, rows = item_params(i, d)
        kw = dict(T=cap(d), d=d, sec=sec, cut=cut, busy=busy, rows=item_rows("python", d, rows),
                  imports="renewal, " if d in RENEWABLE else "")
        src = fill(PY_ITEM, **kw) + (fill(PY_RENEW, T=cap(d), d=d) if d in RENEWABLE else "")
        test = fill(PY_ITEM_TEST, d=d, SEC=sec.upper(), titles=strs("python", sorted(title_key(r[1]) for r in rows)))
        test += fill(PY_RENEW_TEST, d=d) if d in RENEWABLE else ""
        write(os.path.join(root, "library", d + ".py"), src)
        write(os.path.join(root, "tests", f"test_{d}.py"), test)
    for i, d in enumerate(MEMBERS):
        bonus, rows = member_params(i)
        kw = dict(T=cap(d), d=d, bonus=bonus, rows=member_rows("python", d, rows), imports=", notice" if d in REMINDED else "")
        src = fill(PY_MEMBER, **kw) + (fill(PY_REMIND, T=cap(d), d=d) if d in REMINDED else "")
        test = fill(PY_MEMBER_TEST, d=d, limit1=3 + bonus, limit2=5 + bonus, names=strs("python", sorted(r[1] for r in rows)))
        test += fill(PY_REMIND_TEST, d=d) if d in REMINDED else ""
        write(os.path.join(root, "library", d + ".py"), src)
        write(os.path.join(root, "tests", f"test_{d}.py"), test)


# ---- TypeScript -------------------------------------------------------------------------------

TS_COMMON = '''export type Format = "print" | "ebook" | "audio";

export function formatName(f: Format): string {
  return f;
}

export function loanDays(f: Format): number {
  switch (f) {
    case "print":
      return 21;
    case "ebook":
      return 14;
    case "audio":
      return 14;
  }
}

export function dailyFine(f: Format): number {
  switch (f) {
    case "print":
      return 25;
    case "ebook":
      return 0;
    case "audio":
      return 10;
  }
}

export function fineCap(): number {
  return 500;
}

export function fineFor(f: Format, late: number): number {
  if (late <= 0) return 0;
  return Math.min(dailyFine(f) * late, fineCap());
}

export function dueDay(start: number, f: Format): number {
  return start + loanDays(f);
}

export function daysLate(due: number, today: number): number {
  return Math.max(0, today - due);
}

export function renewal(due: number, f: Format): number {
  return due + Math.floor(loanDays(f) / 2);
}

export function cents(c: number): string {
  return `$${Math.floor(c / 100)}.${String(c % 100).padStart(2, "0")}`;
}

export function shelfCode(section: string, n: number): string {
  return `${section.toUpperCase()}-${n}`;
}

export function titleKey(t: string): string {
  return t.trim().toLowerCase();
}

export function maxLoans(tier: number): number {
  if (tier >= 3) return 10;
  if (tier === 2) return 5;
  return 3;
}

export function fineBlock(): number {
  return 1000;
}

export function badge(name: string, tier: number): string {
  return `${name} (tier ${tier})`;
}

export function notice(name: string, amount: number): string {
  return `Dear ${name}, you owe ${cents(amount)}.`;
}

export function joinLines(xs: readonly string[]): string {
  return xs.join("\\n");
}

export function formats(): Format[] {
  return ["print", "ebook", "audio"];
}
'''

TS_COMMON_TEST = '''import { test } from "node:test";
import assert from "node:assert/strict";
import { cents, daysLate, dueDay, fineFor, maxLoans } from "../library/common";

test("cents ok", () => {
  assert.ok(cents(1234) === "$12.34" && cents(5) === "$0.05");
});

test("fine ok", () => {
  assert.ok(fineFor("print", 10) === 250 && fineFor("ebook", 30) === 0 && fineFor("audio", 0) === 0);
});

test("due ok", () => {
  assert.ok(dueDay(10, "ebook") === 24 && daysLate(24, 20) === 0);
});

test("loans ok", () => {
  assert.ok(maxLoans(1) === 3 && maxLoans(2) === 5);
});
'''

TS_ITEM = '''import { Format, daysLate, dueDay, fineFor, formatName, joinLines, @imports@shelfCode, titleKey } from "./common";

export interface @T@ {
  readonly id: number;
  readonly title: string;
  readonly year: number;
  readonly copies: number;
  readonly out: number;
  readonly format: Format;
  readonly section: string;
}

export function @c@New(id: number, title: string, year: number, copies: number, format: Format): @T@ {
  return { id, title, year, copies, out: 0, format, section: "@sec@" };
}

export function @c@Available(x: @T@): number {
  return x.copies - x.out;
}

export function @c@CanLend(x: @T@): boolean {
  return @c@Available(x) > 0;
}

export function @c@Checkout(x: @T@): @T@ {
  return @c@CanLend(x) ? { ...x, out: x.out + 1 } : x;
}

export function @c@Checkin(x: @T@): @T@ {
  return x.out > 0 ? { ...x, out: x.out - 1 } : x;
}

export function @c@AddCopies(x: @T@, n: number): @T@ {
  return { ...x, copies: x.copies + n };
}

export function @c@Shelf(x: @T@): string {
  return shelfCode(x.section, x.id);
}

export function @c@Label(x: @T@): string {
  return `${x.title} (${x.year})`;
}

export function @c@Due(x: @T@, start: number): number {
  return dueDay(start, x.format);
}

export function @c@Fine(x: @T@, start: number, today: number): number {
  return fineFor(x.format, daysLate(@c@Due(x, start), today));
}

export function @c@IsClassic(x: @T@): boolean {
  return x.year < @cut@;
}

export function @c@Usage(x: @T@): number {
  return x.copies === 0 ? 0 : Math.floor((x.out * 100) / x.copies);
}

export function @c@Line(x: @T@): string {
  return `${@c@Label(x)} [${@c@Shelf(x)}] ${@c@Available(x)}/${x.copies} ${formatName(x.format)}`;
}

export function @c@Catalog(xs: readonly @T@[]): string {
  return joinLines(xs.map(@c@Line));
}

export function @c@Copies(xs: readonly @T@[]): number {
  return xs.reduce((s, x) => s + x.copies, 0);
}

export function @c@OnLoan(xs: readonly @T@[]): number {
  return xs.reduce((s, x) => s + x.out, 0);
}

export function @c@OfFormat(xs: readonly @T@[], f: Format): @T@[] {
  return xs.filter((x) => x.format === f);
}

export function @c@Find(xs: readonly @T@[], id: number): @T@ | undefined {
  return xs.find((x) => x.id === id);
}

export function @c@Titles(xs: readonly @T@[]): string[] {
  return xs.map((x) => titleKey(x.title)).sort();
}

export function @c@Oldest(xs: readonly @T@[]): @T@ | undefined {
  return xs.reduce<@T@ | undefined>((best, x) => (best === undefined || x.year < best.year ? x : best), undefined);
}

export function @c@Busy(xs: readonly @T@[]): @T@[] {
  return xs.filter((x) => @c@Usage(x) >= @busy@);
}

export function @c@Sample(): @T@[] {
  return [@rows@];
}
'''

TS_RENEW = '''
export function @c@RenewedDue(x: @T@, start: number): number {
  return renewal(@c@Due(x, start), x.format);
}
'''

TS_ITEM_TEST = '''import { test } from "node:test";
import assert from "node:assert/strict";
import { @imports@ } from "../library/@file@";

test("@c@ available ok", () => {
  assert.equal(@c@Available(@c@Checkout(@c@New(1, "a", 2000, 2, "print"))), 1);
});

test("@c@ fine ok", () => {
  assert.equal(@c@Fine(@c@New(1, "a", 2000, 1, "print"), 0, 31), 250);
});

test("@c@ titles ok", () => {
  assert.deepEqual(@c@Titles(@c@Sample()), @titles@);
});

test("@c@ line ok", () => {
  assert.equal(@c@Line(@c@New(7, "Atlas", 1999, 3, "audio")), "Atlas (1999) [@SEC@-7] 3/3 audio");
});
'''

TS_RENEW_TEST = '''
test("@c@ renew ok", () => {
  assert.equal(@c@RenewedDue(@c@New(1, "a", 2000, 1, "print"), 0), 31);
});
'''

TS_MEMBER = '''import { badge, cents, fineBlock, joinLines, maxLoans@imports@ } from "./common";

export interface @T@ {
  readonly id: number;
  readonly name: string;
  readonly tier: number;
  readonly loans: number;
  readonly fines: number;
  readonly active: boolean;
}

export function @c@New(id: number, name: string, tier: number): @T@ {
  return { id, name, tier, loans: 0, fines: 0, active: true };
}

export function @c@Limit(x: @T@): number {
  return maxLoans(x.tier) + @bonus@;
}

export function @c@CanBorrow(x: @T@): boolean {
  return x.active && x.loans < @c@Limit(x) && x.fines < fineBlock();
}

export function @c@Borrow(x: @T@): @T@ {
  return @c@CanBorrow(x) ? { ...x, loans: x.loans + 1 } : x;
}

export function @c@GiveBack(x: @T@): @T@ {
  return x.loans > 0 ? { ...x, loans: x.loans - 1 } : x;
}

export function @c@Charge(x: @T@, c: number): @T@ {
  return { ...x, fines: x.fines + c };
}

export function @c@Pay(x: @T@, c: number): @T@ {
  return { ...x, fines: x.fines - Math.min(c, x.fines) };
}

export function @c@Suspend(x: @T@): @T@ {
  return { ...x, active: false };
}

export function @c@Upgrade(x: @T@): @T@ {
  return { ...x, tier: Math.min(x.tier + 1, 3) };
}

export function @c@Badge(x: @T@): string {
  return badge(x.name, x.tier);
}

export function @c@Balance(x: @T@): string {
  return cents(x.fines);
}

export function @c@Line(x: @T@): string {
  return `${@c@Badge(x)}: ${x.loans}/${@c@Limit(x)} loans, owes ${@c@Balance(x)}`;
}

export function @c@Roster(xs: readonly @T@[]): string {
  return joinLines(xs.map(@c@Line));
}

export function @c@Owing(xs: readonly @T@[]): @T@[] {
  return xs.filter((x) => x.fines > 0);
}

export function @c@TotalFines(xs: readonly @T@[]): number {
  return xs.reduce((s, x) => s + x.fines, 0);
}

export function @c@ActiveCount(xs: readonly @T@[]): number {
  return xs.filter((x) => x.active).length;
}

export function @c@Find(xs: readonly @T@[], id: number): @T@ | undefined {
  return xs.find((x) => x.id === id);
}

export function @c@Names(xs: readonly @T@[]): string[] {
  return xs.map((x) => x.name).sort();
}

export function @c@OfTier(xs: readonly @T@[], t: number): @T@[] {
  return xs.filter((x) => x.tier === t);
}

export function @c@Sample(): @T@[] {
  return [@rows@];
}
'''

TS_REMIND = '''
export function @c@Reminder(x: @T@): string {
  return notice(x.name, x.fines);
}
'''

TS_MEMBER_TEST = '''import { test } from "node:test";
import assert from "node:assert/strict";
import { @imports@ } from "../library/@file@";

test("@c@ limit ok", () => {
  assert.equal(@c@Limit(@c@New(1, "a", 2)), @limit2@);
});

test("@c@ borrow ok", () => {
  assert.ok(@c@Borrow(@c@Charge(@c@New(1, "a", 1), 1000)).loans === 0 && @c@Borrow(@c@New(1, "a", 1)).loans === 1);
});

test("@c@ line ok", () => {
  assert.equal(@c@Line(@c@Charge(@c@New(3, "Ann", 1), 250)), "Ann (tier 1): 0/@limit1@ loans, owes $2.50");
});

test("@c@ names ok", () => {
  assert.deepEqual(@c@Names(@c@Sample()), @names@);
});
'''

TS_REMIND_TEST = '''
test("@c@ reminder ok", () => {
  assert.equal(@c@Reminder(@c@Charge(@c@New(1, "Bo", 1), 1205)), "Dear Bo, you owe $12.05.");
});
'''


def ts(root):
    write(os.path.join(root, "tsconfig.json"), ts_tsconfig("library"))
    write(os.path.join(root, "library", "common.ts"), TS_COMMON)
    write(os.path.join(root, "tests", "common.test.ts"), TS_COMMON_TEST)
    for i, d in enumerate(ITEMS):
        sec, cut, busy, rows = item_params(i, d)
        c = camel(d)
        src = fill(TS_ITEM, T=cap(d), c=c, sec=sec, cut=cut, busy=busy, rows=item_rows("ts", d, rows),
                   imports="renewal, " if d in RENEWABLE else "")
        imports = [c + x for x in ("Available", "Checkout", "Fine", "Line", "New", "Sample", "Titles")]
        if d in RENEWABLE:
            src += fill(TS_RENEW, T=cap(d), c=c)
            imports.append(c + "RenewedDue")
        test = fill(TS_ITEM_TEST, c=c, file=d, imports=", ".join(sorted(imports)), SEC=sec.upper(),
                    titles=strs("ts", sorted(title_key(r[1]) for r in rows)))
        test += fill(TS_RENEW_TEST, c=c) if d in RENEWABLE else ""
        write(os.path.join(root, "library", d + ".ts"), src)
        write(os.path.join(root, "tests", f"{d}.test.ts"), test)
    for i, d in enumerate(MEMBERS):
        bonus, rows = member_params(i)
        c = camel(d)
        src = fill(TS_MEMBER, T=cap(d), c=c, bonus=bonus, rows=member_rows("ts", d, rows), imports=", notice" if d in REMINDED else "")
        imports = [c + x for x in ("Borrow", "Charge", "Limit", "Line", "Names", "New", "Sample")]
        if d in REMINDED:
            src += fill(TS_REMIND, T=cap(d), c=c)
            imports.append(c + "Reminder")
        test = fill(TS_MEMBER_TEST, c=c, file=d, imports=", ".join(sorted(imports)), limit1=3 + bonus, limit2=5 + bonus,
                    names=strs("ts", sorted(r[1] for r in rows)))
        test += fill(TS_REMIND_TEST, c=c) if d in REMINDED else ""
        write(os.path.join(root, "library", d + ".ts"), src)
        write(os.path.join(root, "tests", f"{d}.test.ts"), test)


# ---- Go ---------------------------------------------------------------------------------------

GO_COMMON = '''package library

import (
	"fmt"
	"strings"
)

type Format int

const (
	Print Format = iota
	Ebook
	Audio
)

func formatName(f Format) string {
	switch f {
	case Print:
		return "print"
	case Ebook:
		return "ebook"
	case Audio:
		return "audio"
	}
	panic(fmt.Sprintf("unknown format %d", int(f)))
}

func loanDays(f Format) int {
	switch f {
	case Print:
		return 21
	case Ebook:
		return 14
	case Audio:
		return 14
	}
	panic(fmt.Sprintf("unknown format %d", int(f)))
}

func dailyFine(f Format) int {
	switch f {
	case Print:
		return 25
	case Ebook:
		return 0
	case Audio:
		return 10
	}
	panic(fmt.Sprintf("unknown format %d", int(f)))
}

func fineCap() int {
	return 500
}

func fineFor(f Format, late int) int {
	if late <= 0 {
		return 0
	}
	return min(dailyFine(f)*late, fineCap())
}

func dueDay(start int, f Format) int {
	return start + loanDays(f)
}

func daysLate(due, today int) int {
	return max(0, today-due)
}

func renewal(due int, f Format) int {
	return due + loanDays(f)/2
}

func cents(c int) string {
	return fmt.Sprintf("$%d.%02d", c/100, c%100)
}

func shelfCode(section string, n int) string {
	return fmt.Sprintf("%s-%d", strings.ToUpper(section), n)
}

func titleKey(t string) string {
	return strings.ToLower(strings.TrimSpace(t))
}

func maxLoans(tier int) int {
	if tier >= 3 {
		return 10
	}
	if tier == 2 {
		return 5
	}
	return 3
}

func fineBlock() int {
	return 1000
}

func badge(name string, tier int) string {
	return fmt.Sprintf("%s (tier %d)", name, tier)
}

func notice(name string, amount int) string {
	return fmt.Sprintf("Dear %s, you owe %s.", name, cents(amount))
}

func joinLines(xs []string) string {
	return strings.Join(xs, "\\n")
}

func formats() []Format {
	return []Format{Print, Ebook, Audio}
}
'''

GO_COMMON_TEST = '''package library

import "testing"

func TestCentsOk(t *testing.T) {
	if !(cents(1234) == "$12.34" && cents(5) == "$0.05") {
		t.Fatal("cents")
	}
}

func TestFineOk(t *testing.T) {
	if !(fineFor(Print, 10) == 250 && fineFor(Ebook, 30) == 0 && fineFor(Audio, 0) == 0) {
		t.Fatal("fineFor")
	}
}

func TestDueOk(t *testing.T) {
	if !(dueDay(10, Ebook) == 24 && daysLate(24, 20) == 0) {
		t.Fatal("due")
	}
}

func TestLoansOk(t *testing.T) {
	if !(maxLoans(1) == 3 && maxLoans(2) == 5) {
		t.Fatal("maxLoans")
	}
}
'''

GO_ITEM = '''package library

import (
	"fmt"
	"sort"
)

type @T@ struct {
	id      int
	title   string
	year    int
	copies  int
	out     int
	format  Format
	section string
}

func @c@New(id int, title string, year int, copies int, format Format) @T@ {
	return @T@{id, title, year, copies, 0, format, "@sec@"}
}

func @c@Available(x @T@) int {
	return x.copies - x.out
}

func @c@CanLend(x @T@) bool {
	return @c@Available(x) > 0
}

func @c@Checkout(x @T@) @T@ {
	if @c@CanLend(x) {
		x.out++
	}
	return x
}

func @c@Checkin(x @T@) @T@ {
	if x.out > 0 {
		x.out--
	}
	return x
}

func @c@AddCopies(x @T@, n int) @T@ {
	x.copies += n
	return x
}

func @c@Shelf(x @T@) string {
	return shelfCode(x.section, x.id)
}

func @c@Label(x @T@) string {
	return fmt.Sprintf("%s (%d)", x.title, x.year)
}

func @c@Due(x @T@, start int) int {
	return dueDay(start, x.format)
}

func @c@Fine(x @T@, start, today int) int {
	return fineFor(x.format, daysLate(@c@Due(x, start), today))
}

func @c@IsClassic(x @T@) bool {
	return x.year < @cut@
}

func @c@Usage(x @T@) int {
	if x.copies == 0 {
		return 0
	}
	return x.out * 100 / x.copies
}

func @c@Line(x @T@) string {
	return fmt.Sprintf("%s [%s] %d/%d %s", @c@Label(x), @c@Shelf(x), @c@Available(x), x.copies, formatName(x.format))
}

func @c@Catalog(xs []@T@) string {
	lines := []string{}
	for _, x := range xs {
		lines = append(lines, @c@Line(x))
	}
	return joinLines(lines)
}

func @c@Copies(xs []@T@) int {
	s := 0
	for _, x := range xs {
		s += x.copies
	}
	return s
}

func @c@OnLoan(xs []@T@) int {
	s := 0
	for _, x := range xs {
		s += x.out
	}
	return s
}

func @c@OfFormat(xs []@T@, f Format) []@T@ {
	out := []@T@{}
	for _, x := range xs {
		if x.format == f {
			out = append(out, x)
		}
	}
	return out
}

func @c@Find(xs []@T@, id int) (@T@, bool) {
	for _, x := range xs {
		if x.id == id {
			return x, true
		}
	}
	return @T@{}, false
}

func @c@Titles(xs []@T@) []string {
	out := []string{}
	for _, x := range xs {
		out = append(out, titleKey(x.title))
	}
	sort.Strings(out)
	return out
}

func @c@Oldest(xs []@T@) (@T@, bool) {
	if len(xs) == 0 {
		return @T@{}, false
	}
	best := xs[0]
	for _, x := range xs[1:] {
		if x.year < best.year {
			best = x
		}
	}
	return best, true
}

func @c@Busy(xs []@T@) []@T@ {
	out := []@T@{}
	for _, x := range xs {
		if @c@Usage(x) >= @busy@ {
			out = append(out, x)
		}
	}
	return out
}

func @c@Sample() []@T@ {
	return []@T@{@rows@}
}
'''

GO_RENEW = '''
func @c@RenewedDue(x @T@, start int) int {
	return renewal(@c@Due(x, start), x.format)
}
'''

GO_ITEM_TEST = '''package library

import (
	"reflect"
	"testing"
)

func Test@T@AvailableOk(t *testing.T) {
	if got := @c@Available(@c@Checkout(@c@New(1, "a", 2000, 2, Print))); got != 1 {
		t.Fatalf("got %d", got)
	}
}

func Test@T@FineOk(t *testing.T) {
	if got := @c@Fine(@c@New(1, "a", 2000, 1, Print), 0, 31); got != 250 {
		t.Fatalf("got %d", got)
	}
}

func Test@T@TitlesOk(t *testing.T) {
	if got := @c@Titles(@c@Sample()); !reflect.DeepEqual(got, @titles@) {
		t.Fatalf("got %v", got)
	}
}

func Test@T@LineOk(t *testing.T) {
	if got := @c@Line(@c@New(7, "Atlas", 1999, 3, Audio)); got != "Atlas (1999) [@SEC@-7] 3/3 audio" {
		t.Fatalf("got %q", got)
	}
}
'''

GO_RENEW_TEST = '''
func Test@T@RenewOk(t *testing.T) {
	if got := @c@RenewedDue(@c@New(1, "a", 2000, 1, Print), 0); got != 31 {
		t.Fatalf("got %d", got)
	}
}
'''

GO_MEMBER = '''package library

import (
	"fmt"
	"sort"
)

type @T@ struct {
	id     int
	name   string
	tier   int
	loans  int
	fines  int
	active bool
}

func @c@New(id int, name string, tier int) @T@ {
	return @T@{id, name, tier, 0, 0, true}
}

func @c@Limit(x @T@) int {
	return maxLoans(x.tier) + @bonus@
}

func @c@CanBorrow(x @T@) bool {
	return x.active && x.loans < @c@Limit(x) && x.fines < fineBlock()
}

func @c@Borrow(x @T@) @T@ {
	if @c@CanBorrow(x) {
		x.loans++
	}
	return x
}

func @c@GiveBack(x @T@) @T@ {
	if x.loans > 0 {
		x.loans--
	}
	return x
}

func @c@Charge(x @T@, c int) @T@ {
	x.fines += c
	return x
}

func @c@Pay(x @T@, c int) @T@ {
	x.fines -= min(c, x.fines)
	return x
}

func @c@Suspend(x @T@) @T@ {
	x.active = false
	return x
}

func @c@Upgrade(x @T@) @T@ {
	x.tier = min(x.tier+1, 3)
	return x
}

func @c@Badge(x @T@) string {
	return badge(x.name, x.tier)
}

func @c@Balance(x @T@) string {
	return cents(x.fines)
}

func @c@Line(x @T@) string {
	return fmt.Sprintf("%s: %d/%d loans, owes %s", @c@Badge(x), x.loans, @c@Limit(x), @c@Balance(x))
}

func @c@Roster(xs []@T@) string {
	lines := []string{}
	for _, x := range xs {
		lines = append(lines, @c@Line(x))
	}
	return joinLines(lines)
}

func @c@Owing(xs []@T@) []@T@ {
	out := []@T@{}
	for _, x := range xs {
		if x.fines > 0 {
			out = append(out, x)
		}
	}
	return out
}

func @c@TotalFines(xs []@T@) int {
	s := 0
	for _, x := range xs {
		s += x.fines
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

func @c@Names(xs []@T@) []string {
	out := []string{}
	for _, x := range xs {
		out = append(out, x.name)
	}
	sort.Strings(out)
	return out
}

func @c@OfTier(xs []@T@, t int) []@T@ {
	out := []@T@{}
	for _, x := range xs {
		if x.tier == t {
			out = append(out, x)
		}
	}
	return out
}

func @c@Sample() []@T@ {
	return []@T@{@rows@}
}
'''

GO_REMIND = '''
func @c@Reminder(x @T@) string {
	return notice(x.name, x.fines)
}
'''

GO_MEMBER_TEST = '''package library

import (
	"reflect"
	"testing"
)

func Test@T@LimitOk(t *testing.T) {
	if got := @c@Limit(@c@New(1, "a", 2)); got != @limit2@ {
		t.Fatalf("got %d", got)
	}
}

func Test@T@BorrowOk(t *testing.T) {
	if !(@c@Borrow(@c@Charge(@c@New(1, "a", 1), 1000)).loans == 0 && @c@Borrow(@c@New(1, "a", 1)).loans == 1) {
		t.Fatal("borrow")
	}
}

func Test@T@LineOk(t *testing.T) {
	if got := @c@Line(@c@Charge(@c@New(3, "Ann", 1), 250)); got != "Ann (tier 1): 0/@limit1@ loans, owes $2.50" {
		t.Fatalf("got %q", got)
	}
}

func Test@T@NamesOk(t *testing.T) {
	if got := @c@Names(@c@Sample()); !reflect.DeepEqual(got, @names@) {
		t.Fatalf("got %v", got)
	}
}
'''

GO_REMIND_TEST = '''
func Test@T@ReminderOk(t *testing.T) {
	if got := @c@Reminder(@c@Charge(@c@New(1, "Bo", 1), 1205)); got != "Dear Bo, you owe $12.05." {
		t.Fatalf("got %q", got)
	}
}
'''


def go(root):
    write(os.path.join(root, "go.mod"), GOMOD)
    write(os.path.join(root, "library", "common.go"), GO_COMMON)
    write(os.path.join(root, "library", "common_test.go"), GO_COMMON_TEST)
    for i, d in enumerate(ITEMS):
        sec, cut, busy, rows = item_params(i, d)
        c, T = camel(d), cap(d)
        src = fill(GO_ITEM, T=T, c=c, sec=sec, cut=cut, busy=busy, rows=item_rows("go", d, rows))
        test = fill(GO_ITEM_TEST, T=T, c=c, SEC=sec.upper(), titles=strs("go", sorted(title_key(r[1]) for r in rows)))
        if d in RENEWABLE:
            src += fill(GO_RENEW, T=T, c=c)
            test += fill(GO_RENEW_TEST, T=T, c=c)
        write(os.path.join(root, "library", d + ".go"), src)
        write(os.path.join(root, "library", f"{d}_test.go"), test)
    for i, d in enumerate(MEMBERS):
        bonus, rows = member_params(i)
        c, T = camel(d), cap(d)
        src = fill(GO_MEMBER, T=T, c=c, bonus=bonus, rows=member_rows("go", d, rows))
        test = fill(GO_MEMBER_TEST, T=T, c=c, limit1=3 + bonus, limit2=5 + bonus, names=strs("go", sorted(r[1] for r in rows)))
        if d in REMINDED:
            src += fill(GO_REMIND, T=T, c=c)
            test += fill(GO_REMIND_TEST, T=T, c=c)
        write(os.path.join(root, "library", d + ".go"), src)
        write(os.path.join(root, "library", f"{d}_test.go"), test)
    gofmt(os.path.join(root, "library"))


def generate(out):
    os.makedirs(out, exist_ok=True)
    write(os.path.join(out, "start.ssp"), ssp())
    py(os.path.join(out, "py"))
    ts(os.path.join(out, "ts"))
    go(os.path.join(out, "go"))


if __name__ == "__main__":
    generate(sys.argv[1])
