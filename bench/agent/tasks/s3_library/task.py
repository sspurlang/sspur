"""s3_library: four targeted changes on the library codebase."""
import os, re, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "large"))
import gen_library as g  # noqa: E402
from lib import C, Eq, L, V, camel, cap  # noqa: E402

CODEBASE = "library"
TITLE = "Library: four targeted changes in a large codebase"
EXAMPLE = "fine_cap is fineCap"
STEPS = [
    "The fine cap goes up from $5.00 to $8.00.",
    "Renewals are limited: renewal gets a new last parameter times (an Int, how often the loan was already renewed). From the third renewal on (times >= 2) the due day no longer moves. Every function that calls renewal gets the same new last parameter times and passes it on.",
    "Add the format Microfilm: 7 loan days, a daily fine of 50 cents, the name \"microfilm\", and listed last by formats().",
    "Add journal_overdue(xs, start, today): the titles of the journals that are out (out > 0) and whose journal_fine for start and today is above zero, keyed and sorted the way journal_titles does it.",
    "Add a test for each change. All existing tests must still pass.",
]
IFACE = {
    "sspur": "fn renewal(due: Int, f: Format, times: Int) -> Int; fn book_renewed_due(x: Book, start: Int, times: Int) -> Int and the same new parameter on every other caller of renewal; Format gets the variant Microfilm; fn journal_overdue(xs: List[Journal], start: Int, today: Int) -> List[Str]; fine_cap, formats and the other existing functions keep their signatures",
    "python": "library.common.renewal(due, f, times) -> int; library.book.book_renewed_due(x, start, times) -> int and the same new parameter on every other caller of renewal; Format.MICROFILM; library.journal.journal_overdue(xs, start, today) -> List[str]; fine_cap, formats and the other existing functions keep their modules and signatures",
    "ts": "library/common.ts: export function renewal(due: number, f: Format, times: number): number; Format gets \"microfilm\"; library/book.ts: export function bookRenewedDue(x: Book, start: number, times: number): number and the same new parameter on every other caller of renewal; library/journal.ts: export function journalOverdue(xs: readonly Journal[], start: number, today: number): string[]; fineCap, formats and the other existing functions keep their modules and signatures",
    "go": "func renewal(due int, f Format, times int) int; func bookRenewedDue(x Book, start, times int) int and the same new parameter on every other caller of renewal; a Format constant Microfilm after Audio; func journalOverdue(xs []Journal, start, today int) []string, all in package library; fineCap, formats and the other existing functions keep their signatures",
}

PRINT, EBOOK, AUDIO, MICRO = (V("Format", n) for n in ("Print", "Ebook", "Audio", "Microfilm"))


def item(d, *a):
    return C(d + "_new", *a)


JOURNALS = L("Journal", [C("journal_checkout", item("journal", 1, "Quiet River", 2001, 2, PRINT)), item("journal", 2, "Alpha", 2002, 1, PRINT),
                         C("journal_checkout", item("journal", 3, " Bright Atlas", 2003, 1, EBOOK)),
                         C("journal_checkout", item("journal", 4, "Atlas Engine", 1990, 1, AUDIO))])
BOOK_TITLES = sorted(g.title_key(r[1]) for r in g.item_params(0, "book")[3])

HIDDEN = [
    ("hidden_cap", [Eq(C("fine_cap"), 800), Eq(C("fine_for", PRINT, 100), 800), Eq(C("fine_for", AUDIO, 30), 300)]),
    ("hidden_cap_used", [Eq(C("book_fine", item("book", 1, "a", 2000, 1, PRINT), 0, 60), 800),
                         Eq(C("dvd_fine", item("dvd", 1, "d", 2000, 1, AUDIO), 0, 74), 600)]),
    ("hidden_renewal", [Eq(C("renewal", 21, PRINT, 0), 31), Eq(C("renewal", 21, PRINT, 1), 31), Eq(C("renewal", 21, PRINT, 2), 21),
                        Eq(C("renewal", 14, EBOOK, 5), 14), Eq(C("renewal", 7, AUDIO, 0), 14)]),
    ("hidden_renewed_callers", [Eq(C("book_renewed_due", item("book", 1, "a", 2000, 1, PRINT), 0, 0), 31),
                                Eq(C("journal_renewed_due", item("journal", 1, "j", 2000, 1, EBOOK), 10, 1), 31),
                                Eq(C("score_renewed_due", item("score", 1, "s", 2000, 1, PRINT), 0, 2), 21)]),
    ("hidden_renewed_rest", [Eq(C("comic_renewed_due", item("comic", 1, "c", 2000, 1, AUDIO), 0, 3), 14),
                             Eq(C("atlas_renewed_due", item("atlas", 1, "a", 2000, 1, PRINT), 5, 1), 36)]),
    ("hidden_microfilm", [Eq(C("loan_days", MICRO), 7), Eq(C("daily_fine", MICRO), 50), Eq(C("format_name", MICRO), "microfilm")]),
    ("hidden_formats", [Eq(C("formats"), L("Format", [PRINT, EBOOK, AUDIO, MICRO]))]),
    ("hidden_microfilm_used", [Eq(C("book_line", item("book", 7, "Atlas", 1999, 3, MICRO)), "Atlas (1999) [BOO-7] 3/3 microfilm"),
                               Eq(C("book_fine", item("book", 1, "a", 2000, 1, MICRO), 0, 10), 150),
                               Eq(C("book_due", item("book", 1, "a", 2000, 1, MICRO), 3), 10)]),
    ("hidden_overdue", [Eq(C("journal_overdue", JOURNALS, 0, 30), ["atlas engine", "quiet river"])]),
    ("hidden_overdue_edges", [Eq(C("journal_overdue", L("Journal", [C("journal_checkout", item("journal", 1, "a", 2000, 1, PRINT))]), 0, 21), L("Str", [])),
                              Eq(C("journal_overdue", L("Journal", []), 0, 99), L("Str", []))]),
    ("hidden_unchanged", [Eq(C("book_titles", C("book_sample")), BOOK_TITLES), Eq(C("fine_for", PRINT, 10), 250),
                          Eq(C("student_line", C("student_charge", C("student_new", 3, "Ann", 1), 250)), "Ann (tier 1): 0/3 loans, owes $2.50")]),
]


