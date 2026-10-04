#!/usr/bin/env bash
#
# Compare main's live protection settings against .github/required-checks.json.
# Run daily by .github/workflows/branch-protection-drift.yml; self-tested in lint.
#
# ── What is compared ──
#
#   required_status_checks  `.checks` (context AND app_id) and `strict` against
#                           `required` / `strict`. ALWAYS a hard failure: this is
#                           the drift the job has guarded since U-562.
#   enforce_admins          `.enforce_admins.enabled` against `enforce_admins`.
#   tag ruleset             the one repo ruleset with target "tag" and the
#                           baseline's name: enforcement, ref_name include/exclude,
#                           rule types (order-insensitive), and bypass_actors WHEN
#                           THE API RETURNS THEM (see below).
#
# ── pending_settings ──
#
# enforce_admins and the tag ruleset are applied by the maintainer by hand, after
# this ships. Until then the baseline says `"pending_settings": true`, and a
# mismatch in those two is a ::warning::, not a failure -- a job that is red on
# main for weeks masks the context drift it exists to catch. Flipping the flag to
# false (in the same PR that records the applied settings) makes them hard
# failures. Context/strict drift fails regardless. The red proof for the
# hard-fail path is a `gh workflow run branch-protection-drift.yml --ref <branch>`
# with the flag set to false on that branch, plus the self-test below.
#
# ── bypass_actors ──
#
# The rulesets API may return `bypass_actors` only to callers that can write the
# ruleset; this job's token has administration:READ. If the field is absent the
# bypass list is reported as unverifiable (::notice::) and stays a
# maintainer-checked item; if present it is compared like everything else.
#
# ── Modes ──
#
#   --fetch      GET the live settings with `gh api` (needs GH_TOKEN and
#                GITHUB_REPOSITORY) and compare. Default. A failed GET is a hard
#                failure with its own message -- "could not read" must never be
#                reported the same way as "nothing configured".
#   --compare BASELINE PROTECTION RULESETS
#                compare saved JSON: PROTECTION is GET /branches/main/protection,
#                RULESETS is a JSON ARRAY of GET /rulesets/{id} objects.
#   --self-test  NEGATIVE CONTROLS over fixture JSON: asserts each class of drift
#                is reported at the right level, and that matching settings pass.
#                A gate that cannot fail is not a gate (assert-upgrade-notes.sh).
#
# Exit status: 1 if any ::error:: was emitted, else 0.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
BASELINE_DEFAULT="$ROOT/.github/required-checks.json"
REPO="${GITHUB_REPOSITORY:-OWNER/REPO}"

