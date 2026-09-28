#!/usr/bin/env bash
set -euo pipefail

RELEASE_TMPDIR=""
RELEASE_PIDS=()
RELEASE_CLEANUP_HOOK=""

release_init() {
    RELEASE_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp}/north-release.XXXXXX")
    mkdir -p "$RELEASE_TMPDIR/home" "$RELEASE_TMPDIR/config" "$RELEASE_TMPDIR/state" "$RELEASE_TMPDIR/logs"
    export HOME="$RELEASE_TMPDIR/home"
    export XDG_CONFIG_HOME="$RELEASE_TMPDIR/config"
    export XDG_STATE_HOME="$RELEASE_TMPDIR/state"
    trap release_cleanup EXIT INT TERM
    if [[ "${NORTH_RELEASE_NSS_CA_TRUST:-false}" == true ]]; then
        release_require_env NORTH_RELEASE_TLS_DIR || return $?
        release_require_tool certutil || return $?
        local nssdb="$HOME/.pki/nssdb"
        [[ -r "$NORTH_RELEASE_TLS_DIR/ca.crt" ]] || {
            release_log "OWNER-ACTION TLS CA certificate missing for temporary Chromium trust"
            return 2
        }
        mkdir -p "$nssdb"
        certutil -N -d "sql:$nssdb" --empty-password >/dev/null 2>&1 || {
            release_log "OWNER-ACTION unable to initialize temporary Chromium trust store"
            return 2
        }
        certutil -A -d "sql:$nssdb" -n north-release-qualification-ca \
            -t "C,," -i "$NORTH_RELEASE_TLS_DIR/ca.crt" >/dev/null || {
            release_log "OWNER-ACTION unable to import TLS CA into temporary Chromium trust store"
            return 2
        }
        release_log "PASS temporary-browser-ca-trust"
    fi
}

release_track_pid() {
    RELEASE_PIDS+=("$1")
}

release_cleanup() {
local status=$?
if [[ -n "$RELEASE_CLEANUP_HOOK" ]]; then
        "$RELEASE_CLEANUP_HOOK" || true
    fi
    trap - EXIT INT TERM
    for pid in "${RELEASE_PIDS[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            kill TERM "$pid" 2>/dev/null || true
        fi
    done
    for pid in "${RELEASE_PIDS[@]}"; do
        wait "$pid" 2>/dev/null || true
    done
    if [[ -n "$RELEASE_TMPDIR" ]]; then
        rm -rf "$RELEASE_TMPDIR"
    fi
    exit "$status"
}

release_require_tool() {
    command -v "$1" >/dev/null 2>&1 || {
        release_log "OWNER-ACTION required tool missing: $1"
        return 2
    }
}

release_require_env() {
    [[ -n "${!1:-}" ]] || {
        release_log "OWNER-ACTION required environment variable missing: $1"
        return 2
    }
}

release_log() {
    printf '[release] %s\n' "$*"
}
