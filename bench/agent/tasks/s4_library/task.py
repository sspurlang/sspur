"""s4_library: five targeted changes on the library codebase (a different change set from s3)."""
import os, re, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "large"))
import gen_library as g  # noqa: E402
from lib import C, Eq, F, L, P, V, camel  # noqa: E402

CODEBASE = "library"
TITLE = "Library, second task: five targeted changes in a large codebase"
EXAMPLE = "title_key is titleKey"
STEPS = [
    "Bug: title_key must ignore a leading article. After trimming and lowercasing, a leading \"the \" is dropped, so \"The Hobbit\" and \" the hobbit\" both key as \"hobbit\"; \"Theory\" stays \"theory\".",
    "Members are now blocked from borrowing once they owe $5.00 (it was $10.00).",
    "Rename notice to fine_notice everywhere; its text does not change.",
    "Theses record their advisor: thesis_new gets a new last parameter advisor (a Str), stored in a new field advisor of Thesis. Every existing caller passes \"\". When the advisor is not empty, thesis_line ends with \" / \" and the advisor.",
    "Add staff_can_take(m, b): true when staff member m may borrow (staff_can_borrow) and book b can be lent (book_can_lend).",
    "Add a test for each change. All existing tests must still pass.",
]
IFACE = {
    "sspur": "fn fine_notice(name: Str, amount: Int) -> Str (was notice); fn thesis_new(id: Int, title: Str, year: Int, copies: Int, format: Format, advisor: Str) -> Thesis (Thesis gets a field advisor: Str); fn staff_can_take(m: Staff, b: Book) -> Bool; title_key, fine_block and the other existing functions keep their signatures",
    "python": "library.common.fine_notice(name, amount) -> str (was notice); library.thesis.thesis_new(id, title, year, copies, format, advisor) -> Thesis (a new field advisor); library.staff.staff_can_take(m, b) -> bool; title_key, fine_block and the other existing functions keep their modules and signatures",
    "ts": "library/common.ts: export function fineNotice(name: string, amount: number): string (was notice); library/thesis.ts: export function thesisNew(id: number, title: string, year: number, copies: number, format: Format, advisor: string): Thesis (Thesis gets readonly advisor: string); library/staff.ts: export function staffCanTake(m: Staff, b: Book): boolean; titleKey, fineBlock and the other existing functions keep their modules and signatures",
    "go": "func fineNotice(name string, amount int) string (was notice); func thesisNew(id int, title string, year int, copies int, format Format, advisor string) Thesis (Thesis gets a field advisor string); func staffCanTake(m Staff, b Book) bool, all in package library; titleKey, fineBlock and the other existing functions keep their signatures",
}

PRINT, AUDIO = V("Format", "Print"), V("Format", "Audio")


def book(*a):
    return C("book_new", *a)


def staff(*a):
    return C("staff_new", *a)


def charged(d, n, amount, tier=1):
    return C(d + "_charge", C(d + "_new", 1, n, tier), amount)


B = book(1, "b", 2000, 1, PRINT)