# One finding per output line: "<level>\t<message>". Levels: error, warning, notice.
# shellcheck disable=SC2016 # a jq program: the $names are jq variables, not shell ones.
JQ_FINDINGS='
def norm_checks: (. // []) | map({context, app_id}) | sort_by(.context);
def types: (. // []) | map(.type) | unique;
def norm_actors: (. // []) | map({actor_id, actor_type, bypass_mode})
                 | sort_by([.actor_type, (.actor_id | tostring), .bypass_mode]);
def sorted: (. // []) | sort;

$b[0] as $b | $p[0] as $p | $r[0] as $rs |
($b.pending_settings == true) as $pending |
(if $pending then "warning" else "error" end) as $lvl |
"if the live setting was changed on purpose, update .github/required-checks.json in a PR; if the file is right, restore the setting (pinned read-modify-write, never POST .../contexts): jq \u0027{strict, checks: .required}\u0027 .github/required-checks.json | gh api --method PATCH repos/\($repo)/branches/main/protection/required_status_checks --input -" as $fix_checks |
"jq \u0027.tag_ruleset | .rules |= map({type: .})\u0027 .github/required-checks.json | gh api" as $rs_body |
(($p.required_status_checks // {}).checks | norm_checks) as $live |
($b.required | norm_checks) as $want |
($live | map(.context)) as $live_names |
($want | map(.context)) as $want_names |
($b.tag_ruleset.name) as $rs_name |
(($rs // []) | map(select(.target == "tag"))) as $tags |
($tags | map(select(.name == $rs_name))) as $mine |

# ---- required status checks: always hard ----
[
  (if $p.required_status_checks == null then
     {level: "error", msg: "branch protection has NO required_status_checks at all. Fix: \($fix_checks)"}
   else empty end),
  (if $p.required_status_checks != null and $p.required_status_checks.strict != $b.strict then
     {level: "error", msg: "required_status_checks.strict is \($p.required_status_checks.strict | tojson), baseline says \($b.strict | tojson). Fix: \($fix_checks)"}
   else empty end),
  ($want[] | . as $w | ($live | map(select(.context == $w.context))) as $hit |
     if ($hit | length) == 0 then
       {level: "error", msg: "required check `\($w.context)` is in the baseline but NOT required live. Fix: \($fix_checks)"}
     elif $hit[0].app_id != $w.app_id then
       {level: "error", msg: "required check `\($w.context)` is pinned to app_id \($hit[0].app_id | tojson) live, baseline pins \($w.app_id) -- an unpinned or wrongly pinned context can be satisfied by another app. Fix: \($fix_checks)"}
     else empty end),
  ($live[] | select(.context as $c | $want_names | index($c) | not) |
     if $pending and (.context as $c | ($b.advisory_pending // []) | index($c)) then
       {level: "warning", msg: "`\(.context)` is already required live but still advisory_pending in the baseline: merge the PR that moves it to `required` (and sets pending_settings false)."}
     else
       {level: "error", msg: "`\(.context)` (app_id \(.app_id | tojson)) is required live but not in the baseline. If intended, add it to `required` in .github/required-checks.json; otherwise Fix: \($fix_checks)"}
     end)
]
+
# ---- enforce_admins: hard unless pending_settings ----
[
  (if (($p.enforce_admins // {}).enabled) != $b.enforce_admins then
     {level: $lvl, msg: "enforce_admins is \((($p.enforce_admins // {}).enabled) | tojson) live, baseline says \($b.enforce_admins | tojson). Fix: gh api --method \(if $b.enforce_admins then "POST" else "DELETE" end) repos/\($repo)/branches/main/protection/enforce_admins"}
   else empty end)
]
+
# ---- tag ruleset: hard unless pending_settings ----
[
  (if ($mine | length) == 0 then
     {level: $lvl, msg: "no tag ruleset named `\($rs_name)` (tag rulesets present: \($tags | map(.name) | tojson)). Fix: \($rs_body) --method POST repos/\($repo)/rulesets --input -"}
   elif ($mine | length) > 1 then
     {level: "error", msg: "\($mine | length) tag rulesets are named `\($rs_name)` (ids \($mine | map(.id) | tojson)): ambiguous, delete the extras."}
   else
     $mine[0] as $m |
     ("Fix: \($rs_body) --method PUT repos/\($repo)/rulesets/\($m.id) --input -") as $fix |
     (if $m.enforcement != $b.tag_ruleset.enforcement then
        {level: $lvl, msg: "tag ruleset `\($rs_name)` enforcement is \($m.enforcement | tojson), baseline \($b.tag_ruleset.enforcement | tojson). \($fix)"}
      else empty end),
     (if ($m.conditions.ref_name.include | sorted) != ($b.tag_ruleset.conditions.ref_name.include | sorted) then
        {level: $lvl, msg: "tag ruleset `\($rs_name)` ref_name.include is \($m.conditions.ref_name.include | tojson), baseline \($b.tag_ruleset.conditions.ref_name.include | tojson). \($fix)"}
      else empty end),
     (if ($m.conditions.ref_name.exclude | sorted) != ($b.tag_ruleset.conditions.ref_name.exclude | sorted) then
        {level: $lvl, msg: "tag ruleset `\($rs_name)` ref_name.exclude is \($m.conditions.ref_name.exclude | tojson), baseline \($b.tag_ruleset.conditions.ref_name.exclude | tojson). \($fix)"}
      else empty end),
     (($m.rules | types) as $have | ($b.tag_ruleset.rules | sorted) as $need |
        (($need - $have) | if length > 0 then
           {level: $lvl, msg: "tag ruleset `\($rs_name)` is missing rule type(s) \(tojson). \($fix)"} else empty end),
        (($have - $need) | if length > 0 then
           {level: $lvl, msg: "tag ruleset `\($rs_name)` has rule type(s) \(tojson) the baseline does not list. \($fix)"} else empty end)),
     (if ($m | has("bypass_actors")) and $m.bypass_actors != null then
        (if ($m.bypass_actors | norm_actors) != ($b.tag_ruleset.bypass_actors | norm_actors) then
           {level: $lvl, msg: "tag ruleset `\($rs_name)` bypass_actors is \($m.bypass_actors | norm_actors | tojson), baseline \($b.tag_ruleset.bypass_actors | norm_actors | tojson). \($fix)"}
         else empty end)
      else
        {level: "notice", msg: "tag ruleset `\($rs_name)`: bypass_actors not returned to this token, so the bypass list is unverifiable here; it stays a maintainer check (expected \($b.tag_ruleset.bypass_actors | norm_actors | tojson))."}
      end)
   end),
  ($tags[] | select(.name != $rs_name) |
     {level: "warning", msg: "additional tag ruleset `\(.name)` (id \(.id)) is not in the baseline: review it, and record it in .github/required-checks.json if it is meant to stay."})
]
| . as $all
| $all + (if $pending and ($all | map(select(.level == "warning" or .level == "error")) | length) == 0 then
           [{level: "notice", msg: "pending_settings is true but enforce_admins and the tag ruleset already match the baseline: set it to false so they become hard failures."}]
         else [] end)
| .[] | "\(.level)\t\(.msg)"
'

# compare BASELINE PROTECTION RULESETS -> prints annotations, returns 1 on any error.
compare() {
  local baseline="$1" protection="$2" rulesets="$3" out level msg errors=0
  # A jq failure (unreadable or malformed input) must fail, never fall through
  # to "no findings" -- which would print as a clean pass.
  out="$(jq -rn --arg repo "$REPO" \
    --slurpfile b "$baseline" --slurpfile p "$protection" --slurpfile r "$rulesets" \
    "$JQ_FINDINGS")" || {
    echo "::error::could not evaluate drift: jq failed on $baseline / $protection / $rulesets"
    return 1
  }
  while IFS=$'\t' read -r level msg; do
    [ -n "$level" ] || continue
    printf '::%s::%s\n' "$level" "$msg"
    [ "$level" != "error" ] || errors=$((errors + 1))
  done <<< "$out"
  if [ "$errors" -gt 0 ]; then
    echo "drift: ${errors} error(s)."
    return 1
  fi
  echo "drift: none that fails (pending_settings=$(jq -c '.pending_settings' "$baseline"))."
  return 0
}

fetch() {
  local tmp ids id
  : "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064 # expand now: $tmp is fixed for this run.
  trap "rm -rf '$tmp'" EXIT
  local api="repos/${GITHUB_REPOSITORY}"

  if ! gh api "$api/branches/main/protection" > "$tmp/protection.json"; then
    echo "::error::could not READ main's branch protection (GET $api/branches/main/protection failed). This is not drift: the check could not run. Does the token still have administration:read?"
    return 1
  fi
  if ! gh api "$api/rulesets?includes_parents=false&per_page=100" > "$tmp/list.json"; then
    echo "::error::could not LIST rulesets (GET $api/rulesets failed). This is not 'no ruleset': the check could not run. Token scope (metadata:read) or plan?"
    return 1
  fi
  if ! jq -e 'type == "array"' "$tmp/list.json" > /dev/null; then
    echo "::error::GET $api/rulesets did not return an array: $(head -c 300 "$tmp/list.json")"
    return 1
  fi
  ids="$(jq -r '.[] | select(.target == "tag") | .id' "$tmp/list.json")"
  echo "[]" > "$tmp/rulesets.json"
  for id in $ids; do
    if ! gh api "$api/rulesets/$id" > "$tmp/one.json"; then
      echo "::error::could not READ tag ruleset $id (GET $api/rulesets/$id failed)."
      return 1
    fi
    jq --slurpfile one "$tmp/one.json" '. + $one' "$tmp/rulesets.json" > "$tmp/next.json"
    mv "$tmp/next.json" "$tmp/rulesets.json"
  done
  echo "tag rulesets read: $(jq -c 'map({id, name, enforcement})' "$tmp/rulesets.json")"
  compare "$BASELINE_DEFAULT" "$tmp/protection.json" "$tmp/rulesets.json"
}

# ── self-test ────────────────────────────────────────────────────────────────

self_test() {
  local dir
  dir="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "rm -rf '$dir'" EXIT

  # Baseline as it will look AFTER the settings window (pending_settings false).
  cat > "$dir/b.json" <<'JSON'
{"strict": true,
 "required": [{"context": "rustfmt", "app_id": 15368}, {"context": "lint", "app_id": 15368}],
 "advisory_pending": ["msrv"],
 "enforce_admins": true,
 "pending_settings": false,
 "tag_ruleset": {"name": "protect-release-tags", "target": "tag", "enforcement": "active",
   "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}},
   "rules": ["creation", "update", "deletion", "non_fast_forward"],
   "bypass_actors": [{"actor_id": 4260407, "actor_type": "Integration", "bypass_mode": "always"}]}}
JSON
  # Live settings that MATCH it. Arrays deliberately in a different order from the
  # baseline, and gh-style extra fields present, so normalisation is exercised.
  cat > "$dir/p.json" <<'JSON'
{"url": "x", "required_status_checks": {"strict": true, "contexts": ["lint", "rustfmt"],
   "checks": [{"context": "lint", "app_id": 15368}, {"context": "rustfmt", "app_id": 15368}]},
 "enforce_admins": {"url": "x", "enabled": true}}
JSON
  cat > "$dir/r.json" <<'JSON'
[{"id": 7, "name": "protect-release-tags", "target": "tag", "source_type": "Repository",
  "enforcement": "active", "node_id": "x",
  "conditions": {"ref_name": {"exclude": [], "include": ["~ALL"]}},
  "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}, {"type": "creation"}, {"type": "update"}],
  "bypass_actors": [{"actor_id": 4260407, "actor_type": "Integration", "bypass_mode": "always"}]}]
JSON
  # Today's live state: six checks, enforce_admins off, no rulesets.
  jq '.enforce_admins.enabled = false' "$dir/p.json" > "$dir/p-today.json"
  echo '[]' > "$dir/r-empty.json"

  local n=0 failures=0
  # case LABEL WANT_RC PATTERN [B_FILTER [P_FILTER [R_FILTER [P_FILE [R_FILE]]]]]
  # PATTERN: an extended regex that must match the output ("" = none). A pattern
  # starting with "!" must NOT match.
  case_() {
    local label="$1" want_rc="$2" pattern="$3" bf="${4:-.}" pf="${5:-.}" rf="${6:-.}"
    local pfile="${7:-$dir/p.json}" rfile="${8:-$dir/r.json}" out rc ok=1
    n=$((n + 1))
    jq "$bf" "$dir/b.json" > "$dir/case-b.json"
    jq "$pf" "$pfile" > "$dir/case-p.json"
    jq "$rf" "$rfile" > "$dir/case-r.json"
    set +e
    out="$(compare "$dir/case-b.json" "$dir/case-p.json" "$dir/case-r.json" 2>&1)"
    rc=$?
    set -e
    [ "$rc" -eq "$want_rc" ] || ok=0
    local p
    while IFS= read -r p; do
      [ -n "$p" ] || continue
      if [ "${p#!}" != "$p" ]; then
        ! printf '%s\n' "$out" | grep -Eq -- "${p#!}" || ok=0
      else
        printf '%s\n' "$out" | grep -Eq -- "$p" || ok=0
      fi
    done <<< "$pattern"
    if [ "$ok" -eq 1 ]; then
      echo "ok    $label"
    else
      echo "FAIL  $label (rc=$rc, want $want_rc; patterns: ${pattern//$'\n'/ | })"
      printf '%s\n' "$out" | sed 's/^/        /'
      failures=$((failures + 1))
    fi
  }
  local NL=$'\n'

  # Positive controls.
  case_ "matching settings pass, no findings at all" 0 "!::(error|warning|notice)::"
  case_ "matching settings without bypass_actors: notice, still green" 0 "^::notice::.*unverifiable${NL}!::(error|warning)::" \
    . . 'map(del(.bypass_actors))'
  case_ "pending_settings true + everything matching: nudge to flip the flag" 0 "^::notice::.*set it to false" \
    '.pending_settings = true'

  # Today's live state (plan Phase 2 AC1 / AC2).
  case_ "today, pending_settings false: RED on enforce_admins AND missing ruleset" 1 \
    "^::error::enforce_admins is false${NL}^::error::no tag ruleset named .protect-release-tags." \
    . . . "$dir/p-today.json" "$dir/r-empty.json"
  case_ "today, pending_settings true: green with warnings" 0 \
    "^::warning::enforce_admins is false${NL}^::warning::no tag ruleset${NL}!::error::" \
    '.pending_settings = true' . . "$dir/p-today.json" "$dir/r-empty.json"

  # Context / strict drift: hard even while pending.
  case_ "strict false is RED even while pending" 1 "^::error::required_status_checks.strict is false" \
    '.pending_settings = true' '.required_status_checks.strict = false'
  case_ "app_id mismatch is RED" 1 "^::error::required check .lint. is pinned to app_id 8329" \
    '.pending_settings = true' '.required_status_checks.checks[0].app_id = 8329'
  case_ "unpinned live check (app_id null) is RED" 1 "^::error::required check .lint. is pinned to app_id null" \
    . '.required_status_checks.checks[0].app_id = null'
  case_ "baseline check missing live is RED" 1 "^::error::required check .rustfmt. is in the baseline but NOT required" \
    . '.required_status_checks.checks |= map(select(.context != "rustfmt"))'
  case_ "unexpected live check is RED" 1 "^::error::.surprise. .app_id 15368. is required live but not in the baseline" \
    . '.required_status_checks.checks += [{"context": "surprise", "app_id": 15368}]'
  case_ "unexpected live check is RED even while pending" 1 "^::error::.surprise. .app_id 15368. is required live" \
    '.pending_settings = true' '.required_status_checks.checks += [{"context": "surprise", "app_id": 15368}]'
  case_ "advisory check already required, pending: warning only" 0 "^::warning::.msrv. is already required live" \
    '.pending_settings = true' '.required_status_checks.checks += [{"context": "msrv", "app_id": 15368}]'
  case_ "advisory check already required, not pending: RED" 1 "^::error::.msrv. .app_id 15368. is required live but not in the baseline" \
    . '.required_status_checks.checks += [{"context": "msrv", "app_id": 15368}]'
  case_ "no required_status_checks at all is RED" 1 "^::error::branch protection has NO required_status_checks" \
    '.pending_settings = true' 'del(.required_status_checks)'

  # Ruleset drift (plan Phase 2 AC3).
  case_ "enforcement evaluate is RED" 1 "^::error::tag ruleset .protect-release-tags. enforcement is \"evaluate\"" \
    . . '.[0].enforcement = "evaluate"'
  case_ "enforcement disabled while pending is a warning" 0 "^::warning::tag ruleset .protect-release-tags. enforcement is \"disabled\"" \
    '.pending_settings = true' . '.[0].enforcement = "disabled"'
  case_ "missing rule type is RED" 1 "^::error::.*missing rule type\\(s\\) \\[\"deletion\"\\]" \
    . . '.[0].rules |= map(select(.type != "deletion"))'
  case_ "extra rule type is RED" 1 "^::error::.*has rule type\\(s\\) \\[\"required_signatures\"\\]" \
    . . '.[0].rules += [{"type": "required_signatures"}]'
  case_ "narrowed include is RED" 1 "^::error::.*ref_name.include is \\[\"refs/tags/v\\*\"\\]" \
    . . '.[0].conditions.ref_name.include = ["refs/tags/v*"]'
  case_ "non-empty exclude is RED" 1 "^::error::.*ref_name.exclude is \\[\"refs/tags/archive/\\*\"\\]" \
    . . '.[0].conditions.ref_name.exclude = ["refs/tags/archive/*"]'
  case_ "extra bypass actor is RED" 1 "^::error::.*bypass_actors is" \
    . . '.[0].bypass_actors += [{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}]'
  case_ "bypass_mode pull_request instead of always is RED" 1 "^::error::.*bypass_actors is" \
    . . '.[0].bypass_actors[0].bypass_mode = "pull_request"'
  case_ "baseline mutated (wrong enforcement expected) is RED" 1 "^::error::.*enforcement is \"active\", baseline \"evaluate\"" \
    '.tag_ruleset.enforcement = "evaluate"'
  case_ "enforce_admins off is RED" 1 "^::error::enforce_admins is false.*POST" \
    . '.enforce_admins.enabled = false'
  case_ "renamed ruleset is missing (RED) and reported as additional" 1 "^::error::no tag ruleset named${NL}^::warning::additional tag ruleset .renamed." \
    . . '.[0].name = "renamed"'
  case_ "a second tag ruleset is reported but not fatal" 0 "^::warning::additional tag ruleset .other. \\(id 8\\)${NL}!::error::" \
    . . '. + [.[0] | .id = 8 | .name = "other"]'
  case_ "two tag rulesets with the baseline name are RED" 1 "^::error::2 tag rulesets are named" \
    . . '. + [.[0] | .id = 8]'
  case_ "a BRANCH ruleset with the name does not satisfy it" 1 "^::error::no tag ruleset named" \
    . . 'map(.target = "branch")'
  case_ "unrelated branch rulesets are ignored" 0 "!::(error|warning)::" \
    . . '. + [{"id": 9, "name": "branch-rules", "target": "branch", "enforcement": "active"}]'

  echo "self-test: ${n} case(s), ${failures} failure(s)"
  if [ "$n" -lt 20 ]; then
    echo "::error::self-test ran only ${n} cases; the harness is broken"
    return 1
  fi
  [ "$failures" -eq 0 ]
}

main() {
  local mode="${1:---fetch}"
  case "$mode" in
    --fetch) fetch ;;
    --compare)
      [ "$#" -eq 4 ] || { echo "usage: $0 --compare BASELINE PROTECTION RULESETS" >&2; exit 2; }
      compare "$2" "$3" "$4" ;;
    --self-test) self_test ;;
    *) echo "usage: $0 [--fetch | --compare BASELINE PROTECTION RULESETS | --self-test]" >&2; exit 2 ;;
  esac
}

main "$@"
