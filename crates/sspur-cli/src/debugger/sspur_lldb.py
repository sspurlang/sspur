"""lldb formatters for SSPUR debug builds (sspur run --debug).

Load with:  command script import /path/to/sspur_lldb.py
Shows Str as text, Bool as true/false, List elements, Opt as some/none, and sum values by
variant name with their fields. `sspur-locals` lists only the SSPUR variables of the frame.
"""

import lldb

MAX_STR = 4096
MAX_ITEMS = 1000
# A length past this is a variable that isn't set yet.
MAX_LEN = 1 << 40


def _ptr(v):
    return v.GetValueAsUnsigned(0)


def _quote(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t") + '"'


def _short(v):
    s = v.GetSummary()
    if s:
        return s
    s = v.GetValue()
    if s is not None:
        return s
    return "{...}"


def str_summary(valobj, internal_dict):
    v = valobj.GetNonSyntheticValue()
    n = v.GetChildMemberWithName("len").GetValueAsSigned(0)
    if n == 0:
        return '""'
    if n < 0 or n > MAX_LEN:
        return "<not set>"
    k = min(n, MAX_STR)
    err = lldb.SBError()
    data = valobj.GetProcess().ReadMemory(_ptr(v.GetChildMemberWithName("p")), k, err)
    if not err.Success():
        return "<unreadable Str>"
    text = _quote(data.decode("utf-8", "replace"))
    return text if k == n else text[:-1] + '..." (%d bytes)' % n


def bool_summary(valobj, internal_dict):
    x = valobj.GetValueAsSigned(0)
    return "true" if x == 1 else "false" if x == 0 else str(x)


class ListProvider:
    def __init__(self, valobj, internal_dict):
        self.v = valobj

    def update(self):
        v = self.v.GetNonSyntheticValue()
        n = v.GetChildMemberWithName("len").GetValueAsSigned(0)
        self.n = n if 0 <= n <= MAX_LEN else 0
        self.data = v.GetChildMemberWithName("data")
        self.et = self.data.GetType().GetPointeeType()
        self.size = self.et.GetByteSize()
        return False

    def num_children(self):
        return min(self.n, MAX_ITEMS)

    def get_child_index(self, name):
        try:
            return int(name.strip("[]"))
        except ValueError:
            return -1

    def get_child_at_index(self, i):
        if i < 0 or i >= self.num_children():
            return None
        return self.data.CreateChildAtOffset("[%d]" % i, i * self.size, self.et)

    def has_children(self):
        return True


def list_summary(valobj, internal_dict):
    v = valobj.GetNonSyntheticValue()
    n = v.GetChildMemberWithName("len").GetValueAsSigned(0)
    if n < 0 or n > MAX_LEN:
        return "<not set>"
    p = ListProvider(valobj, internal_dict)
    p.update()
    shown = [_short(p.get_child_at_index(i)) for i in range(min(n, 8))]
    return "[" + ", ".join(shown) + (", ..." if n > 8 else "") + "] len=%d" % n


class OptProvider:
    def __init__(self, valobj, internal_dict):
        self.v = valobj

    def update(self):
        v = self.v.GetNonSyntheticValue()
        self.some = v.GetChildMemberWithName("some").GetValueAsSigned(0) != 0
        self.val = v.GetChildMemberWithName("v")
        return False

    def num_children(self):
        return 1 if self.some else 0

    def get_child_index(self, name):
        return 0 if name == "some" else -1

    def get_child_at_index(self, i):
        if i == 0 and self.some:
            return self.val.CreateValueFromData("some", self.val.GetData(), self.val.GetType())
        return None

    def has_children(self):
        return self.some


def opt_summary(valobj, internal_dict):
    v = valobj.GetNonSyntheticValue()
    if v.GetChildMemberWithName("some").GetValueAsSigned(0) == 0:
        return "none"
    return "some(" + _short(v.GetChildMemberWithName("v")) + ")"


def _variants(target, struct_name):
    """Variant names by tag, from the enum a debug build emits for each sum type."""
    for name in ("SST_" + struct_name, "enum SST_" + struct_name):
        t = target.FindFirstType(name)
        if t.IsValid():
            members = t.GetEnumMembers()
            out = {}
            for i in range(members.GetSize()):
                m = members.GetTypeEnumMemberAtIndex(i)
                out[m.GetValueAsSigned()] = m.GetName().split("__", 1)[-1]
            return out
    return {}


def _sum_parts(valobj):
    """(variant name, the struct of its fields or None) for a pointer to a sum value."""
    v = valobj.GetNonSyntheticValue()
    t = v.GetType()
    if t.IsPointerType():
        st = t.GetPointeeType().GetCanonicalType()
    else:
        st = t.GetCanonicalType()
    sname = st.GetName().replace("struct ", "")
    names = _variants(valobj.GetTarget(), sname)
    union = None
    has_tag = False
    for i in range(st.GetNumberOfFields()):
        f = st.GetFieldAtIndex(i)
        if f.GetName() == "tag":
            has_tag = True
        if f.GetName() == "u":
            union = f.GetType()
    present = []
    if union is not None:
        for i in range(union.GetNumberOfFields()):
            n = union.GetFieldAtIndex(i).GetName()
            if n.startswith("v") and n[1:].isdigit():
                present.append(int(n[1:]))
    if t.IsPointerType() and _ptr(v) == 0:
        if has_tag:
            return ("null", None)
        empty = [k for k in names if k not in present]
        return (names.get(empty[0], "?") if empty else "?", None)
    obj = (v.Dereference() if t.IsPointerType() else v).GetNonSyntheticValue()
    if has_tag:
        k = obj.GetChildMemberWithName("tag").GetValueAsSigned(0)
    else:
        k = present[0] if present else 0
    name = names.get(k, "#%d" % k)
    if k in present:
        return (name, obj.GetChildMemberWithName("u").GetNonSyntheticValue().GetChildMemberWithName("v%d" % k).GetNonSyntheticValue())
    return (name, None)


class SumProvider:
    def __init__(self, valobj, internal_dict):
        self.v = valobj
        self.fields = None

    def update(self):
        try:
            self.fields = _sum_parts(self.v)[1]
        except Exception:
            self.fields = None
        return False

    def num_children(self):
        return self.fields.GetNumChildren() if self.fields is not None else 0

    def get_child_index(self, name):
        if self.fields is None:
            return -1
        for i in range(self.fields.GetNumChildren()):
            if self.fields.GetChildAtIndex(i).GetName() == name:
                return i
        return -1

    def get_child_at_index(self, i):
        return self.fields.GetChildAtIndex(i) if self.fields is not None else None

    def has_children(self):
        return self.num_children() > 0


def sum_summary(valobj, internal_dict):
    try:
        name, fields = _sum_parts(valobj)
    except Exception as e:
        return "<sum: %s>" % e
    if fields is None:
        return name
    parts = []
    for i in range(fields.GetNumChildren()):
        c = fields.GetChildAtIndex(i)
        parts.append("%s: %s" % (c.GetName(), _short(c)))
    return name + "{" + ", ".join(parts) + "}"


def _sspur_name(n):
    return n and "$" not in n and not n.endswith("_") and n not in ("st", "depth", "env") and not (n[0] == "a" and n[1:].isdigit())


def locals_cmd(debugger, command, result, internal_dict):
    """sspur-locals: the SSPUR parameters and locals of the selected frame."""
    frame = debugger.GetSelectedTarget().GetProcess().GetSelectedThread().GetSelectedFrame()
    if not frame.IsValid():
        result.SetError("no frame")
        return
    for v in frame.GetVariables(True, True, False, True):
        if _sspur_name(v.GetName()):
            result.AppendMessage(str(v).strip())


def __lldb_init_module(debugger, internal_dict):
    m = __name__
    run = debugger.HandleCommand
    run("type summary add -w sspur -F %s.str_summary Str" % m)
    run("type summary add -w sspur -v -F %s.bool_summary ss_bool" % m)
    run("type synthetic add -w sspur -x '^(struct )?L_[A-Za-z0-9_]+$' -l %s.ListProvider" % m)
    run("type summary add -w sspur -e -x '^(struct )?L_[A-Za-z0-9_]+$' -F %s.list_summary" % m)
    run("type synthetic add -w sspur -x '^(struct )?O_[A-Za-z0-9_]+$' -l %s.OptProvider" % m)
    run("type summary add -w sspur -e -x '^(struct )?O_[A-Za-z0-9_]+$' -F %s.opt_summary" % m)
    run("type synthetic add -w sspur -x '^(struct )?S_[A-Za-z0-9_]+$' -l %s.SumProvider" % m)
    run("type summary add -w sspur -v -e -x '^(struct )?S_[A-Za-z0-9_]+$' -F %s.sum_summary" % m)
    run("type category enable sspur")
    run("command script add -o -f %s.locals_cmd sspur-locals" % m)
