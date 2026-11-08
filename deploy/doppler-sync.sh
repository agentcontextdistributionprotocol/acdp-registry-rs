#!/usr/bin/env bash
#
# Sync a Railway service's variables from Doppler, the source of truth.
#
#   deploy/doppler-sync.sh            # dry run (default): key names and changed/unchanged, never values
#   deploy/doppler-sync.sh --apply    # push changed allowlisted keys from Doppler into Railway
#   deploy/doppler-sync.sh --apply --no-redeploy   # set them but leave the running deployment alone
#
# Only the keys listed in deploy/doppler-keys.txt are managed. The script sets
# and updates those; it never deletes a variable, never touches a key it does
# not list, and refuses RAILWAY_* system variables and Railway references
# (values containing "${{"), which stay Railway-managed.
#
# Values move by pipe only: Doppler -> jq -> Railway GraphQL request body on
# stdin. Nothing is printed, written to disk or passed in argv. The check is a
# sha256 of the sorted KEY=VALUE set of the managed keys, Doppler vs Railway.
# A redeploy happens only when at least one value actually changed.
#
# Needs git, jq, curl and the doppler CLI logged in, plus a Railway token: the
# railway CLI login (~/.railway/config.json) or RAILWAY_API_TOKEN (an account
# or workspace token; a project token cannot use these GraphQL calls).
# --apply refuses to run on a dirty tree, so the allowlist it applies is the
# committed one.
#
# To reuse in another repo: copy this file and doppler-keys.txt, then change
# the five settings below. Nothing else is repo-specific.

# Railway GraphQL queries are single-quoted on purpose.
# shellcheck disable=SC2016
set -euo pipefail

RAILWAY_PROJECT_ID=15502f9b-2cd4-4ea8-b495-1fadd01c80de # Railway: acdp-registry
RAILWAY_ENVIRONMENT=production
RAILWAY_SERVICE=acdp-registry
DOPPLER_PROJECT=acdp-registry
DOPPLER_CONFIG=prd

mode=dry-run
redeploy=1
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) mode=dry-run ;;
    --apply) mode=apply ;;
    --no-redeploy) redeploy=0 ;;
    -h|--help) sed -n '2,26p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

say() { printf '%s\n' "$*" >&2; }
die() { say "error: $*"; exit 1; }
for c in git jq curl doppler shasum; do command -v "$c" >/dev/null || die "$c is not on PATH"; done

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
keys_file="$here/doppler-keys.txt"

if [ "$mode" = apply ] && [ -n "$(git -C "$repo" status --porcelain)" ]; then
  die "the working tree is dirty; commit or stash first so --apply uses the committed allowlist"
fi

# The allowlist: one KEY per line; '#' comments and blank lines are ignored.
keys=()
while IFS= read -r line; do
  k="${line%%#*}"
  k="$(printf '%s' "$k" | tr -d '[:space:]')"
  [ -n "$k" ] || continue
  [[ "$k" =~ ^[A-Z_][A-Z0-9_]*$ ]] || die "doppler-keys.txt: '$k' is not a valid variable name"
  [[ "$k" == RAILWAY_* ]] && die "doppler-keys.txt: $k is a Railway system variable; Railway owns it"
  keys+=("$k")
done <"$keys_file"
[ "${#keys[@]}" -gt 0 ] || die "doppler-keys.txt lists no keys"
keys_json="$(printf '%s\n' "${keys[@]}" | jq -R . | jq -sc 'unique')"

railway_token="${RAILWAY_API_TOKEN:-$(jq -r '.user.accessToken // .user.token // empty' ~/.railway/config.json 2>/dev/null || true)}"
[ -n "$railway_token" ] || die "no Railway token: log in with the railway CLI or set RAILWAY_API_TOKEN"

