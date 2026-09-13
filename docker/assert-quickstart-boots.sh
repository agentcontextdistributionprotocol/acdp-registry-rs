#!/usr/bin/env bash
#
# Assert that the DOCUMENTED quickstart recipe actually boots (#266).
#
# ── Why this exists, and why the pre-existing smoke test did not cover it ──
#
# `.github/workflows/docker.yml` already boots a registry container and hits
# `/healthz`. That step hand-rolls `docker run` and mounts
# `docker/config.docker.toml` directly. It therefore never reads
# `docker/docker-compose.yml` at all -- and `docker compose up --build` from
# `docker/` is what README.md's "Production (Postgres + Docker)" section tells
# operators to run.
#
# That gap is not hypothetical. W3-U5 existed because the quickstart did not
# boot: `docker-compose.yml` shipped `ACDP_REGISTRY_AUTH__JWT_SECRET=changeme`,
# and `validate_config` rejects that placeholder by name
# (`crates/acdp-registry-server/src/main.rs:166`), so the stack exited rc=1
# before serving a request. The defect lived in the one file CI never opened,
# which is why users hit it first. Fixed in `abfebf7`; nothing guarded it.
#
# So this script drives the recipe through `docker compose`, using the compose
# file's own environment block and its own config mount. A CI step that
# reimplements the recipe proves things about the reimplementation.
#
# ── Modes ──
#
#   --check            boot the recipe exactly as shipped; must come up healthy.
#   --check-auth-on    boot it with auth enabled and a real generated secret;
#                      must come up healthy. `validate_config` gates a separate
#                      branch on `auth.enabled` (main.rs:108), so the shipped
#                      auth-off default exercises neither that branch nor the
#                      HS256 empty-secret refusal beside it.
#   --self-test        NEGATIVE CONTROLS. Each one asserts the stack REFUSES.
#                      Without these, `--check` passing means nothing: a boot
#                      check that cannot fail is decorative. Mirrors the
#                      `--self-test` convention in assert-image-tags.sh, wiring
#                      the falsification into CI instead of a PR body.
#
# Requires the image to already exist locally as `acdp-registry:smoke`; the
# overlay `compose.ci.yml` points the service at it so this does not trigger a
# second full release build.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE=(docker compose -f "$HERE/docker-compose.yml" -f "$HERE/compose.ci.yml" -p acdpqs)
# Auth-on stacks one more CI-only overlay. It is a SEPARATE variable rather than
# a flag on the recipe because the shipped compose file forwards only the
# variables named in its own `environment:` block -- so `ACDP_REGISTRY_AUTH__ENABLED`
# set in the caller's environment never reaches the container at all. An earlier
# draft of this script did exactly that and its auth-on check was decorative: it
# booted the auth-OFF stack and reported success. Only negative control (2)
# caught it. See compose.ci-auth-on.yml for why this is not fixed by editing the
# recipe.
AUTH_ON=("${BASE[@]}" -f "$HERE/compose.ci-auth-on.yml")
COMPOSE=("${BASE[@]}")
HEALTH_URL="http://localhost:8443/healthz"
BOOT_TIMEOUT=60

down() { "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true; }
trap down EXIT

# Bring the stack up and report whether the registry answers /healthz.
# Returns 0 = healthy, 1 = did not come up. Never aborts the script itself:
# the negative controls NEED the failure case to be observable rather than fatal.
# usage: boot [VAR=VAL ...] -- env is passed explicitly rather than exported,
# so each case states the whole configuration it is testing on one line and no
# value leaks between cases.
boot() {
    down
    if ! env "$@" "${COMPOSE[@]}" up -d >/tmp/acdpqs-up.log 2>&1; then
        echo "     compose up failed:"; sed 's/^/       /' /tmp/acdpqs-up.log
        return 1
    fi
    local waited=0
    while [ "$waited" -lt "$BOOT_TIMEOUT" ]; do
        if curl --fail --silent --show-error "$HEALTH_URL" -o /tmp/acdpqs-health.json 2>/dev/null; then
            return 0
        fi
        # A registry that has already exited will never become healthy; stop
        # waiting out the full timeout for a container that is gone.
        if [ "$("${COMPOSE[@]}" ps -q registry | wc -l)" -eq 0 ]; then return 1; fi
        local state
        state="$("${COMPOSE[@]}" ps --format '{{.State}}' registry 2>/dev/null | head -1)"
        if [ "$state" = "exited" ] || [ "$state" = "dead" ]; then return 1; fi
        sleep 2; waited=$((waited + 2))
    done
    return 1
}

registry_logs() { "${COMPOSE[@]}" logs --no-color registry 2>/dev/null || true; }

fail() { echo "::error::$1"; echo "--- registry logs ---"; registry_logs | sed 's/^/  /'; exit 1; }

case "${1:---check}" in

--check)
    echo "==> booting the documented quickstart recipe, exactly as shipped"
    boot || fail "the documented quickstart (docker compose up) did not come up healthy. \
This is the W3-U5 class of defect: the recipe the README tells operators to run does not boot."
    echo "    healthy: $(cat /tmp/acdpqs-health.json)"
    # Prove we booted the RECIPE, not some other stack: the compose file mounts
    # config.docker.toml, which ships storage.backend = "postgres". A stack that
    # silently fell back to sqlite would still answer /healthz.
    grep -q '"storage":true' /tmp/acdpqs-health.json \
        || fail "/healthz does not report storage:true -- the postgres wiring in the recipe is not live"
    echo "    OK"
    ;;

