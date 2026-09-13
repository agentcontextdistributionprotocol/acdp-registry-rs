#!/usr/bin/env bash
#
# Assert the one-writer invariant for the container image's main-line tags.
#
#   A `sha-`-prefixed tag is present IF AND ONLY IF this is a push to the
#   default branch.
#
# Why this exists. `.github/workflows/docker.yml` publishes on a push to `main`
# AND on an `acdp-registry-server/v*` tag, and release-plz tags the same commit
# it merges (release-plz.toml's `git_tag_name`), so one commit gets two
# publishing runs. The `tags:` rules are shared by every trigger, so an ungated
# `type=sha` made BOTH runs push `sha-<short>` for the same commit — the second
# silently re-pointing it. Measured on v0.1.3 (commit f8b6d9e):
#
#   run 34734028111  push refs/heads/main                       02:52:16Z
#                    ["main","latest","sha-f8b6d9e"]  -> sha256:b9315cc84f08…
#   run 34734871991  push refs/tags/acdp-registry-server/v0.1.3 03:12:00Z
#                    ["0.1.3","0.1","sha-f8b6d9e"]    -> sha256:cf2f85068eb6…
#
# 19m44s apart, two different digests, one tag. A `sha-`-prefixed tag reads as
# content-addressed and was not: anyone who pinned it inside that window is
# running different bytes than they pinned.
#
# Why the assertion is an IFF rather than "no sha- on tag pushes". An iff fires
# in both directions, and — the part that gives it teeth — it fires ON A PULL
# REQUEST. A PR is not the default branch, so if anyone deletes the
# `enable={{is_default_branch}}` gate in docker.yml, the PR that deletes it
# computes a `sha-` tag and this assertion fails THERE, before merge. The
# one-directional form would stay silent until the next release. The reverse
# direction matters too: if the gate is ever written so tightly that a push to
# main stops publishing `sha-<short>` at all, this fails rather than quietly
# dropping a documented tag.
#
# What this does NOT claim: GHCR tags are still mutable at the registry. This
# reduces the number of workflow paths that write `sha-<short>` from two to one.
# A manual re-run of a main-push job will rebuild that commit and re-point its
# `sha-` tag. Registry-level immutability would need a pre-push existence check,
# which is a separate decision with a real operational cost.
#
# Usage:
#   assert-image-tags.sh --check --tags <list> --event <name> --ref <ref> \
#                        [--default-branch <branch>]
#   assert-image-tags.sh --self-test
#
# <list> is the set of tags being published: newline-, comma- or space-separated,
# either bare tag names (`0.1.3`) or full refs (`ghcr.io/org/img:0.1.3`), with
# JSON-array punctuation tolerated. The workflow passes
# `${{ steps.meta.outputs.tags }}`, which is the exact value handed to
# docker/build-push-action — so this asserts on what is actually pushed rather
# than on a parallel representation of it. No jq dependency, deliberately: this
# is the first shell script in the repo, so it depends on as little as possible.
# (It was written when CI ran no shellcheck at all; `.github/workflows/lint.yml`
# now lints it on every PR, pinned to the shellcheck version this was checked
# against. The minimal-dependency posture stays — it is still the only script
# here, and it runs in the container publish path.)

set -euo pipefail

readonly SHA_TAG_PREFIX='sha-'

