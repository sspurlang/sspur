#!/usr/bin/env python3
"""A stand-in for the gh CLI in the registry end-to-end test: the subset of commands that
`sspur publish`, `sspur yank` and the registry workflow scripts use, with GitHub kept as files
under $FAKE_GH_DIR. Pull requests merge into the bare repository $FAKE_GH_INDEX, and only when
the head commit has a successful `index-guard` status, as branch protection requires. It never
talks to the network."""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zlib

D = os.environ["FAKE_GH_DIR"]
os.makedirs(D, exist_ok=True)
STATE = os.path.join(D, "state.json")


def load():
    if os.path.exists(STATE):
        with open(STATE) as f:
            return json.load(f)
    return {"next": 1, "issues": {}, "prs": {}, "comments": {}, "statuses": [], "runs": []}


def save(s):
    tmp = STATE + ".tmp"
    with open(tmp, "w") as f:
        json.dump(s, f, indent=1)
    os.replace(tmp, STATE)


def opt(args, name, default=None):
    for i, a in enumerate(args):
        if a == name and i + 1 < len(args):
            return args[i + 1]
    return default


def opts(args, name):
    return [args[i + 1] for i, a in enumerate(args) if a == name and i + 1 < len(args)]


def body_of(args):
    p = opt(args, "--body-file")
    if p is None:
        return opt(args, "--body", "")
    if p == "-":
        return sys.stdin.read()
    with open(p) as f:
        return f.read()


def user_id(login):
    """A stable id for a login, as GitHub gives every account one."""
    for pair in os.environ.get("FAKE_GH_IDS", "").split(","):
        if pair.startswith(login + "="):
            return int(pair.split("=", 1)[1])
    return 1000 + zlib.crc32(login.encode()) % 9000000


def listed(var, org, user):
    """Whether `org:user` is in the comma separated list in the environment variable `var`."""
    return f"{org.lower()}:{user.lower()}" in [x.strip().lower() for x in os.environ.get(var, "").split(",")]


