"""gdb pretty-printers for SSPUR debug builds (sspur run --debug).

Load with:  source /path/to/sspur_gdb.py
Shows Str as text, Bool as true/false, List elements, Opt as some/none, and sum values by
variant name with their fields. `sspur-locals` lists only the SSPUR variables of the frame.
"""

import gdb

MAX_STR = 4096
MAX_ITEMS = 1000
# A length past this is a variable that isn't set yet.
MAX_LEN = 1 << 40


def _quote(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t") + '"'


def _names(t):
    """The type's name and the names of the typedefs under it."""
    out = []
    while True:
        if t.name:
            out.append(t.name)
        if t.tag:
            out.append(t.tag)
        if t.code != gdb.TYPE_CODE_TYPEDEF:
            return out
        t = t.target()


class StrPrinter:
    def __init__(self, v):
        self.v = v

    def to_string(self):
        n = int(self.v["len"])
        if n == 0:
            return '""'
        if n < 0 or n > MAX_LEN:
            return "<not set>"
        k = min(n, MAX_STR)
        try:
            data = bytes(gdb.selected_inferior().read_memory(int(self.v["p"]), k))
        except gdb.MemoryError:
            return "<unreadable Str>"
        text = _quote(data.decode("utf-8", "replace"))
        return text if k == n else text[:-1] + '..." (%d bytes)' % n


class BoolPrinter:
    def __init__(self, v):
        self.v = v

    def to_string(self):
        x = int(self.v)
        return "true" if x == 1 else "false" if x == 0 else str(x)


class ListPrinter:
    def __init__(self, v):
        self.v = v

    def _len(self):
        n = int(self.v["len"])
        return n if 0 <= n <= MAX_LEN else None

    def to_string(self):
        n = self._len()
        return "<not set>" if n is None else "len=%d" % n

    def children(self):
        data = self.v["data"]
        for i in range(min(self._len() or 0, MAX_ITEMS)):
            yield ("[%d]" % i, data[i])

    def display_hint(self):
        return "array"


class OptPrinter:
    def __init__(self, v):
        self.v = v

    def to_string(self):
        if not int(self.v["some"]):
            return "none"
        return "some(%s)" % self.v["v"].format_string()


_tags = {}


def _variants(struct_name):
    if struct_name not in _tags:
        out = {}
        try:
            t = gdb.lookup_type("enum SST_" + struct_name)
            for f in t.fields():
                out[int(f.enumval)] = f.name.split("__", 1)[-1]
        except gdb.error:
            pass
        _tags[struct_name] = out
    return _tags[struct_name]


class SumPrinter:
    def __init__(self, v):
        self.v = v
        st = v.type.strip_typedefs().target().strip_typedefs()
        self.names = _variants(st.tag or "")
        fields = [f.name for f in st.fields()]
        self.has_tag = "tag" in fields
        self.present = []
        if "u" in fields:
            for f in st["u"].type.strip_typedefs().fields():
                if f.name.startswith("v") and f.name[1:].isdigit():
                    self.present.append(int(f.name[1:]))

    def _parts(self):
        if int(self.v) == 0:
            if self.has_tag:
                return ("null", None)
            empty = [k for k in self.names if k not in self.present]
            return (self.names.get(empty[0], "?") if empty else "?", None)
        obj = self.v.dereference()
        k = int(obj["tag"]) if self.has_tag else (self.present[0] if self.present else 0)
        name = self.names.get(k, "#%d" % k)
        if k in self.present:
            return (name, obj["u"]["v%d" % k])
        return (name, None)

    def to_string(self):
        return self._parts()[0]

    def children(self):
        fields = self._parts()[1]
        if fields is not None:
            for f in fields.type.strip_typedefs().fields():
                yield (f.name, fields[f.name])


def lookup(val):
    t = val.type
    names = _names(t)
    if "ss_bool" in names:
        return BoolPrinter(val)
    if "Str" in names:
        return StrPrinter(val)
    st = t.strip_typedefs()
    if st.code == gdb.TYPE_CODE_PTR:
        tgt = st.target().strip_typedefs()
        if tgt.code == gdb.TYPE_CODE_STRUCT and (tgt.tag or "").startswith("S_"):
            return SumPrinter(val)
        return None
    if st.code != gdb.TYPE_CODE_STRUCT:
        return None
    tag = st.tag or (names[0] if names else "")
    if tag.startswith("L_"):
        return ListPrinter(val)
    if tag.startswith("O_"):
        return OptPrinter(val)
    return None


def _sspur_name(n):
    return n and "$" not in n and not n.endswith("_") and n not in ("st", "depth", "env") and not (n[0] == "a" and n[1:].isdigit())


class Locals(gdb.Command):
    """sspur-locals: the SSPUR parameters and locals of the selected frame."""

    def __init__(self):
        super().__init__("sspur-locals", gdb.COMMAND_DATA)

    def invoke(self, arg, from_tty):
        frame = gdb.selected_frame()
        block = frame.block()
        seen = set()
        while block is not None:
            for sym in block:
                if (sym.is_variable or sym.is_argument) and sym.name not in seen and _sspur_name(sym.name):
                    seen.add(sym.name)
                    gdb.write("%s = %s\n" % (sym.name, sym.value(frame).format_string()))
            if block.function is not None:
                break
            block = block.superblock


gdb.pretty_printers.append(lookup)
Locals()
