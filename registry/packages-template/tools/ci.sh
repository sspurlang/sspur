#!/usr/bin/env bash
# The steps of the registry workflows. Each GitHub Actions job runs one of them; the end-to-end
# test in the sspur repository runs them in order against local repositories and a stand-in for
# the gh CLI. The decisions (checks, entries, what may merge) are made by the SSPUR program in
# tools/registry; this script moves files and calls git and gh.
#
#   ci.sh request ISSUE    read and parse a request, resolve its ref to a commit
#   ci.sh check            gather the package facts and its snapshot (no package code runs)
#   ci.sh tests            run the package's tests, native and interpreted
#   ci.sh write            decide, update the pull request, merge it when every blocking check passes
#   ci.sh sweep            rerun open requests whose ref moved
#   ci.sh guard BASE       check an index change against BASE (append-only, valid files)
#   ci.sh site OUT         build the package site
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
INDEX_DIR="${INDEX_DIR:-$(cd "$here/.." && pwd)}"
SSPUR="${SSPUR:-sspur}"
GH="${GH:-gh}"
REPO="${GITHUB_REPOSITORY:-utkarshavardhana/sspur-packages}"
WORK="${WORK:-${RUNNER_TEMP:-/tmp}/registry}"
GIT_BASE="${GIT_BASE:-https://github.com}"
SNAPSHOT_BASE="${SNAPSHOT_BASE:-https://github.com/$REPO/releases/download}"
CHECKS="$here/registry/registry.ssp"
BOT_NAME="github-actions[bot]"
BOT_EMAIL="41898282+github-actions[bot]@users.noreply.github.com"
export SSPUR_CACHE="${SSPUR_CACHE:-$WORK/cache}"
mkdir -p "$WORK"

# The registry program: one JSON object on stdin, its answer on stdout.
reg() { "$SSPUR" run --interp "$CHECKS"; }

json_or_null() { if [ -s "$1" ]; then cat "$1"; else printf 'null'; fi; }

output() {
  if [ -n "${GITHUB_OUTPUT:-}" ]; then cat "$1" >> "$GITHUB_OUTPUT"; fi
  cat "$1"
  echo
}

# Loads the key=value lines of a vars file into shell variables (v_name, v_commit, ...).
load_vars() {
  while IFS='=' read -r k v; do
    case "$k" in
      ok|kind|name|version|commit|issue|slug|ref|passed|blocking_failed|failed) printf -v "v_$k" '%s' "$v" ;;
    esac
  done < "$1"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

cmd_request() {
  local n="${1:?issue number}"
  "$GH" api "repos/$REPO/issues/$n" > "$WORK/issue.json"
  printf '{"cmd":"parse-request","issue":%s}' "$(cat "$WORK/issue.json")" | reg > "$WORK/parsed0.json"
  printf '{"cmd":"vars","parsed":%s}' "$(cat "$WORK/parsed0.json")" | reg > "$WORK/vars0.txt"
  load_vars "$WORK/vars0.txt"
  local commit="" tag=false
  if [ "$v_ok" = true ] && [ "$v_kind" = publish ] && [ -n "$v_slug" ] && [ -n "$v_ref" ]; then
    local url="$GIT_BASE/$v_slug.git" refs
    refs="$(git ls-remote "$url" "refs/tags/$v_ref" "refs/tags/$v_ref^{}" "refs/heads/$v_ref" 2>/dev/null || true)"
    commit="$(printf '%s\n' "$refs" | awk -v r="refs/tags/$v_ref^{}" '$2 == r {print $1}')"
    [ -n "$commit" ] || commit="$(printf '%s\n' "$refs" | awk -v r="refs/tags/$v_ref" '$2 == r {print $1}')"
    if [ -n "$commit" ]; then tag=true; fi
    [ -n "$commit" ] || commit="$(printf '%s\n' "$refs" | awk -v r="refs/heads/$v_ref" '$2 == r {print $1}')"
    if [ -z "$commit" ] && printf '%s' "$v_ref" | grep -Eq '^[0-9a-f]{40}$'; then commit="$v_ref"; fi
  fi
  printf '{"cmd":"parse-request","issue":%s,"commit":"%s","tag":%s}' "$(cat "$WORK/issue.json")" "$commit" "$tag" | reg > "$WORK/parsed.json"
  printf '{"cmd":"vars","parsed":%s}' "$(cat "$WORK/parsed.json")" | reg > "$WORK/vars.txt"
  output "$WORK/vars.txt"
}