--check-auth-on)
    echo "==> booting the recipe with auth ENABLED and a real generated secret"
    # The secret arrives the way the compose file's own header documents it
    # (ACDP_REGISTRY_JWT_SECRET -> ACDP_REGISTRY_AUTH__JWT_SECRET), so this
    # exercises the documented override path rather than inventing one.
    # Honour a secret supplied by the caller (the workflow generates one, so
    # the #266 grep lands on real configuration there); fall back to generating
    # one so the script is runnable by hand.
    secret="${ACDP_REGISTRY_JWT_SECRET:-$(openssl rand -base64 32)}"
    COMPOSE=("${AUTH_ON[@]}")
    boot "ACDP_REGISTRY_JWT_SECRET=$secret" || fail "the recipe does not boot with auth enabled and a valid secret. \
validate_config's auth.enabled branch (main.rs:108) rejected a configuration that should be accepted."
    echo "    healthy: $(cat /tmp/acdpqs-health.json)"
    echo "    OK"
    ;;

--self-test)
    echo "==> NEGATIVE CONTROLS: the boot check must be able to fail"
    rc=0

    # (1) The literal W3-U5 regression. `changeme` is 8 base64 chars -> 6 bytes,
    #     below the 32-byte floor, and validate_config rejects it BY NAME. If
    #     this boots, the guard above is not reading the compose file's
    #     environment block, which is precisely where the original defect lived.
    echo "--> (1) replaying W3-U5: ACDP_REGISTRY_JWT_SECRET=changeme must refuse to boot"
    (
        if boot "ACDP_REGISTRY_JWT_SECRET=changeme"; then
            echo "::error::the stack booted with the 'changeme' placeholder; \
validate_config's literal guard (main.rs:166) is not reached through the compose recipe"
            exit 1
        fi
        registry_logs | grep -qi "changeme" \
            || { echo "::error::refused to boot, but not for the placeholder reason -- \
the negative control is passing for the wrong cause"; registry_logs | sed 's/^/    /'; exit 1; }
        echo "    correctly refused, and the log names the placeholder"
    ) || rc=1

    # (2) Auth on with an EMPTY secret must refuse (main.rs:129-142). Distinct
    #     from (1): that one is the ungated non-empty check, this one is the
    #     auth.enabled-gated empty check. One passing does not imply the other.
    echo "--> (2) auth enabled with an empty secret must refuse to boot"
    (
        COMPOSE=("${AUTH_ON[@]}")
        if boot "ACDP_REGISTRY_JWT_SECRET="; then
            echo "::error::the stack booted with auth enabled and no secret; \
the HS256 empty-secret refusal is not reached through the compose recipe"
            exit 1
        fi
        registry_logs | grep -qi "jwt_secret is empty\|allow_ephemeral_secret" \
            || { echo "::error::refused, but not for the empty-secret reason"; registry_logs | sed 's/^/    /'; exit 1; }
        echo "    correctly refused, and the log names the empty secret"
    ) || rc=1

    [ "$rc" -eq 0 ] || { echo "::error::negative controls failed"; exit 1; }
    echo "==> negative controls passed: the boot check discriminates"
    ;;

*)
    echo "usage: $0 [--check|--check-auth-on|--self-test]" >&2; exit 2 ;;
esac