def patches():
    sp = [
        ("start.ssp", "fn fine_cap() -> Int\n= 500", "fn fine_cap() -> Int\n= 800"),
        ("start.ssp", "fn renewal(due: Int, f: Format) -> Int\n= due + loan_days(f) / 2",
         "fn renewal(due: Int, f: Format, times: Int) -> Int\n= if times >= 2 then due else due + loan_days(f) / 2"),
        ("start.ssp", re.compile(r"fn (\w+)_renewed_due\(x: (\w+), start: Int\) -> Int\n= renewal\((\w+)_due\(x, start\), x\.format\)"),
         r"fn \1_renewed_due(x: \2, start: Int, times: Int) -> Int\n= renewal(\3_due(x, start), x.format, times)", 5),
        ("start.ssp", re.compile(r'(\w+)_renewed_due\((\w+)_new\(1, "a", 2000, 1, Print\), 0\)'), r'\1_renewed_due(\2_new(1, "a", 2000, 1, Print), 0, 0)', 5),
        ("start.ssp", "type Format = Print | Ebook | Audio", "type Format = Print | Ebook | Audio | Microfilm"),
        ("start.ssp", '  | Audio => "audio"\n', '  | Audio => "audio"\n  | Microfilm => "microfilm"\n'),
        ("start.ssp", "  | Audio => 14\n", "  | Audio => 14\n  | Microfilm => 7\n"),
        ("start.ssp", "  | Audio => 10\n", "  | Audio => 10\n  | Microfilm => 50\n"),
        ("start.ssp", "= [Print, Ebook, Audio]", "= [Print, Ebook, Audio, Microfilm]"),
        ("start.ssp", None, "fn journal_overdue(xs: List[Journal], start: Int, today: Int) -> List[Str]\n"
         "= journal_titles(xs.filter(x => x.out > 0 and journal_fine(x, start, today) > 0))\n\n"
         "test s3_cap = fine_for(Print, 100) == 800\n\n"
         "test s3_renewal_limit = renewal(21, Print, 2) == 21 and book_renewed_due(book_new(1, \"a\", 2000, 1, Print), 0, 1) == 31\n\n"
         "test s3_microfilm = loan_days(Microfilm) == 7 and daily_fine(Microfilm) == 50 and format_name(Microfilm) == \"microfilm\"\n\n"
         "test s3_overdue = journal_overdue([journal_checkout(journal_new(1, \"B\", 2000, 1, Print)), journal_new(2, \"A\", 2000, 1, Print)], 0, 30) == [\"b\"]"),
    ]
    py = [
        ("library/common.py", "def fine_cap() -> int:\n    return 500", "def fine_cap() -> int:\n    return 800"),
        ("library/common.py", "def renewal(due: int, f: Format) -> int:\n    return due + loan_days(f) // 2",
         "def renewal(due: int, f: Format, times: int) -> int:\n    if times >= 2:\n        return due\n    return due + loan_days(f) // 2"),
        ("library/common.py", '    AUDIO = "audio"\n', '    AUDIO = "audio"\n    MICROFILM = "microfilm"\n'),
        ("library/common.py", "Format.AUDIO: 14}", "Format.AUDIO: 14, Format.MICROFILM: 7}"),
        ("library/common.py", "Format.AUDIO: 10}", "Format.AUDIO: 10, Format.MICROFILM: 50}"),
        ("library/journal.py", None, "\ndef journal_overdue(xs: List[Journal], start: int, today: int) -> List[str]:\n"
         "    return journal_titles([x for x in xs if x.out > 0 and journal_fine(x, start, today) > 0])"),
    ]
    ts = [
        ("library/common.ts", "export function fineCap(): number {\n  return 500;", "export function fineCap(): number {\n  return 800;"),
        ("library/common.ts", "export function renewal(due: number, f: Format): number {\n  return due + Math.floor(loanDays(f) / 2);",
         "export function renewal(due: number, f: Format, times: number): number {\n  if (times >= 2) return due;\n  return due + Math.floor(loanDays(f) / 2);"),
        ("library/common.ts", 'export type Format = "print" | "ebook" | "audio";', 'export type Format = "print" | "ebook" | "audio" | "microfilm";'),
        ("library/common.ts", '    case "audio":\n      return 14;\n', '    case "audio":\n      return 14;\n    case "microfilm":\n      return 7;\n'),
        ("library/common.ts", '    case "audio":\n      return 10;\n', '    case "audio":\n      return 10;\n    case "microfilm":\n      return 50;\n'),
        ("library/common.ts", 'return ["print", "ebook", "audio"];', 'return ["print", "ebook", "audio", "microfilm"];'),
        ("library/journal.ts", None, "export function journalOverdue(xs: readonly Journal[], start: number, today: number): string[] {\n"
         "  return journalTitles(xs.filter((x) => x.out > 0 && journalFine(x, start, today) > 0));\n}"),
    ]
    go = [
        ("library/common.go", "func fineCap() int {\n\treturn 500", "func fineCap() int {\n\treturn 800"),
        ("library/common.go", "func renewal(due int, f Format) int {\n\treturn due + loanDays(f)/2",
         "func renewal(due int, f Format, times int) int {\n\tif times >= 2 {\n\t\treturn due\n\t}\n\treturn due + loanDays(f)/2"),
        ("library/common.go", "\tAudio\n)", "\tAudio\n\tMicrofilm\n)"),
        ("library/common.go", '\tcase Audio:\n\t\treturn "audio"\n', '\tcase Audio:\n\t\treturn "audio"\n\tcase Microfilm:\n\t\treturn "microfilm"\n'),
        ("library/common.go", "\tcase Audio:\n\t\treturn 14\n", "\tcase Audio:\n\t\treturn 14\n\tcase Microfilm:\n\t\treturn 7\n"),
        ("library/common.go", "\tcase Audio:\n\t\treturn 10\n", "\tcase Audio:\n\t\treturn 10\n\tcase Microfilm:\n\t\treturn 50\n"),
        ("library/common.go", "[]Format{Print, Ebook, Audio}", "[]Format{Print, Ebook, Audio, Microfilm}"),
        ("library/journal.go", None, "func journalOverdue(xs []Journal, start, today int) []string {\n\tdue := []Journal{}\n\tfor _, x := range xs {\n"
         "\t\tif x.out > 0 && journalFine(x, start, today) > 0 {\n\t\t\tdue = append(due, x)\n\t\t}\n\t}\n\treturn journalTitles(due)\n}"),
    ]
    for d in g.RENEWABLE:
        T, c = cap(d), camel(d)
        py += [(f"library/{d}.py", f"def {d}_renewed_due(x: {T}, start: int) -> int:\n    return renewal({d}_due(x, start), x.format)",
                f"def {d}_renewed_due(x: {T}, start: int, times: int) -> int:\n    return renewal({d}_due(x, start), x.format, times)"),
               (f"tests/test_{d}.py", f'{d}_renewed_due({d}_new(1, "a", 2000, 1, Format.PRINT), 0)', f'{d}_renewed_due({d}_new(1, "a", 2000, 1, Format.PRINT), 0, 0)')]
        ts += [(f"library/{d}.ts", f"export function {c}RenewedDue(x: {T}, start: number): number {{\n  return renewal({c}Due(x, start), x.format);",
                f"export function {c}RenewedDue(x: {T}, start: number, times: number): number {{\n  return renewal({c}Due(x, start), x.format, times);"),
               (f"tests/{d}.test.ts", f'{c}RenewedDue({c}New(1, "a", 2000, 1, "print"), 0)', f'{c}RenewedDue({c}New(1, "a", 2000, 1, "print"), 0, 0)')]
        go += [(f"library/{d}.go", f"func {c}RenewedDue(x {T}, start int) int {{\n\treturn renewal({c}Due(x, start), x.format)",
                f"func {c}RenewedDue(x {T}, start, times int) int {{\n\treturn renewal({c}Due(x, start), x.format, times)"),
               (f"library/{d}_test.go", f'{c}RenewedDue({c}New(1, "a", 2000, 1, Print), 0)', f'{c}RenewedDue({c}New(1, "a", 2000, 1, Print), 0, 0)')]
    return {"sspur": sp, "python": py, "ts": ts, "go": go}