HIDDEN = [
    ("hidden_title_key", [Eq(C("title_key", "The Hobbit"), "hobbit"), Eq(C("title_key", "  the Hobbit "), "hobbit"), Eq(C("title_key", "THE END"), "end"),
                          Eq(C("title_key", "Theory"), "theory"), Eq(C("title_key", "Other Worlds"), "other worlds")]),
    ("hidden_titles_used", [Eq(C("book_titles", L("Book", [book(1, "The Lost River", 2000, 1, PRINT), book(2, "Archive", 2000, 1, PRINT), book(3, "the Garden", 2000, 1, PRINT)])),
                               ["archive", "garden", "lost river"]),
                            Eq(C("zine_titles", L("Zine", [C("zine_new", 1, "The Zine", 2000, 1, PRINT), C("zine_new", 2, "Theme", 2000, 1, PRINT)])), ["theme", "zine"])]),
    ("hidden_block", [Eq(C("fine_block"), 500), Eq(C("student_can_borrow", charged("student", "a", 500)), False),
                      Eq(C("student_can_borrow", charged("student", "a", 499)), True)]),
    ("hidden_block_used", [Eq(F(C("family_borrow", charged("family", "a", 700, 2)), "loans"), 0),
                           Eq(F(C("guest_borrow", charged("guest", "a", 120)), "loans"), 1)]),
    ("hidden_fine_notice", [Eq(C("fine_notice", "Bo", 1205), "Dear Bo, you owe $12.05."), Eq(C("fine_notice", "Al", 7), "Dear Al, you owe $0.07.")]),
    ("hidden_reminders", [Eq(C("student_reminder", charged("student", "Al", 250)), "Dear Al, you owe $2.50."),
                          Eq(C("staff_reminder", charged("staff", "Bea", 1000)), "Dear Bea, you owe $10.00."),
                          Eq(C("faculty_reminder", charged("faculty", "Cy", 5)), "Dear Cy, you owe $0.05."),
                          Eq(C("alumni_reminder", C("alumni_new", 1, "Di", 1)), "Dear Di, you owe $0.00.")]),
    ("hidden_thesis_advisor", [Eq(F(C("thesis_new", 1, "On Rivers", 2010, 1, PRINT, "Dr. Lee"), "advisor"), "Dr. Lee"),
                               Eq(C("thesis_line", C("thesis_new", 7, "Atlas", 1999, 3, AUDIO, "Kim")), "Atlas (1999) [THE-7] 3/3 audio / Kim")]),
    ("hidden_thesis_plain", [Eq(C("thesis_line", C("thesis_new", 7, "Atlas", 1999, 3, AUDIO, "")), "Atlas (1999) [THE-7] 3/3 audio"),
                             Eq(P(C("thesis_sample"), "Thesis", "advisor", "Str"), ["", "", ""]),
                             Eq(C("thesis_available", C("thesis_checkout", C("thesis_new", 1, "a", 2000, 2, PRINT, "X"))), 1)]),
    ("hidden_staff_can_take", [Eq(C("staff_can_take", staff(1, "a", 1), B), True),
                               Eq(C("staff_can_take", staff(1, "a", 1), C("book_checkout", B)), False),
                               Eq(C("staff_can_take", C("staff_suspend", staff(1, "a", 1)), B), False),
                               Eq(C("staff_can_take", charged("staff", "a", 600), B), False)]),
    ("hidden_unchanged", [Eq(C("book_titles", C("book_sample")), sorted(g.title_key(r[1]) for r in g.item_params(0, "book")[3])),
                          Eq(C("fine_for", PRINT, 10), 250), Eq(C("max_loans", 3), 10)]),
]

THESIS_NEW = re.compile(r'(thesis_new|thesisNew)\((\d+, "[^"]*", \d+, \d+, (?:Format\.)?"?\w+"?)\)')


