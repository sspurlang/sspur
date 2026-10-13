"""Sum token usage from Claude Code subagent transcripts (JSONL).

  python3 tokens.py <agents.json> <out.json>

agents.json maps "<lang>/<task>" to a transcript path (small tasks and the large-codebase tasks s1 to s6
alike). It prints one row per cell, a TOTAL per language and, when the run has large-codebase cells
other than s1_shop, a total per language over the large cells.

Input side (exact, from the API usage records): for every API call (one per requestId),
input_tokens + cache_creation_input_tokens + cache_read_input_tokens.

Output side (estimated): the transcript stores the usage snapshot taken when the response
stream starts, so its output_tokens is a placeholder (16 to 75). We therefore re-count the
output from the recorded content (text, thinking, tool name and JSON input) with tiktoken
cl100k_base, and take max(recorded, estimate) per call. Claude's tokenizer differs from
cl100k, so treat output counts as +-15%. Output is about 1% of the totals.

  input      exact input tokens summed over calls (includes cache reads)
  output     estimated output tokens
  total      input + output
  net        total - calls * BASE   (BASE = context of a no-op subagent; removes the fixed
             system prompt and tool schema that every call re-reads)
  read, wrote  cl100k tokens of all tool results the agent read, and of all tool inputs and
             text it wrote. Independent of the harness, and the cleanest language comparison.
"""
import json, os, re, sys
import tiktoken

BASE = int(os.environ.get("BASE", 25_750))  # no-op subagent context (system prompt + tools + one-line prompt), Sonnet, measured 2026-10-03; set BASE for other models
ENC = tiktoken.get_encoding("cl100k_base")
LARGE = re.compile(r"s\d+_")  # large-codebase task ids: s1_shop, s2_shop, s3_library, ...


def ntok(s):
    return len(ENC.encode(s, disallowed_special=()))


def block_text(b):
    t = b.get("type")
    if t == "text":
        return b.get("text", "")
    if t == "thinking":
        return b.get("thinking", "")
    if t == "tool_use":
        return b.get("name", "") + json.dumps(b.get("input", {}))
    return ""


def result_text(b):
    c = b.get("content")
    if isinstance(c, list):
        return "".join(x.get("text", "") for x in c if isinstance(x, dict))
    return c if isinstance(c, str) else ""


def usage(path):
    calls, out_est = {}, {}
    tools = read = wrote = 0
    for line in open(path):
        d = json.loads(line)
        m = d.get("message")
        if not isinstance(m, dict):
            continue
        content = m.get("content") if isinstance(m.get("content"), list) else []
        if d.get("type") == "assistant":
            rid = d.get("requestId") or d["uuid"]
            calls[rid] = m["usage"]
            n = sum(ntok(block_text(b)) for b in content)
            out_est[rid] = out_est.get(rid, 0) + n
            wrote += sum(ntok(block_text(b)) for b in content if b.get("type") != "thinking")
            tools += sum(1 for b in content if b.get("type") == "tool_use")
        elif d.get("type") == "user":
            read += sum(ntok(result_text(b)) for b in content if b.get("type") == "tool_result")
    inp = sum(u["input_tokens"] + u.get("cache_creation_input_tokens", 0) + u.get("cache_read_input_tokens", 0) for u in calls.values())
    out = sum(max(u["output_tokens"], out_est.get(r, 0)) for r, u in calls.items())
    return {"calls": len(calls), "tool_uses": tools, "input": inp, "output": out, "total": inp + out,
            "net": inp + out - len(calls) * BASE, "read": read, "wrote": wrote}


if __name__ == "__main__":
    agents = json.load(open(sys.argv[1]))
    out = {k: usage(p) for k, p in agents.items()}
    json.dump(out, open(sys.argv[2], "w"), indent=1)
    keys = ["calls", "tool_uses", "input", "output", "total", "net", "read", "wrote"]
    print(f"{'run':22}" + "".join(f"{k:>10}" for k in keys))
    for k, v in out.items():
        print(f"{k:22}" + "".join(f"{v[x]:>10}" for x in keys))
    for lang in ("sspur", "python", "ts", "go"):
        rs = [v for k, v in out.items() if k.startswith(lang + "/")]
        if rs:
            print(f"{lang + ' TOTAL':22}" + "".join(f"{sum(r[x] for r in rs):>10}" for x in keys))
    # the large-codebase cells (s1_shop to s6_fleet), when the run has any besides s1_shop
    if any(LARGE.match(k.split("/", 1)[1]) and not k.endswith("/s1_shop") for k in out):
        for lang in ("sspur", "python", "ts", "go"):
            rs = [v for k, v in out.items() if k.startswith(lang + "/") and LARGE.match(k.split("/", 1)[1])]
            if rs:
                print(f"{lang + f' large ({len(rs)})':22}" + "".join(f"{sum(r[x] for r in rs):>10}" for x in keys))