clone_package() {
  load_vars "$WORK/vars.txt"
  rm -rf "$WORK/pkg"
  git -c core.autocrlf=false clone -q --no-checkout "$GIT_BASE/$v_slug.git" "$WORK/pkg"
  git -C "$WORK/pkg" -c core.autocrlf=false checkout -q --detach "$v_commit"
  # Dependencies resolve from the index, never from a lock the package ships.
  rm -f "$WORK/pkg/sspur.lock"
}

cmd_check() {
  load_vars "$WORK/vars.txt"
  [ "$v_kind" = publish ] && [ -n "$v_commit" ] || { echo "nothing to check"; return 0; }
  clone_package
  local facts="$WORK/facts.json"
  if [ -f "$WORK/pkg/sspur.toml" ] && (cd "$WORK/pkg" && SSPUR_INDEX="$INDEX_DIR" "$SSPUR" api --json --rev "$v_commit") > "$facts" 2> "$WORK/api.err"; then
    :
  else
    local why="no sspur.toml at the top of the repository"
    [ -f "$WORK/pkg/sspur.toml" ] && why="$(head -c 400 "$WORK/api.err" | tr '\n"\\' '   ')"
    printf '{"source":"","parse":false,"checked":false,"diags":[{"code":"E_PACKAGE","severity":"error","file":"sspur.toml","line":0,"col":0,"msg":"%s","hint":null}],"fmt":{"clean":false},"defs":[],"holes":[],"run_cmd":[],"tests":0,"tested":[]}' "$why" > "$facts"
  fi
  rm -f "$WORK/snapshot.tar" "$WORK/snapshot.json"
  if (cd "$WORK/pkg" && "$SSPUR" archive --rev "$v_commit" -o "$WORK/snapshot.tar") > "$WORK/snapshot.txt" 2>/dev/null; then
    read -r sha size < "$WORK/snapshot.txt"
    printf '{"url":"%s/%s-%s/%s-%s.tar","sha256":"%s","size":%s}' "$SNAPSHOT_BASE" "$v_name" "$v_version" "$v_name" "$v_version" "$sha" "$size" > "$WORK/snapshot.json"
  fi
  local prev="$INDEX_DIR/packages/${v_name:0:2}/$v_name.json"
  if [ -f "$prev" ]; then cp "$prev" "$WORK/previous.json"; else rm -f "$WORK/previous.json"; fi
  if [ -f "$INDEX_DIR/index.json" ]; then cp "$INDEX_DIR/index.json" "$WORK/summary.json"; else rm -f "$WORK/summary.json"; fi
  echo "facts and snapshot of $v_name $v_version at $v_commit are in $WORK"
}

cmd_tests() {
  load_vars "$WORK/vars.txt"
  [ "$v_kind" = publish ] && [ -n "$v_commit" ] || { echo "nothing to test"; return 0; }
  clone_package
  local tmo=""
  if command -v timeout >/dev/null 2>&1; then tmo="timeout 1200"; fi
  if ! (cd "$WORK/pkg" && SSPUR_INDEX="$INDEX_DIR" $tmo "$SSPUR" publish --tests-json) > "$WORK/tests.json" 2> "$WORK/tests.err"; then
    printf '{"native":{"ran":false,"passed":0,"failed":0,"detail":"the tests did not finish"},"interp":{"ran":false,"passed":0,"failed":0,"detail":"the tests did not finish"}}' > "$WORK/tests.json"
  fi
  cat "$WORK/tests.json"
  echo
}

comment() {
  "$GH" issue comment "$1" --repo "$REPO" --body-file "$2" > /dev/null
}