# Normalise a whitespace/comma/JSON-punctuated list of tags-or-refs into one
# bare tag name per line. A token containing `/` is a full image ref, so the tag
# is what follows its LAST colon (which also survives a registry:port).
# The `|| [ -n "$token" ]` is load-bearing: `read` returns false on a final line
# with no trailing newline, so a plain `while read` silently DROPS the last
# token. `steps.meta.outputs.tags` may or may not end in a newline, and a guard
# that quietly loses the last tag in its input is worse than no guard — the
# self-test below caught exactly this while it was being written.
# shellcheck disable=SC2020  # deliberate, and narrowed to this function only.
# SC2020 warns against expecting `tr` to replace WORDS. This maps three
# separator CHARACTERS (comma, space, tab) each to a newline, which is what
# `tr` is for; the note fires only because set2's newlines repeat. Equalising
# the set lengths does not silence it (the duplicates are the point), and the
# alternatives are worse: unquoted parameter expansion trades this for SC2086
# plus glob exposure needing `set -f`, and `sed` diverges between BSD and GNU
# on `\n` in the replacement. The directive cannot sit on the offending line
# itself, because a `#` between backslash-continued lines is part of the
# command, not a comment — hence function scope, one code, with this reason.
normalise_tags() {
    local raw="$1"
    echo TEMPORARY falsification probe $raw > /dev/null
    printf '%s\n' "$raw" \
        | tr -d '"[]' \
        | tr ', \t' '\n\n\n\n' \
        | while IFS= read -r token || [ -n "$token" ]; do
            [ -n "$token" ] || continue
            case "$token" in
                */*) printf '%s\n' "${token##*:}" ;;
                *)   printf '%s\n' "$token" ;;
            esac
          done
}

# The invariant. Returns 0 when it holds, 1 when it does not, and explains
# itself on stderr either way — a green job that printed nothing is how the
# original defect stayed invisible through two successful runs.
check_tags() {
    local raw_tags="$1" event="$2" ref="$3" default_branch="${4:-main}"

    local tags
    tags="$(normalise_tags "$raw_tags")"

    if [ -z "$tags" ]; then
        echo "::error::no image tags were computed for ${event} ${ref}." \
             "An empty tag set is a metadata-action failure, not a pass." >&2
        return 1
    fi

    local sha_tags
    sha_tags="$(printf '%s\n' "$tags" | grep "^${SHA_TAG_PREFIX}" || true)"

    local is_default_push=false
    if [ "$event" = push ] && [ "$ref" = "refs/heads/${default_branch}" ]; then
        is_default_push=true
    fi

    if [ "$is_default_push" = true ] && [ -z "$sha_tags" ]; then
        echo "::error::${ref} is a push to the default branch (${default_branch})" \
             "but no ${SHA_TAG_PREFIX}* tag was computed. The gate on the sha rule" \
             "is over-firing and a documented tag has silently stopped being" \
             "published. Tags: $(printf '%s' "$tags" | tr '\n' ' ')" >&2
        return 1
    fi

    if [ "$is_default_push" != true ] && [ -n "$sha_tags" ]; then
        echo "::error::${event} ${ref} is not a push to the default branch" \
             "(${default_branch}) yet it computed $(printf '%s' "$sha_tags" | tr '\n' ' ')." \
             "Two paths publishing one ${SHA_TAG_PREFIX}* tag is what let" \
             "sha-f8b6d9e be re-pointed to a second digest 19m44s after it was" \
             "first published. Gate the sha rule to the default branch." >&2
        return 1
    fi

    if [ "$is_default_push" = true ]; then
        echo "ok: ${ref} publishes" \
             "$(printf '%s' "$sha_tags" | tr '\n' ' ')" "as its single writer" >&2
    else
        echo "ok: ${event} ${ref} computes no ${SHA_TAG_PREFIX}* tag, so it cannot" \
             "re-point one" >&2
    fi
    return 0
}

# The self-test is this script's falsification, kept executable rather than
# claimed in a PR body. Three of its cases are the REAL tag sets from the runs
# quoted at the top of this file: the guard must reject the exact bytes that
# caused the incident, not a hand-made approximation of them. The workflow runs
# this on every event, so the falsification re-runs forever instead of being a
# one-time assertion by whoever wrote it.
self_test() {
    local failures=0 case_no=0 rejects=0 real_rejects=0

    # expect | tags | event | ref | default-branch | description
    local cases=(
        'pass|main,latest,sha-f8b6d9e|push|refs/heads/main|main|REAL pre-fix main push (run 34734028111) — legitimate sole writer'
        'fail|0.1.3,0.1,sha-f8b6d9e|push|refs/tags/acdp-registry-server/v0.1.3|main|REAL pre-fix release push (run 34734871991) — the second writer that re-pointed the tag'
        'fail|pr-262,sha-3617f76|pull_request|refs/pull/262/merge|main|REAL pre-fix PR run (run 34725795501) — ungated sha rule, visible in-PR'
        'pass|0.1.3,0.1|push|refs/tags/acdp-registry-server/v0.1.3|main|post-fix release push — version tags only'
        'pass|pr-262|pull_request|refs/pull/262/merge|main|post-fix PR run — no sha tag computed'
        'fail|main,latest|push|refs/heads/main|main|gate over-fired: default-branch push stopped publishing a documented tag'
        'fail||push|refs/heads/main|main|empty tag set is a metadata-action failure, not a pass'
        'pass|develop,sha-deadbee|push|refs/heads/develop|develop|the default branch is a parameter, not a hardcoded "main"'
        'fail|main,latest,sha256sum|push|refs/heads/main|main|a tag merely containing "sha" is not a sha- tag, so this main push is missing its real one'
        'pass|ghcr.io/org/acdp-registry:main,ghcr.io/org/acdp-registry:sha-f8b6d9e|push|refs/heads/main|main|full image refs, as steps.meta.outputs.tags actually emits them'
    )

    local spec expect tags event ref branch desc rc verdict
    for spec in "${cases[@]}"; do
        case_no=$((case_no + 1))
        IFS='|' read -r expect tags event ref branch desc <<<"$spec"

        rc=0
        check_tags "$tags" "$event" "$ref" "$branch" >/dev/null 2>&1 || rc=$?

        if [ "$expect" = pass ]; then
            [ "$rc" -eq 0 ] && verdict=ok || verdict=BROKEN
        else
            [ "$rc" -ne 0 ] && verdict=ok || verdict=BROKEN
            rejects=$((rejects + 1))
            case "$desc" in REAL*) real_rejects=$((real_rejects + 1)) ;; esac
        fi

        printf '%-7s case %2d  expect=%-4s rc=%d  %s\n' \
            "$verdict" "$case_no" "$expect" "$rc" "$desc"
        [ "$verdict" = ok ] || failures=$((failures + 1))
    done

    if [ "$failures" -ne 0 ]; then
        echo "::error::self-test: ${failures} of ${case_no} cases behaved unexpectedly." \
             "This guard is not trustworthy until they pass." >&2
        return 1
    fi
    # Counts are derived from the table, never hand-maintained: a literal here
    # would go stale the first time a case is added, and this line is the one a
    # reader quotes.
    echo "self-test: all ${case_no} cases behaved as expected" \
         "(${rejects} of them assert the guard REJECTS," \
         "${real_rejects} of those on real incident data)"
    return 0
}

usage() {
    sed -n '/^# Usage:/,/^# this is the first/p' "$0" | sed 's/^# \{0,1\}//'
}

main() {
    local mode='' tags='' event='' ref='' default_branch='main'

    while [ $# -gt 0 ]; do
        case "$1" in
            --check)          mode=check ;;
            --self-test)      mode=self-test ;;
            --tags)           tags="${2-}"; shift ;;
            --event)          event="${2-}"; shift ;;
            --ref)            ref="${2-}"; shift ;;
            --default-branch) default_branch="${2-}"; shift ;;
            -h|--help)        usage; return 0 ;;
            *) echo "unknown argument: $1" >&2; usage >&2; return 2 ;;
        esac
        shift
    done

    case "$mode" in
        self-test) self_test ;;
        check)
            if [ -z "$event" ] || [ -z "$ref" ]; then
                echo "--check needs --event and --ref" >&2
                return 2
            fi
            check_tags "$tags" "$event" "$ref" "$default_branch"
            ;;
        *) echo "pick a mode: --check or --self-test" >&2; usage >&2; return 2 ;;
    esac
}

main "$@"