# POST a GraphQL request whose JSON body arrives on stdin, so values never reach argv.
# The token travels as a header read from a file descriptor (printf is a shell
# builtin), so it is never in curl's argv either, where `ps` would show it.
#
# Fails closed. A bad or expired token makes Railway answer HTTP 200 with
# `data: null` and an `errors` array; read as "no variables", that would list
# every key as new and --apply would rewrite them all. So this exits non-zero on
# a network error or non-2xx (curl -f), an unparsable body, a non-empty
# `errors`, or a null or missing `data`. Each function below is ONE pipeline on
# purpose: under `x="$(f)" || die`, bash disables `set -e` inside f, so only the
# exit status of f's last pipeline (with pipefail) carries a failure out.
railway_gql() {
  curl -fsS https://backboard.railway.com/graphql/v2 \
    -H @<(printf 'Authorization: Bearer %s\n' "$railway_token") -H 'content-type: application/json' --data-binary @- |
    jq -e '
      if type != "object" then error("railway: the response is not a JSON object")
      elif (.errors // null) != null and .errors != [] then
        error("railway: \(.errors | if type == "array" then map(.message? // "unknown error") | join("; ") else "unknown error" end)")
      elif (.data // null) == null then error("railway: the response has no data")
      else .data end'
}
gql() { jq -nc --arg q "$1" --argjson v "${2:-{\}}" '{query: $q, variables: $v}' | railway_gql; }

ids="$(gql 'query($p: String!){project(id: $p){environments{edges{node{id name}}} services{edges{node{id name}}}}}' \
  "$(jq -nc --arg p "$RAILWAY_PROJECT_ID" '{p: $p}')")" || die "reading the Railway project failed; nothing changed"
env_id="$(jq -r --arg n "$RAILWAY_ENVIRONMENT" '.project.environments.edges[].node | select(.name == $n) | .id' <<<"$ids")" ||
  die "the Railway project response is malformed; nothing changed"
svc_id="$(jq -r --arg n "$RAILWAY_SERVICE" '.project.services.edges[].node | select(.name == $n) | .id' <<<"$ids")" ||
  die "the Railway project response is malformed; nothing changed"
[ -n "$env_id" ] || die "Railway environment $RAILWAY_ENVIRONMENT not found in project $RAILWAY_PROJECT_ID"
[ -n "$svc_id" ] || die "Railway service $RAILWAY_SERVICE not found in project $RAILWAY_PROJECT_ID"
scope="$(jq -nc --arg p "$RAILWAY_PROJECT_ID" --arg e "$env_id" --arg s "$svc_id" '{p: $p, e: $e, s: $s}')"

# Raw (unrendered) service variables, so a reference shows up as ${{...}}, not its resolved value.
railway_vars() {
  gql 'query($p: String!, $e: String!, $s: String!){variables(projectId: $p, environmentId: $e, serviceId: $s, unrendered: true)}' \
    "$scope" | jq -ce '.variables | if type == "object" then . else error("railway: variables is not an object") end'
}
doppler_vars() {
  doppler secrets download --project "$DOPPLER_PROJECT" --config "$DOPPLER_CONFIG" --no-file --format json |
    jq -ce 'if type == "object" then . else error("doppler: the download is not a JSON object") end'
}

# sha256 of the sorted KEY=VALUE lines of the managed keys (a missing key hashes as absent).
managed_hash() {
  jq -r --argjson k "$keys_json" '. as $v | $k[] as $key | select($v | has($key)) | "\($key)=\($v[$key])"' | LC_ALL=C sort | shasum -a 256 | cut -c1-16
}

rv="$(railway_vars)" || die "reading the Railway variables failed; nothing changed"
dv="$(doppler_vars)" || die "reading the Doppler secrets failed; nothing changed"

# Per key: status only. The comparison happens inside jq; values never leave it.
plan="$(printf '%s\n%s\n' "$dv" "$rv" | jq -sc --argjson k "$keys_json" '
  .[0] as $d | .[1] as $r |
  [$k[] | . as $key | {key: $key, status: (
    if ($d | has($key) | not) then "missing-in-doppler"
    elif ($d[$key] | tostring | contains("${{")) then "doppler-value-is-a-reference"
    elif ($r | has($key)) and ($r[$key] | contains("${{")) then "railway-reference"
    elif ($r | has($key) | not) then "new"
    elif $r[$key] == $d[$key] then "unchanged"
    else "changed" end)}]')" || die "comparing Doppler with Railway failed; nothing changed"

say "Doppler $DOPPLER_PROJECT/$DOPPLER_CONFIG -> Railway $RAILWAY_SERVICE ($RAILWAY_ENVIRONMENT), ${#keys[@]} managed key(s):"
jq -r '.[] | "  \(.key)\t\(.status)"' <<<"$plan" >&2 || die "printing the plan failed; nothing changed"
unmanaged="$(jq -r --argjson k "$keys_json" 'keys - $k | join(" ")' <<<"$rv")" || die "listing unmanaged keys failed; nothing changed"
[ -z "$unmanaged" ] || say "  left alone (not in doppler-keys.txt): $unmanaged"

bad="$(jq -r '[.[] | select(.status | IN("missing-in-doppler", "doppler-value-is-a-reference", "railway-reference")) | .key] | join(" ")' <<<"$plan")" || die "reading the plan failed; nothing changed"
[ -z "$bad" ] || die "refusing: $bad (see status above; a Railway reference must stay Railway-managed, so drop it from doppler-keys.txt)"
todo="$(jq -r '.[] | select(.status | IN("new", "changed")) | .key' <<<"$plan")" || die "reading the plan failed; nothing changed"

before="$(managed_hash <<<"$rv")" || die "hashing the Railway variables failed; nothing changed"
want="$(managed_hash <<<"$dv")" || die "hashing the Doppler secrets failed; nothing changed"
say "hash: railway $before, doppler $want ($([ "$before" = "$want" ] && echo match || echo differ))"

if [ "$mode" = dry-run ]; then
  if [ -n "$todo" ]; then say "dry run: $(wc -w <<<"$todo" | tr -d ' ') key(s) would change; rerun with --apply"; else say "dry run: nothing to change"; fi
  exit 0
fi

if [ -z "$todo" ]; then
  say "apply: nothing to change; no redeploy"
  exit 0
fi

for key in $todo; do
  # The value goes Doppler -> jq -> curl stdin; it is never in argv or on disk.
  doppler_vars | jq -c --arg key "$key" --argjson s "$scope" '{
      query: "mutation($i: VariableUpsertInput!){variableUpsert(input: $i)}",
      variables: {i: {projectId: $s.p, environmentId: $s.e, serviceId: $s.s, name: $key, value: .[$key], skipDeploys: true}}}' |
    railway_gql >/dev/null || die "setting $key failed; keys before it were set, no redeploy; rerun to finish"
  say "  set $key"
done

after="$(railway_vars | managed_hash)" || die "reading the Railway variables after apply failed; no redeploy"
[ "$after" = "$want" ] || die "hash after apply is $after, expected $want; check the service in Railway"
say "hash after apply: $after (match)"

if [ "$redeploy" = 1 ]; then
  gql 'mutation($e: String!, $s: String!){serviceInstanceRedeploy(environmentId: $e, serviceId: $s)}' "$scope" >/dev/null || die "the redeploy request failed; the values are set and apply on the next deploy"
  say "redeploy triggered for $RAILWAY_SERVICE so the new values take effect"
else
  say "no redeploy (--no-redeploy): the values apply on the next deploy"
fi