# The open pull request from `branch`, or nothing.
pr_of() {
  "$GH" pr list --repo "$REPO" --head "$1" --state open --json number --jq '.[0].number // empty'
}

cmd_write() {
  load_vars "$WORK/vars.txt"
  local issue="$v_issue"
  if [ "$v_ok" != true ]; then
    printf '{"cmd":"render","parsed":%s,"repo":"%s","part":"comment"}' "$(cat "$WORK/parsed.json")" "$REPO" | reg > "$WORK/comment.md"
    comment "$issue" "$WORK/comment.md"
    echo "the request has errors; commented on #$issue"
    return 0
  fi
  local name="$v_name" version="$v_version" kind="$v_kind"
  local file="packages/${name:0:2}/$name.json"
  local branch="$kind/$name-$version"
  local now="${NOW:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"
  cd "$INDEX_DIR"
  git fetch -q origin main
  git checkout -q -B "$branch" origin/main
  local old="$WORK/old.json"
  if [ -f "$file" ]; then cp "$file" "$old"; else rm -f "$old"; fi
  rm -f "$WORK/report.json" "$WORK/entry.json" "$WORK/package.json" "$WORK/guard.json"
  local report="null" mergeable=true
  if [ "$kind" = publish ]; then
    if [ -z "$v_commit" ]; then
      printf 'The ref `%s` was not found in %s. Push the tag (or branch), then edit the request or comment `/recheck`.\n' "$v_ref" "$GIT_BASE/$v_slug" > "$WORK/comment.md"
      comment "$issue" "$WORK/comment.md"
      return 0
    fi
    [ -s "$WORK/facts.json" ] || { echo "no facts from the check job" >&2; return 1; }
    # The index as it is now, not as the check job saw it: another version may have merged since.
    printf '{"cmd":"check","facts":%s,"parsed":%s,"previous":%s,"summary":%s,"tests":%s,"env":"ci"}' \
      "$(cat "$WORK/facts.json")" "$(cat "$WORK/parsed.json")" "$(json_or_null "$old")" "$(json_or_null index.json)" "$(json_or_null "$WORK/tests.json")" | reg > "$WORK/report.json"
    if grep -q '^{"error"' "$WORK/report.json"; then cat "$WORK/report.json" >&2; return 1; fi
    printf '{"cmd":"vars","report":%s}' "$(cat "$WORK/report.json")" | reg > "$WORK/report-vars.txt"
    load_vars "$WORK/report-vars.txt"
    [ "$v_passed" = true ] || mergeable=false
    printf '{"cmd":"entry","facts":%s,"parsed":%s,"snapshot":%s,"now":"%s","git_base":"%s"}' \
      "$(cat "$WORK/facts.json")" "$(cat "$WORK/parsed.json")" "$(json_or_null "$WORK/snapshot.json")" "$now" "$GIT_BASE" | reg > "$WORK/entry.json"
    report="$(cat "$WORK/report.json")"
  fi
  printf '{"cmd":"apply","package":%s,"entry":%s,"parsed":%s,"facts":%s}' \
    "$(json_or_null "$old")" "$(json_or_null "$WORK/entry.json")" "$(cat "$WORK/parsed.json")" "$(json_or_null "$WORK/facts.json")" | reg > "$WORK/package.json"
  if grep -q '^{"error"' "$WORK/package.json"; then
    printf '{"cmd":"render","parsed":%s,"repo":"%s","part":"comment","note":%s}' "$(cat "$WORK/parsed.json")" "$REPO" "$(sed -n 's/^{"error": \(.*\)}$/\1/p' "$WORK/package.json")" | reg > "$WORK/comment.md"
    comment "$issue" "$WORK/comment.md"
    echo "refused: $(cat "$WORK/comment.md")"
    git checkout -q --detach origin/main
    return 0
  fi
  mkdir -p "$(dirname "$file")"
  cp "$WORK/package.json" "$file"
  regen_summary
  printf '{"cmd":"guard","path":"%s","base":%s,"head":%s}' "$file" "$(json_or_null "$old")" "$(cat "$file")" | reg > "$WORK/guard.json"
  grep -q '"ok":true' "$WORK/guard.json" || mergeable=false
  git add -A packages index.json
  if ! git diff --cached --quiet; then
    git -c user.name="$BOT_NAME" -c user.email="$BOT_EMAIL" commit -q -m "$kind: $name $version (#$issue)"
  fi
  # Only the package's own file and index.json may change in an automatic pull request.
  local paths
  paths="$(git diff --name-only origin/main..."$branch" | sort -u)"
  if [ -n "$(printf '%s\n' "$paths" | grep -v -x -e "$file" -e index.json -e '' || true)" ]; then
    echo "the branch changes files other than $file and index.json:" >&2
    printf '%s\n' "$paths" >&2
    mergeable=false
  fi
  if [ -z "$paths" ]; then
    printf '%s %s is already in the index as requested; nothing to change.\n' "$name" "$version" > "$WORK/comment.md"
    comment "$issue" "$WORK/comment.md"
    "$GH" issue close "$issue" --repo "$REPO" > /dev/null || true
    git checkout -q --detach origin/main
    echo "already in the index: $name $version"
    return 0
  fi
  git push -q -f origin "$branch"
  local pr
  pr="$(pr_of "$branch")"
  printf '{"cmd":"render","report":%s,"parsed":%s,"repo":"%s","part":"body"}' "$report" "$(cat "$WORK/parsed.json")" "$REPO" | reg > "$WORK/body.md"
  if grep -q '"ok":false' "$WORK/guard.json"; then
    printf '\n### Index guard\n\n%s\n' "$(sed 's/.*"errors":\[\(.*\)\]}/\1/' "$WORK/guard.json")" >> "$WORK/body.md"
  fi
  local title="$kind: $name $version"
  if [ -z "$pr" ]; then
    pr="$("$GH" pr create --repo "$REPO" --base main --head "$branch" --title "$title" --body-file "$WORK/body.md" | sed -n 's#.*/pull/\([0-9][0-9]*\).*#\1#p' | tail -n1)"
  else
    "$GH" pr edit "$pr" --repo "$REPO" --title "$title" --body-file "$WORK/body.md" > /dev/null
  fi
  local head_sha state=failure desc="blocking checks failed"
  head_sha="$(git rev-parse "$branch")"
  if [ "$mergeable" = true ]; then state=success; desc="every blocking check passed"; fi
  "$GH" api -X POST "repos/$REPO/statuses/$head_sha" -f state="$state" -f context=index-guard -f description="$desc" > /dev/null
  printf '{"cmd":"render","report":%s,"parsed":%s,"repo":"%s","pr":%s,"part":"comment"}' "$report" "$(cat "$WORK/parsed.json")" "$REPO" "${pr:-null}" | reg > "$WORK/comment.md"
  if [ "$mergeable" = true ]; then
    if [ "$kind" = publish ] && [ -s "$WORK/snapshot.tar" ]; then
      local tag="$name-$version" asset="$WORK/$name-$version.tar"
      cp "$WORK/snapshot.tar" "$asset"
      if "$GH" release view "$tag" --repo "$REPO" > /dev/null 2>&1; then
        "$GH" release upload "$tag" "$asset" --repo "$REPO" --clobber > /dev/null
      else
        "$GH" release create "$tag" "$asset" --repo "$REPO" --title "$name $version" --notes "Source snapshot of $name $version (commit $v_commit, sha256 $(sha256_of "$asset")). Published by request #$issue." > /dev/null
      fi
    fi
    local try
    for try in 1 2 3 4 5; do
      if "$GH" pr merge "$pr" --repo "$REPO" --squash --delete-branch > /dev/null; then break; fi
      [ "$try" = 5 ] && { echo "cannot merge #$pr" >&2; return 1; }
      sleep "${RETRY_SLEEP:-6}"
    done
    "$GH" workflow run pages.yml --repo "$REPO" --ref main > /dev/null 2>&1 || echo "note: could not start the Pages workflow"
    comment "$issue" "$WORK/comment.md"
    "$GH" issue close "$issue" --repo "$REPO" > /dev/null || true
    echo "merged #$pr: $name $version"
  else
    comment "$issue" "$WORK/comment.md"
    echo "left #$pr open: $name $version (${v_failed:-guard})"
  fi
  git checkout -q --detach origin/main
}