def git(*a, cwd=None):
    r = subprocess.run(["git", *a], cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(r.stderr)
        sys.exit(1)
    return r.stdout.strip()


def main(argv):
    s = load()
    repo = opt(argv, "--repo", os.environ.get("GITHUB_REPOSITORY", ""))
    with open(os.path.join(D, "log.txt"), "a") as f:
        f.write(" ".join(argv) + "\n")
    cmd = argv[:2]
    if argv[:1] == ["auth"]:
        return 0 if os.environ.get("FAKE_GH_AUTH", "1") == "1" else 1
    if argv[:2] == ["api", "user"]:
        print(os.environ.get("FAKE_GH_USER", "alice"))
        return 0
    if argv[0] == "api":
        path = [a for a in argv[1:] if not a.startswith("-") and "=" not in a and a not in ("POST",)][0]
        m = re.match(r"repos/[^/]+/[^/]+/issues/(\d+)$", path)
        if m:
            i = s["issues"].get(m.group(1))
            if not i:
                sys.stderr.write("HTTP 404\n")
                return 1
            print(json.dumps({"number": i["number"], "title": i["title"], "body": i["body"], "user": {"login": i["user"], "id": user_id(i["user"])}, "state": i["state"]}))
            return 0
        # Accounts: the logins in FAKE_GH_ORGS are organizations, every other login a user.
        m = re.match(r"users/([^/]+)$", path)
        if m:
            orgs = [x.strip().lower() for x in os.environ.get("FAKE_GH_ORGS", "").split(",")]
            kind = "Organization" if m.group(1).lower() in orgs else "User"
            print(kind if opt(argv, "--jq") == ".type" else json.dumps({"login": m.group(1), "id": user_id(m.group(1)), "type": kind}))
            return 0
        # Public memberships (FAKE_GH_PUBLIC_MEMBERS, org:user pairs) answer 204 to anyone;
        # all memberships (FAKE_GH_MEMBERS adds the private ones) only to a member asking.
        m = re.match(r"orgs/([^/]+)/(public_members|members)/([^/]+)$", path)
        if m:
            org, which, user = m.groups()
            ok = listed("FAKE_GH_PUBLIC_MEMBERS", org, user) or (which == "members" and listed("FAKE_GH_MEMBERS", org, user))
            if not ok:
                sys.stderr.write("HTTP 404\n")
                return 1
            return 0
        m = re.match(r"repos/[^/]+/[^/]+/collaborators/([^/]+)/permission$", path)
        if m:
            maint = os.environ.get("FAKE_GH_MAINTAINERS", "maintainer").split(",")
            print(json.dumps({"permission": "admin" if m.group(1) in maint else "read"}))
            return 0
        m = re.match(r"repos/[^/]+/[^/]+/statuses/([0-9a-f]+)$", path)
        if m:
            fields = dict(x.split("=", 1) for x in opts(argv, "-f"))
            s["statuses"].append({"sha": m.group(1), **fields})
            save(s)
            print("{}")
            return 0
        sys.stderr.write(f"fake gh: unknown api {path}\n")
        return 1
    if cmd == ["issue", "create"]:
        n = s["next"]
        s["next"] += 1
        s["issues"][str(n)] = {"number": n, "title": opt(argv, "--title", ""), "body": body_of(argv), "user": os.environ.get("FAKE_GH_USER", "alice"), "state": "open"}
        save(s)
        print(f"https://github.com/{repo}/issues/{n}")
        return 0
    if cmd == ["issue", "comment"]:
        s["comments"].setdefault(argv[2], []).append(body_of(argv))
        save(s)
        return 0
    if cmd == ["issue", "close"]:
        s["issues"][argv[2]]["state"] = "closed"
        save(s)
        return 0
    if cmd == ["issue", "list"]:
        for i in sorted(s["issues"].values(), key=lambda x: x["number"]):
            if i["state"] == "open" and re.match(r"^ *publish[: ]", i["title"], re.I):
                print(i["number"])
        return 0
    if cmd == ["pr", "list"]:
        head = opt(argv, "--head")
        for p in s["prs"].values():
            if p["head"] == head and p["state"] == "open":
                print(p["number"])
                break
        return 0
    if cmd == ["pr", "create"]:
        n = s["next"]
        s["next"] += 1
        s["prs"][str(n)] = {"number": n, "head": opt(argv, "--head"), "base": opt(argv, "--base"), "title": opt(argv, "--title"), "body": body_of(argv), "state": "open"}
        save(s)
        print(f"https://github.com/{repo}/pull/{n}")
        return 0
    if cmd == ["pr", "edit"]:
        p = s["prs"][argv[2]]
        p["body"] = body_of(argv)
        p["title"] = opt(argv, "--title", p["title"])
        save(s)
        return 0
    if cmd == ["pr", "view"]:
        print(s["prs"][argv[2]]["body"])
        return 0
    if cmd == ["pr", "merge"]:
        # As GitHub does: the squashed commit's message is the subject and body given, its author
        # the account of --author-email (else the pull request's), and --match-head-commit must
        # be the head. FAKE_GH_REFUSE_AUTHOR_EMAIL=1 refuses an author email, as GitHub may.
        p = s["prs"][argv[2]]
        bare = os.environ["FAKE_GH_INDEX"]
        head = git("rev-parse", f"refs/heads/{p['head']}", cwd=bare)
        st = [x for x in s["statuses"] if x["sha"] == head and x.get("context") == "index-guard"]
        if not st or st[-1].get("state") != "success":
            sys.stderr.write("fake gh: the required status check index-guard has not passed\n")
            return 1
        want = opt(argv, "--match-head-commit")
        if want and want != head:
            sys.stderr.write(f"fake gh: the head is {head}, not {want}\n")
            return 1
        email = opt(argv, "--author-email")
        if email and os.environ.get("FAKE_GH_REFUSE_AUTHOR_EMAIL") == "1":
            sys.stderr.write("fake gh: GraphQL: the author email can't be used\n")
            return 1
        subject = opt(argv, "--subject", f"{p['title']} (#{p['number']})")
        body = body_of(argv) if (opt(argv, "--body-file") or opt(argv, "--body")) else ""
        with tempfile.TemporaryDirectory() as t:
            git("clone", "-q", bare, t)
            git("checkout", "-q", "main", cwd=t)
            git("merge", "-q", "--squash", f"origin/{p['head']}", cwd=t)
            who = ["--author", f"{email.split('@')[0].split('+')[-1]} <{email}>"] if email else []
            git("-c", "user.name=github-actions[bot]", "-c", "user.email=41898282+github-actions[bot]@users.noreply.github.com", "commit", "-q", *who, "-m", subject + ("\n\n" + body.strip() if body.strip() else ""), cwd=t)
            git("push", "-q", "origin", "main", cwd=t)
            git("push", "-q", "origin", "--delete", p["head"], cwd=t)
        p["state"] = "merged"
        p["merge"] = {"author_email": email, "subject": subject, "body": body}
        save(s)
        return 0
    if cmd == ["release", "view"]:
        return 0 if os.path.isdir(os.path.join(os.environ["FAKE_GH_RELEASES"], argv[2])) else 1
    if cmd in (["release", "create"], ["release", "upload"]):
        d = os.path.join(os.environ["FAKE_GH_RELEASES"], argv[2])
        if cmd[1] == "create" and os.path.isdir(d):
            sys.stderr.write("fake gh: release exists\n")
            return 1
        os.makedirs(d, exist_ok=True)
        for f in argv[3:]:
            if f.startswith("-"):
                break
            shutil.copy(f.split("#")[0], os.path.join(d, os.path.basename(f.split("#")[0])))
        return 0
    if cmd == ["workflow", "run"]:
        s["runs"].append({"workflow": argv[2], "fields": opts(argv, "-f")})
        save(s)
        return 0
    sys.stderr.write(f"fake gh: unsupported: {' '.join(argv)}\n")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