def patches():
    sp = [
        ("start.ssp", "fn title_key(t: Str) -> Str\n= t.trim.lower",
         "fn title_key(t: Str) -> Str\n= do\n  k = t.trim.lower\n  if k.starts_with(\"the \") then k.drop(4) else k"),
        ("start.ssp", "fn fine_block() -> Int\n= 1000", "fn fine_block() -> Int\n= 500"),
        ("start.ssp", "fn notice(name: Str, amount: Int) -> Str", "fn fine_notice(name: Str, amount: Int) -> Str"),
        ("start.ssp", "= notice(x.name, x.fines)", "= fine_notice(x.name, x.fines)", 4),
        ("start.ssp", "type Thesis = {id: Int, title: Str, year: Int, copies: Int, out: Int, format: Format, section: Str}",
         "type Thesis = {id: Int, title: Str, year: Int, copies: Int, out: Int, format: Format, section: Str, advisor: Str}"),
        ("start.ssp", 'fn thesis_new(id: Int, title: Str, year: Int, copies: Int, format: Format) -> Thesis\n= Thesis{id, title, year, copies, out: 0, format, section: "the"}',
         'fn thesis_new(id: Int, title: Str, year: Int, copies: Int, format: Format, advisor: Str) -> Thesis\n= Thesis{id, title, year, copies, out: 0, format, section: "the", advisor}'),
        ("start.ssp", THESIS_NEW, r'\1(\2, "")', 6),
        ("start.ssp", 'fn thesis_line(x: Thesis) -> Str\n= "{thesis_label(x)} [{thesis_shelf(x)}] {thesis_available(x)}/{x.copies} {format_name(x.format)}"',
         'fn thesis_line(x: Thesis) -> Str\n= "{thesis_label(x)} [{thesis_shelf(x)}] {thesis_available(x)}/{x.copies} {format_name(x.format)}" + (if x.advisor.is_empty then "" else " / {x.advisor}")'),
        ("start.ssp", None, "fn staff_can_take(m: Staff, b: Book) -> Bool\n= staff_can_borrow(m) and book_can_lend(b)\n\n"
         "test s4_title_key = title_key(\"The Hobbit\") == \"hobbit\" and title_key(\"Theory\") == \"theory\"\n\n"
         "test s4_block = not student_can_borrow(student_charge(student_new(1, \"a\", 1), 500))\n\n"
         "test s4_fine_notice = fine_notice(\"Bo\", 5) == \"Dear Bo, you owe $0.05.\"\n\n"
         "test s4_advisor = thesis_line(thesis_new(7, \"Atlas\", 1999, 3, Audio, \"Kim\")) == \"Atlas (1999) [THE-7] 3/3 audio / Kim\"\n\n"
         "test s4_staff_can_take = staff_can_take(staff_new(1, \"a\", 1), book_new(1, \"b\", 2000, 1, Print))"),
    ]
    py = [
        ("library/common.py", "def title_key(t: str) -> str:\n    return t.strip().lower()",
         "def title_key(t: str) -> str:\n    k = t.strip().lower()\n    return k[4:] if k.startswith(\"the \") else k"),
        ("library/common.py", "def fine_block() -> int:\n    return 1000", "def fine_block() -> int:\n    return 500"),
        ("library/common.py", "def notice(name: str, amount: int) -> str:", "def fine_notice(name: str, amount: int) -> str:"),
        ("library/thesis.py", "    section: str\n", "    section: str\n    advisor: str\n"),
        ("library/thesis.py", 'def thesis_new(id: int, title: str, year: int, copies: int, format: Format) -> Thesis:\n    return Thesis(id, title, year, copies, 0, format, "the")',
         'def thesis_new(id: int, title: str, year: int, copies: int, format: Format, advisor: str) -> Thesis:\n    return Thesis(id, title, year, copies, 0, format, "the", advisor)'),
        ("library/thesis.py", THESIS_NEW, r'\1(\2, "")', 3),
        ("tests/test_thesis.py", THESIS_NEW, r'\1(\2, "")', 3),
        ("library/thesis.py", 'return f"{thesis_label(x)} [{thesis_shelf(x)}] {thesis_available(x)}/{x.copies} {format_name(x.format)}"',
         'line = f"{thesis_label(x)} [{thesis_shelf(x)}] {thesis_available(x)}/{x.copies} {format_name(x.format)}"\n    return line + f" / {x.advisor}" if x.advisor else line'),
        ("library/staff.py", "from library.common import", "from library.book import Book, book_can_lend\nfrom library.common import"),
        ("library/staff.py", None, "\ndef staff_can_take(m: Staff, b: Book) -> bool:\n    return staff_can_borrow(m) and book_can_lend(b)"),
    ]
    ts = [
        ("library/common.ts", "return t.trim().toLowerCase();", 'const k = t.trim().toLowerCase();\n  return k.startsWith("the ") ? k.slice(4) : k;'),
        ("library/common.ts", "export function fineBlock(): number {\n  return 1000;", "export function fineBlock(): number {\n  return 500;"),
        ("library/common.ts", "export function notice(", "export function fineNotice("),
        ("library/thesis.ts", "  readonly section: string;\n", "  readonly section: string;\n  readonly advisor: string;\n"),
        ("library/thesis.ts", 'export function thesisNew(id: number, title: string, year: number, copies: number, format: Format): Thesis {\n  return { id, title, year, copies, out: 0, format, section: "the" };',
         'export function thesisNew(id: number, title: string, year: number, copies: number, format: Format, advisor: string): Thesis {\n  return { id, title, year, copies, out: 0, format, section: "the", advisor };'),
        ("library/thesis.ts", THESIS_NEW, r'\1(\2, "")', 3),
        ("tests/thesis.test.ts", THESIS_NEW, r'\1(\2, "")', 3),
        ("library/thesis.ts", "return `${thesisLabel(x)} [${thesisShelf(x)}] ${thesisAvailable(x)}/${x.copies} ${formatName(x.format)}`;",
         "const line = `${thesisLabel(x)} [${thesisShelf(x)}] ${thesisAvailable(x)}/${x.copies} ${formatName(x.format)}`;\n  return x.advisor === \"\" ? line : `${line} / ${x.advisor}`;"),
        ("library/staff.ts", "import {", 'import { Book, bookCanLend } from "./book";\nimport {'),
        ("library/staff.ts", None, "export function staffCanTake(m: Staff, b: Book): boolean {\n  return staffCanBorrow(m) && bookCanLend(b);\n}"),
    ]
    go = [
        ("library/common.go", "return strings.ToLower(strings.TrimSpace(t))", 'return strings.TrimPrefix(strings.ToLower(strings.TrimSpace(t)), "the ")'),
        ("library/common.go", "func fineBlock() int {\n\treturn 1000", "func fineBlock() int {\n\treturn 500"),
        ("library/common.go", "func notice(", "func fineNotice("),
        ("library/thesis.go", "\tsection string\n}", "\tsection string\n\tadvisor string\n}"),
        ("library/thesis.go", 'func thesisNew(id int, title string, year int, copies int, format Format) Thesis {\n\treturn Thesis{id, title, year, copies, 0, format, "the"}',
         'func thesisNew(id int, title string, year int, copies int, format Format, advisor string) Thesis {\n\treturn Thesis{id, title, year, copies, 0, format, "the", advisor}'),
        ("library/thesis.go", THESIS_NEW, r'\1(\2, "")', 3),
        ("library/thesis_test.go", THESIS_NEW, r'\1(\2, "")', 3),
        ("library/thesis.go", '\treturn fmt.Sprintf("%s [%s] %d/%d %s", thesisLabel(x), thesisShelf(x), thesisAvailable(x), x.copies, formatName(x.format))',
         '\tline := fmt.Sprintf("%s [%s] %d/%d %s", thesisLabel(x), thesisShelf(x), thesisAvailable(x), x.copies, formatName(x.format))\n'
         '\tif x.advisor != "" {\n\t\tline += " / " + x.advisor\n\t}\n\treturn line'),
        ("library/staff.go", None, "func staffCanTake(m Staff, b Book) bool {\n\treturn staffCanBorrow(m) && bookCanLend(b)\n}"),
    ]
    for d in g.REMINDED:
        c = camel(d)
        py += [(f"library/{d}.py", "fine_block, join_lines, max_loans, notice", "fine_block, fine_notice, join_lines, max_loans"),
               (f"library/{d}.py", "return notice(x.name, x.fines)", "return fine_notice(x.name, x.fines)")]
        ts += [(f"library/{d}.ts", "fineBlock, joinLines, maxLoans, notice", "fineBlock, fineNotice, joinLines, maxLoans"),
               (f"library/{d}.ts", "return notice(x.name, x.fines);", "return fineNotice(x.name, x.fines);")]
        go += [(f"library/{d}.go", "return notice(x.name, x.fines)", "return fineNotice(x.name, x.fines)")]
    return {"sspur": sp, "python": py, "ts": ts, "go": go}