regen_summary() {
  {
    printf '{"cmd":"summary","packages":['
    local first=1 f
    for f in packages/*/*.json; do
      [ -f "$f" ] || continue
      if [ $first = 1 ]; then first=0; else printf ','; fi
      cat "$f"
    done
    printf ']}'
  } | reg > index.json.new
  mv index.json.new index.json
}

cmd_sweep() {
  local n
  for n in $("$GH" issue list --repo "$REPO" --state open --limit 200 --json number,title --jq '.[] | select(.title | test("^ *publish[: ]"; "i")) | .number'); do
    GITHUB_OUTPUT="" WORK="$WORK/sweep-$n" "$0" request "$n" > /dev/null 2>&1 || continue
    load_vars "$WORK/sweep-$n/vars.txt"
    [ -n "$v_commit" ] || continue
    local pr body
    pr="$(pr_of "publish/$v_name-$v_version")"
    body=""
    [ -n "$pr" ] && body="$("$GH" pr view "$pr" --repo "$REPO" --json body --jq .body)"
    case "$body" in
      *"commit=$v_commit "*) ;;
      *) echo "#$n: $v_name $v_version is at ${v_commit:0:12}; checking it again"
         "$GH" workflow run publish.yml --repo "$REPO" -f issue="$n" ;;
    esac
  done
}

cmd_guard() {
  local base="${1:?base ref}" f status=0
  cd "$INDEX_DIR"
  for f in $(git diff --name-only "$base"...HEAD -- packages); do
    local old="$WORK/guard-old.json"
    if git cat-file -e "$base:$f" 2>/dev/null; then git show "$base:$f" > "$old"; else rm -f "$old"; fi
    local new="null"
    [ -f "$f" ] && new="$(cat "$f")"
    printf '{"cmd":"guard","path":"%s","base":%s,"head":%s}' "$f" "$(json_or_null "$old")" "$new" | reg > "$WORK/guard.json"
    if ! grep -q '"ok":true' "$WORK/guard.json"; then
      echo "$f: $(cat "$WORK/guard.json")"
      status=1
    fi
  done
  regen_summary
  if ! git diff --quiet -- index.json; then
    echo "index.json does not match the package files; regenerate it with tools/ci.sh"
    git checkout -q -- index.json
    status=1
  fi
  return $status
}

cmd_site() {
  local out="${1:?output directory}"
  mkdir -p "$out" "$WORK/src"
  cd "$INDEX_DIR"
  printf '{"cmd":"latest","packages":[' > "$WORK/latest-in.json"
  local first=1 f
  for f in packages/*/*.json; do
    [ -f "$f" ] || continue
    if [ $first = 1 ]; then first=0; else printf ',' >> "$WORK/latest-in.json"; fi
    cat "$f" >> "$WORK/latest-in.json"
  done
  printf ']}' >> "$WORK/latest-in.json"
  reg < "$WORK/latest-in.json" > "$WORK/latest.txt"
  local name version
  while read -r name version; do
    [ -n "$name" ] || continue
    rm -rf "$WORK/src/$name"
    SSPUR_INDEX="$INDEX_DIR" "$SSPUR" source "$name@=$version" "$WORK/src/$name" > /dev/null || echo "note: no source for $name $version"
  done < "$WORK/latest.txt"
  "$SSPUR" run --interp "$here/site/main.ssp" "$INDEX_DIR" "$WORK/src" "$out" "$here/../site"
}

case "${1:-}" in
  request) shift; cmd_request "$@" ;;
  check) cmd_check ;;
  tests) cmd_tests ;;
  write) cmd_write ;;
  sweep) cmd_sweep ;;
  guard) shift; cmd_guard "$@" ;;
  site) shift; cmd_site "$@" ;;
  *) sed -n '2,15p' "$0"; exit 2 ;;
esac
