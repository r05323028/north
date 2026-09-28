#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
readonly LINUX_RELEASE_BUILDER_IMAGE="rust:1.97.1-bullseye@sha256:02d78ca3f928195c2a907543de778adfd728ad7e2a24fdc6aef582b7c77842e0"
readonly GLIBC_BASELINE=2.31

release_require_clean_source() {
    local command=$1 stage=$2 source_commit=$3 status
    local current_commit
    current_commit=$(git -C "$ROOT" rev-parse HEAD) || {
        printf 'release.sh %s: unable to verify source commit after %s\n' "$command" "$stage" >&2
        return 2
    }
    [[ "$current_commit" == "$source_commit" ]] || {
        printf 'release.sh %s: HEAD changed after %s; refusing to label artifacts as commit %s\n' "$command" "$stage" "$source_commit" >&2
        return 2
    }
    status=$(git -C "$ROOT" status --porcelain --untracked-files=all) || {
        printf 'release.sh %s: unable to verify source worktree cleanliness after %s\n' "$command" "$stage" >&2
        return 2
    }
    [[ -z "$status" ]] || {
        printf 'release.sh %s: source worktree changed after %s; refusing to label mutable inputs as commit %s\n%s\n' \
            "$command" "$stage" "$source_commit" "$status" >&2
        return 2
    }
}

release_cargo_build() {
    local target=$1 binaries=$2 cargo_cache
    shift 2
    if [[ "$target" == x86_64-unknown-linux-gnu ]]; then
        command -v docker >/dev/null 2>&1 || {
            printf 'release.sh: Docker is required for pinned Linux release builds\n' >&2
            return 2
        }
        cargo_cache=${CARGO_HOME:-${HOME:?}/.cargo}
        mkdir -p "$cargo_cache"
        docker run --rm --platform=linux/amd64 \
            --user "$(id -u):$(id -g)" \
            --volume "$ROOT:/workspace" \
            --volume "$cargo_cache:/cargo" \
            --workdir /workspace \
            --env CARGO_HOME=/cargo \
            --env RUSTUP_HOME=/usr/local/rustup \
            "$LINUX_RELEASE_BUILDER_IMAGE" \
            bash -euo pipefail -c '
                builder_libc=$(getconf GNU_LIBC_VERSION)
                [[ "$builder_libc" == "glibc 2.31" ]] || {
                    printf "release Linux builder requires glibc 2.31; found %s\n" "$builder_libc" >&2
                    exit 2
                }
                target=$1
                binary_names=$2
                shift 2
                cargo build --locked --release --target "$target" "$@"
                read -r -a binaries <<< "$binary_names"
                for binary in "${binaries[@]}"; do
                    [[ -x "target/$target/release/$binary" ]] || {
                        printf "release Linux build did not produce %s\n" "$binary" >&2
                        exit 2
                    }
                done
            ' north-release-build "$target" "$binaries" "$@"
    else
        cargo build --locked --release --target "$target" "$@"
    fi
}

usage() {
    printf 'usage: %s <package|cli-package|qualify> [version|target|artifact-dir]\n' "${0##*/}" >&2
}

release_package() {
requested_version=${1:-${NORTH_RELEASE_VERSION:-}}
target=${NORTH_RELEASE_TARGET:-x86_64-unknown-linux-gnu}
glibc_baseline=$GLIBC_BASELINE
node_requirement=${NORTH_RELEASE_NODE_REQUIREMENT:-22}

cargo_version=$(cargo metadata --locked --no-deps --format-version 1 | node -e '
let input=""; process.stdin.on("data", chunk => input += chunk); process.stdin.on("end", () => {
  const metadata = JSON.parse(input);
  const pkg = metadata.packages.find(({ name }) => name === "north-server");
  if (!pkg) process.exit(1);
  process.stdout.write(pkg.version);
});')
web_version=$(node -p "require('./apps/web/package.json').version")
[[ -n "$requested_version" ]] || requested_version=$cargo_version
version=${requested_version#v}
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || {
    printf 'release.sh package: invalid version: %s\n' "$requested_version" >&2
    exit 2
}
[[ "$cargo_version" == "$version" && "$web_version" == "$version" ]] || {
    printf 'release.sh package: version mismatch server=%s web=%s requested=%s\n' "$cargo_version" "$web_version" "$version" >&2
    exit 2
}

source_commit=$(git rev-parse HEAD)
source_status=$(git status --porcelain --untracked-files=all) || {
    printf 'release.sh package: unable to verify source worktree cleanliness\n' >&2
    exit 2
}
[[ -z "$source_status" ]] || {
    printf 'release.sh package: worktree is not clean; refusing to label mutable inputs as commit %s\n' "$source_commit" >&2
    exit 2
}
expected_source_commit=${NORTH_RELEASE_SOURCE_SHA:-${GITHUB_SHA:-}}
if [[ -n "$expected_source_commit" && "$source_commit" != "$expected_source_commit" ]]; then
    printf 'release.sh package: HEAD %s does not match expected source SHA %s\n' \
        "$source_commit" "$expected_source_commit" >&2
    exit 2
fi
if ! parent_commit=$(git rev-parse "$source_commit^1" 2>/dev/null); then
    printf 'release.sh package: first parent unavailable; fetch complete history\n' >&2
    exit 2
fi
previous_cargo_version=$(git show "$parent_commit:Cargo.toml" | awk '
    /^\[workspace\.package\][[:space:]]*$/ { in_workspace = 1; next }
    /^\[/ { in_workspace = 0 }
    in_workspace && /^[[:space:]]*version[[:space:]]*=/ {
        value = $3
        gsub(/"/, "", value)
        print value
        exit
    }
')
previous_web_version=$(git show "$parent_commit:apps/web/package.json" | node -e '
let input = "";
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => process.stdout.write(JSON.parse(input).version));
')
[[ "$previous_cargo_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ && "$previous_web_version" == "$previous_cargo_version" ]] || {
    printf 'release.sh package: first-parent server/web versions are missing or mismatched\n' >&2
    exit 2
}
version_changed=false
[[ "$version" == "$previous_cargo_version" ]] || version_changed=true
output_root=${NORTH_RELEASE_OUTPUT_DIR:-$ROOT/dist/north-v$version}
printf 'release.sh package: source=%s version=%s previous_version=%s version_changed=%s\n' \
    "$source_commit" "$version" "$previous_cargo_version" "$version_changed"

if ! rustup target list --installed | grep -Fx "$target" >/dev/null; then
    printf 'release.sh package: Rust target is not installed: %s\n' "$target" >&2
    exit 2
fi
release_cargo_build "$target" "north-server north-daemon" -p north-server -p north-daemon
release_require_clean_source package "Cargo build" "$source_commit"
(cd apps/web && npm run build)
release_require_clean_source package "web build" "$source_commit"

rm -rf "$output_root"
mkdir -p "$output_root/bin" "$output_root/web" "$output_root/docs"

target_release_dir="target/$target/release"
install -m 0755 "$target_release_dir/north-server" "$output_root/bin/north-server"
install -m 0755 "$target_release_dir/north-daemon" "$output_root/bin/north-daemon"
cp -R apps/web/.next/standalone/. "$output_root/web/"
mkdir -p "$output_root/web/.next"
cp -R apps/web/.next/static "$output_root/web/.next/static"
if [[ -d apps/web/public ]]; then cp -R apps/web/public "$output_root/web/public"; fi
cp README.md "$output_root/docs/README.md"
if [[ -d docs ]]; then cp -R docs/. "$output_root/docs/"; fi
if find "$output_root" ! -type f ! -type d -print -quit | grep -q .; then
    printf 'release.sh package: non-regular release entry is not allowed\n' >&2
    exit 2
fi

node - "$output_root/manifest.json" "$version" "$source_commit" "$target" "$glibc_baseline" "$node_requirement" "$previous_cargo_version" "$version_changed" <<'NODE'
const fs = require("node:fs");
const [path, version, sourceCommit, target, glibcBaseline, nodeRequirement, previousVersion, versionChanged] = process.argv.slice(2);
fs.writeFileSync(path, JSON.stringify({
  version,
  source_commit: sourceCommit,
  previous_version: previousVersion,
  version_changed: versionChanged === "true",
  target,
  glibc_baseline: glibcBaseline,
  node_major: nodeRequirement,
  server_version: version,
  daemon_version: version,
  web_version: version,
}, null, 2) + "\n");
NODE

(
    cd "$output_root"
    payload_list=$(mktemp)
    find . -type f ! -name checksums.sha256 ! -name checksums.sha256.tmp -print0 \
        | sort -z > "$payload_list"
    if ! xargs -0 shasum -a 256 < "$payload_list" > checksums.sha256.tmp; then
        rm -f "$payload_list" checksums.sha256.tmp
        exit 1
    fi
    rm -f "$payload_list"
    mv checksums.sha256.tmp checksums.sha256
)
printf 'release.sh package: %s\n' "$output_root"
(cd "$output_root" && shasum -a 256 -c checksums.sha256 >/dev/null)
node "$ROOT/scripts/verify-release-artifact.mjs" \
    "$output_root" "$source_commit" "v$version"
release_require_clean_source package "package assembly" "$source_commit"
}

release_cli_package() {
    [[ $# -eq 1 ]] || { usage; exit 2; }
    target=$1
    case "$target:$(uname -s):$(uname -m)" in
        x86_64-unknown-linux-gnu:Linux:x86_64 | \
        x86_64-apple-darwin:Darwin:x86_64 | \
        aarch64-apple-darwin:Darwin:arm64) ;;
        *)
            printf 'release.sh cli-package: target must match native release runner: %s\n' "$target" >&2
            exit 2
            ;;
    esac
    cargo_version=$(cargo metadata --locked --no-deps --format-version 1 | node -e '
let input = "";
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => {
  const pkg = JSON.parse(input).packages.find(({ name }) => name === "north-daemon");
  if (!pkg) process.exit(1);
  process.stdout.write(pkg.version);
});')
    web_version=$(node -p "require('./apps/web/package.json').version")
    [[ "$cargo_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ && "$web_version" == "$cargo_version" ]] || {
        printf 'release.sh cli-package: Cargo/web versions are invalid or mismatched\n' >&2
        exit 2
    }
    source_commit=$(git rev-parse HEAD)
    source_status=$(git status --porcelain --untracked-files=all) || {
        printf 'release.sh cli-package: unable to verify source worktree cleanliness\n' >&2
        exit 2
    }
    [[ -z "$source_status" ]] || {
        printf 'release.sh cli-package: worktree is not clean\n' >&2
        exit 2
    }
    expected_source_commit=${NORTH_RELEASE_SOURCE_SHA:-${GITHUB_SHA:-}}
    if [[ -n "$expected_source_commit" && "$source_commit" != "$expected_source_commit" ]]; then
        printf 'release.sh cli-package: HEAD does not match expected source SHA\n' >&2
        exit 2
    fi
    if ! rustup target list --installed | grep -Fx "$target" >/dev/null; then
        printf 'release.sh cli-package: Rust target is not installed: %s\n' "$target" >&2
        exit 2
    fi

    release_cargo_build "$target" "north north-daemon" -p north-daemon --bin north --bin north-daemon
    release_require_clean_source cli-package "Cargo build" "$source_commit"
    stage_root=$(mktemp -d)
    trap 'rm -rf "$stage_root"' EXIT
    stage="$stage_root/package"
    mkdir -p "$stage"
    target_dir="$ROOT/target/$target/release"
    install -m 0755 "$target_dir/north" "$stage/north"
    install -m 0755 "$target_dir/north-daemon" "$stage/north-daemon"
    node - "$stage/manifest.json" "$cargo_version" "$source_commit" "$target" "$GLIBC_BASELINE" <<'NODE'
const fs = require("node:fs");
const [path, version, sourceCommit, target, glibcBaseline] = process.argv.slice(2);
const platform = {
  "x86_64-unknown-linux-gnu": "linux/amd64",
  "x86_64-apple-darwin": "darwin/amd64",
  "aarch64-apple-darwin": "darwin/arm64",
}[target];
if (!platform) process.exit(1);
fs.writeFileSync(path, JSON.stringify({
  version,
  source_commit: sourceCommit,
  target,
  platform,
  glibc_baseline: target === "x86_64-unknown-linux-gnu" ? glibcBaseline : null,
  binaries: ["north", "north-daemon"],
}, null, 2) + "\n");
NODE
    (cd "$stage" && shasum -a 256 north north-daemon manifest.json > checksums.sha256)

    archive_dir=${NORTH_CLI_OUTPUT_DIR:-$ROOT/dist/cli}
    archive_name="north-cli-v${cargo_version}-${target}.tar.gz"
    mkdir -p "$archive_dir"
    archive="$archive_dir/$archive_name"
    rm -f "$archive" "$archive.sha256"
    tar -czf "$archive" -C "$stage" north north-daemon manifest.json checksums.sha256
    (cd "$archive_dir" && shasum -a 256 "$archive_name" > "$archive_name.sha256")
    node "$ROOT/scripts/verify-cli-archive.mjs" \
        "$archive" "$source_commit" "$cargo_version" "$target" --execute
    release_require_clean_source cli-package "package assembly" "$source_commit"
    printf 'release.sh cli-package: %s\n' "$archive"
}

release_qualify() {

RELEASE_ORIGINAL_HOME=${HOME:?}
# Keep toolchain caches available while release_init isolates runtime HOME/state.
RELEASE_RUSTUP_HOME=${RUSTUP_HOME:-${HOME:?}/.rustup}
RELEASE_CARGO_HOME=${CARGO_HOME:-${HOME:?}/.cargo}
if [[ -n "${PLAYWRIGHT_BROWSERS_PATH:-}" ]]; then
    RELEASE_PLAYWRIGHT_BROWSERS_PATH=$PLAYWRIGHT_BROWSERS_PATH
elif [[ "${OSTYPE:-}" == darwin* ]]; then
    RELEASE_PLAYWRIGHT_BROWSERS_PATH="${HOME:?}/Library/Caches/ms-playwright"
else
    RELEASE_PLAYWRIGHT_BROWSERS_PATH="${HOME:?}/.cache/ms-playwright"
fi
# shellcheck source=tests/release/common.sh
source "$ROOT/tests/release/common.sh"

release_require_tool node
if [[ "${OSTYPE:-}" == darwin* ]]; then
    node_version=$(node -p 'process.versions.node')
    IFS=. read -r node_major node_minor _ <<<"$node_version"
    if (( node_major < 24 || (node_major == 24 && node_minor < 21) )); then
        printf 'release.sh qualify: macOS local qualification requires Node.js 24.21.0 or newer for user-Keychain TLS; detected %s. Use Node 24.21.0+; trust stores are not modified.\n' "$node_version" >&2
        exit 2
    fi
fi
artifact_dir=${NORTH_RELEASE_ARTIFACT_DIR:-${1:-}}
oci_dir=${NORTH_RELEASE_OCI_DIR:-}
if [[ -n "$oci_dir" && -z "$artifact_dir" ]]; then
    release_log "OWNER-ACTION OCI qualification requires NORTH_RELEASE_ARTIFACT_DIR"
    exit 2
fi
if [[ -z "$artifact_dir" ]]; then
    release_log "NOT-RUN immutable-package qualification; workspace build selected"
fi
if [[ -z "$oci_dir" ]]; then
    release_log "NOT-RUN OCI Compose and PostgreSQL-volume qualification; NORTH_RELEASE_OCI_DIR is unset"
fi
if [[ -n "$artifact_dir" ]]; then
    manifest="$artifact_dir/manifest.json"
    checksums="$artifact_dir/checksums.sha256"
    server_bin="$artifact_dir/bin/north-server"
    daemon_bin="$artifact_dir/bin/north-daemon"
    [[ -x "$server_bin" ]] || server_bin="$artifact_dir/north-server"
    [[ -x "$daemon_bin" ]] || daemon_bin="$artifact_dir/north-daemon"
    [[ -f "$manifest" && -f "$checksums" ]] || {
        printf 'release.sh qualify: manifest/checksums missing in %s\n' "$artifact_dir" >&2
        exit 2
    }
    [[ -x "$server_bin" && -x "$daemon_bin" && -f "$artifact_dir/web/server.js" ]] || {
        printf 'release.sh qualify: packaged server, daemon, or web artifact missing\n' >&2
        exit 2
    }

    version=$(node -p "require(process.argv[1]).version" "$manifest")
    version_changed=$(node -p "String(require(process.argv[1]).version_changed)" "$manifest")
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
        printf 'release.sh qualify: invalid manifest version\n' >&2
        exit 2
    }
    source_commit=$(git rev-parse HEAD)
    node "$ROOT/scripts/verify-release-artifact.mjs" \
        "$artifact_dir" "$source_commit" "v$version"
    if [[ -n "$oci_dir" ]]; then
        release_require_env NORTH_RELEASE_IMAGE_OWNER
        node "$ROOT/scripts/verify-release-images.mjs" \
            "$oci_dir" "$source_commit" "v$version" "$version_changed" "$NORTH_RELEASE_IMAGE_OWNER"
    fi
    target=$(node -p "require(process.argv[1]).target" "$manifest")
    [[ "$target" == x86_64-unknown-linux-gnu ]] || {
        printf 'release.sh qualify: unsupported artifact target: %s\n' "$target" >&2
        exit 2
    }
    command -v file >/dev/null 2>&1 || {
        printf 'release.sh qualify: file command is required for ELF verification\n' >&2
        exit 2
    }
    release_require_tool shasum
    server_file=$(file "$server_bin")
    daemon_file=$(file "$daemon_bin")
    for binary_file in "$server_file" "$daemon_file"; do
        grep -q 'ELF 64-bit.*x86-64' <<<"$binary_file" || {
            printf 'release.sh qualify: packaged binaries are not Linux x86_64 ELF\n' >&2
            exit 2
        }
    done
    (cd "$artifact_dir" && shasum -a 256 -c checksums.sha256 >/dev/null)
    [[ "$("$server_bin" --version)" == "north-server $version" ]] || {
        printf 'release.sh qualify: server version mismatch\n' >&2
        exit 2
    }
    "$daemon_bin" --help >/dev/null
    export NORTH_RELEASE_ARTIFACT_DIR="$artifact_dir"
fi

release_init
export RUSTUP_HOME="$RELEASE_RUSTUP_HOME"
export CARGO_HOME="$RELEASE_CARGO_HOME"
export PLAYWRIGHT_BROWSERS_PATH="$RELEASE_PLAYWRIGHT_BROWSERS_PATH"
export NORTH_PI_AGENT_COMMAND="$ROOT/tests/release/fake-agent.mjs"
release_require_tool cargo
release_require_tool curl
release_require_tool openssl
release_require_env NORTH_TEST_DATABASE_URL

release_tls_default_dir="$RELEASE_ORIGINAL_HOME/.config/north/release-tls"
if [[ -n "${NORTH_RELEASE_TLS_DIR:-}" ]]; then
    release_tls_dir=$NORTH_RELEASE_TLS_DIR
else
    release_tls_dir=$(dirname "${NORTH_RELEASE_TLS_KEY_FILE:-$release_tls_default_dir/server.key}")
fi
release_tls_cert=${NORTH_RELEASE_TLS_CERT_FILE:-"$release_tls_dir/server.crt"}
release_tls_key=${NORTH_RELEASE_TLS_KEY_FILE:-"$release_tls_dir/server.key"}
release_tls_key_dir=$(dirname "$release_tls_key")
release_tls_mode() {
    if [[ "${OSTYPE:-}" == darwin* ]]; then
        stat -f "%Lp" "$1"
    else
        stat -c "%a" "$1"
    fi
}
release_tls_owner_action() {
    release_log "OWNER-ACTION persistent TLS setup: $*"
    exit 2
}
[[ -d "$release_tls_key_dir" && ! -L "$release_tls_key_dir" ]] || release_tls_owner_action "create real directory $release_tls_key_dir once, install trusted localhost server certificate and private key"
[[ -f "$release_tls_cert" && ! -L "$release_tls_cert" && -r "$release_tls_cert" ]] || release_tls_owner_action "certificate must be a readable regular file: $release_tls_cert"
[[ -f "$release_tls_key" && ! -L "$release_tls_key" && -r "$release_tls_key" ]] || release_tls_owner_action "private key must be a readable regular file: $release_tls_key"
dir_mode=$(release_tls_mode "$release_tls_key_dir")
key_mode=$(release_tls_mode "$release_tls_key")
if [[ "$dir_mode" =~ ^[0-7]+$ ]] && (( 8#$dir_mode & 077 )); then
    release_tls_owner_action "TLS directory must be mode 0700: $release_tls_dir"
fi
if [[ "$key_mode" =~ ^[0-7]+$ ]] && (( 8#$key_mode & 077 )); then
    release_tls_owner_action "TLS private key must be mode 0600: $release_tls_key"
fi
openssl x509 -in "$release_tls_cert" -noout >/dev/null 2>"$RELEASE_TMPDIR/state/tls-validation.log" || release_tls_owner_action "invalid PEM certificate; replace $release_tls_cert"
openssl pkey -in "$release_tls_key" -passin pass: -noout >/dev/null 2>>"$RELEASE_TMPDIR/state/tls-validation.log" || release_tls_owner_action "invalid or encrypted PEM private key; replace $release_tls_key"
openssl x509 -in "$release_tls_cert" -checkend 86400 -noout >/dev/null 2>>"$RELEASE_TMPDIR/state/tls-validation.log" || release_tls_owner_action "certificate expires within 24 hours: $release_tls_cert"
release_tls_not_before=$(openssl x509 -in "$release_tls_cert" -noout -startdate 2>>"$RELEASE_TMPDIR/state/tls-validation.log" | sed 's/^notBefore=//')
if [[ "${OSTYPE:-}" == darwin* ]]; then
    release_tls_not_before_epoch=$(LC_ALL=C date -j -f "%b %e %T %Y %Z" "$release_tls_not_before" +%s 2>/dev/null || true)
else
    release_tls_not_before_epoch=$(LC_ALL=C date -d "$release_tls_not_before" +%s 2>/dev/null || true)
fi
[[ "$release_tls_not_before_epoch" =~ ^[0-9]+$ ]] || release_tls_owner_action "certificate has invalid notBefore date: $release_tls_cert"
(( release_tls_not_before_epoch <= $(date +%s) )) || release_tls_owner_action "certificate is not valid yet: $release_tls_cert"
release_tls_details=$(openssl x509 -in "$release_tls_cert" -text -noout 2>>"$RELEASE_TMPDIR/state/tls-validation.log" || true)
if grep -q "CA:TRUE" <<<"$release_tls_details"; then
    release_tls_owner_action "server certificate must not be a CA: $release_tls_cert"
fi
grep -q "X509v3 Key Usage:" <<<"$release_tls_details" || release_tls_owner_action "certificate must declare key usage: $release_tls_cert"
grep -q "Digital Signature" <<<"$release_tls_details" || release_tls_owner_action "certificate key usage must include digitalSignature: $release_tls_cert"
release_tls_san=$(openssl x509 -in "$release_tls_cert" -noout -ext subjectAltName 2>>"$RELEASE_TMPDIR/state/tls-validation.log" || true)
grep -Eq "DNS:localhost([^[:alnum:]_]|$)" <<<"$release_tls_san" || release_tls_owner_action "certificate SAN must include DNS:localhost: $release_tls_cert"
grep -Eq "IP Address:127\\.0\\.0\\.1([^[:alnum:]_]|$)" <<<"$release_tls_san" || release_tls_owner_action "certificate SAN must include IP:127.0.0.1: $release_tls_cert"
release_tls_eku=$(openssl x509 -in "$release_tls_cert" -text -noout 2>>"$RELEASE_TMPDIR/state/tls-validation.log" || true)
grep -q "TLS Web Server Authentication" <<<"$release_tls_eku" || release_tls_owner_action "certificate EKU must include serverAuth: $release_tls_cert"
release_tls_cert_pub=$(openssl x509 -in "$release_tls_cert" -pubkey -noout | openssl pkey -pubin -outform DER | openssl dgst -sha256)
release_tls_key_pub=$(openssl pkey -in "$release_tls_key" -passin pass: -pubout | openssl pkey -pubin -outform DER | openssl dgst -sha256)
[[ "$release_tls_cert_pub" == "$release_tls_key_pub" ]] || release_tls_owner_action "certificate and private key do not match"
release_log "PASS persistent-tls-material cert=$release_tls_cert"

if [[ -n "${NORTH_OTP_HMAC_KEY:-}" ]]; then
    otp_key=$NORTH_OTP_HMAC_KEY
else
    otp_key=$(openssl rand -hex 32)
fi

pick_port() {
    node -e '
      const net = require("node:net");
      const server = net.createServer();
      server.listen(0, "127.0.0.1", () => {
        process.stdout.write(String(server.address().port));
        server.close();
      });
    '
}

wait_for_http() {
    local url=$1
    for _ in $(seq 1 120); do
        if curl --silent --show-error --fail --max-time 2 "$url" >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
    done
    release_log "OWNER-ACTION startup timeout url=$url"
    return 1
}

wait_for_line() {
    local file=$1
    local pattern=$2
    for _ in $(seq 1 120); do
        if grep -q "$pattern" "$file" 2>/dev/null; then return 0; fi
        sleep 1
    done
    release_log "OWNER-ACTION process output timeout file=$file pattern=$pattern"
    return 1
}

run_bounded() {
    local seconds=$1
    shift
    "$@" &
    local pid=$!
    for _ in $(seq 1 "$seconds"); do
        if ! kill -0 "$pid" 2>/dev/null; then
            wait "$pid"
            return $?
        fi
        sleep 1
    done
    kill TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    release_log "OWNER-ACTION bounded command timeout seconds=$seconds"
    return 124
}

json_field() {
    node -e 'const value=JSON.parse(process.argv[1]); const path=process.argv[2].split("."); let current=value; for (const key of path) current=current?.[key]; if (current === undefined || current === null) process.exit(1); process.stdout.write(String(current));' "$1" "$2"
}

server_port=$(pick_port)
web_port=$(pick_port)
proxy_port=$(pick_port)
server_url="http://127.0.0.1:$server_port"
web_url="http://127.0.0.1:$web_port"
release_compose_file="$ROOT/docker-compose.yaml"
release_compose_project=""
release_compose_started=false

release_compose_cleanup() {
    if [[ "$release_compose_started" == true ]]; then
        docker compose --project-name "$release_compose_project" \
            --file "$release_compose_file" down \
            --volumes --remove-orphans --timeout 10 >/dev/null 2>&1 || true
    fi
}

release_compose_start() {
    release_require_tool docker
    release_require_tool skopeo
    docker compose version >/dev/null 2>&1 || {
        release_log "OWNER-ACTION docker compose plugin required for OCI qualification"
        exit 2
    }

    local archive_tag="sha-$source_commit"
    local database_password
    local image_format
    local service image image_info image_metadata image_digest archive_sha image_id
    image_format='{{.Os}}/{{.Architecture}} {{index .Config.Labels "org.opencontainers.image.version"}} {{index .Config.Labels "org.opencontainers.image.revision"}}'
    for service in north-server north-web; do
        image=$(node -e 'const [path, service] = process.argv.slice(1); const images = JSON.parse(require("node:fs").readFileSync(path, "utf8")).images; const image = images.find((entry) => entry.service === service)?.image; if (!image) process.exit(1); process.stdout.write(image);' "$oci_dir/images.json" "$service")
        image_info=$(node -e '
          const fs = require("node:fs");
          const [path, service] = process.argv.slice(1);
          const item = JSON.parse(fs.readFileSync(path, "utf8"))
            .images.find((entry) => entry.service === service);
          if (!item) process.exit(1);
          process.stdout.write(
            item.manifest_digest + "|" + item.archive_sha256,
          );
        ' "$oci_dir/images.json" "$service")
        IFS='|' read -r image_digest archive_sha <<< "$image_info"
        skopeo copy "oci-archive:$oci_dir/$service.oci.tar:$archive_tag" \
            "docker-daemon:$image"
        image_metadata=$(docker image inspect --format "$image_format" "$image")
        [[ "$image_metadata" == "linux/amd64 $version $source_commit" ]] || {
            release_log "OWNER-ACTION OCI image metadata mismatch service=$service"
            exit 2
        }
        image_id=$(docker image inspect --format '{{.Id}}' "$image")
        release_log "PASS compose-image service=$service ref=$image"
        release_log "compose-image-digest service=$service sha256=$image_digest"
        release_log "compose-image-archive service=$service sha256=$archive_sha"
        release_log "compose-image-id service=$service id=$image_id"
        release_log "compose-image-metadata service=$service value=$image_metadata"
    done

    NORTH_SERVER_IMAGE=$(node -e 'const d = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8")); process.stdout.write(d.images.find((i) => i.service === "north-server").image);' "$oci_dir/images.json")
    NORTH_WEB_IMAGE=$(node -e 'const d = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8")); process.stdout.write(d.images.find((i) => i.service === "north-web").image);' "$oci_dir/images.json")
    export NORTH_SERVER_IMAGE NORTH_WEB_IMAGE
    export POSTGRES_DB=north POSTGRES_USER=north
    database_password=$(openssl rand -hex 16)
    export POSTGRES_PASSWORD="$database_password"
    export DATABASE_URL="postgres://north:$database_password@postgres:5432/north"
    export NORTH_OTP_HMAC_KEY="$otp_key"
    export NORTH_SERVER_HOST_PORT="$server_port"
    export NORTH_WEB_HOST_PORT="$web_port"

    release_compose_project="north-release-${RANDOM}-${RANDOM}"
    release_compose_started=true
    # shellcheck disable=SC2034 # Read by release_cleanup in sourced common.sh.
    RELEASE_CLEANUP_HOOK=release_compose_cleanup
    release_log "qualification evidence=oci-image-migration"
    run_bounded 300 docker compose --project-name "$release_compose_project" \
        --file "$release_compose_file" up --detach --wait postgres
    release_log "PASS compose-database-ready"
    run_bounded 60 docker compose --project-name "$release_compose_project" \
        --file "$release_compose_file" run --rm --no-deps north-server migrate
    release_log "PASS compose-image-migration-command"
    release_log "qualification evidence=oci-compose"
    run_bounded 300 docker compose --project-name "$release_compose_project" \
        --file "$release_compose_file" up --detach --wait
    release_log "PASS compose-runtime-started project=$release_compose_project"

    local invalid_key="not-a-valid-otp-key"
    local startup_log="$RELEASE_TMPDIR/logs/oci-invalid-otp.log"
    local startup_container="${release_compose_project}-invalid-key"
    local startup_status
    if run_bounded 30 docker run --rm --name "$startup_container" \
        --network "${release_compose_project}_default" \
        --env DATABASE_URL \
        --env "NORTH_OTP_HMAC_KEY=$invalid_key" \
        --env NORTH_BIND_ADDR=0.0.0.0:8080 \
        "$NORTH_SERVER_IMAGE" >"$startup_log" 2>&1; then
        docker rm --force "$startup_container" >/dev/null 2>&1 || true
        release_log "OWNER-ACTION OCI server image accepted invalid OTP key"
        exit 1
    else
        startup_status=$?
    fi
    docker rm --force "$startup_container" >/dev/null 2>&1 || true
    if [[ "$startup_status" -eq 124 ]] || ! grep -Fq "north-server startup failed: configuration" "$startup_log"; then
        release_log "OWNER-ACTION OCI invalid-key startup lacked safe failure log=$startup_log"
        exit 1
    fi
    if grep -Fq "$invalid_key" "$startup_log" || grep -Fq "$database_password" "$startup_log"; then
        release_log "OWNER-ACTION OCI invalid-key startup leaked configuration log=$startup_log"
        exit 1
    fi
    release_log "PASS compose-image-invalid-otp-startup"
}

if [[ -n "$artifact_dir" ]]; then
    server_bin="${artifact_dir}/bin/north-server"
    daemon_bin="${artifact_dir}/bin/north-daemon"
    web_dir="${artifact_dir}/web"
    [[ -x "$server_bin" ]] || server_bin="${artifact_dir}/north-server"
    [[ -x "$daemon_bin" ]] || daemon_bin="${artifact_dir}/north-daemon"
    [[ -x "$server_bin" ]] || { release_log "OWNER-ACTION missing artifact north-server"; exit 2; }
    [[ -x "$daemon_bin" ]] || { release_log "OWNER-ACTION missing artifact north-daemon"; exit 2; }
    [[ -f "$web_dir/server.js" ]] || { release_log "OWNER-ACTION missing artifact web/server.js"; exit 2; }
else
    run_bounded 900 cargo build --locked --release -p north-server -p north-daemon
    server_bin="$ROOT/target/release/north-server"
    daemon_bin="$ROOT/target/release/north-daemon"
    web_dir="$ROOT/apps/web"
    (cd "$web_dir" && run_bounded 900 npm run build)
fi

release_check_invalid_otp_startup() {
    local invalid_key='north-release-invalid-otp-7c5b41'
    local invalid_log="$RELEASE_TMPDIR/logs/invalid-otp-startup.log"
    local database_password probe_port probe_url server_pid startup_status=0 listening=false

    probe_port=$(pick_port)
    probe_url="http://127.0.0.1:$probe_port/healthz"
    database_password=$(node - "$NORTH_TEST_DATABASE_URL" <<'NODE'
try {
  process.stdout.write(decodeURIComponent(new URL(process.argv[2]).password));
} catch {
  process.exit(2);
}
NODE
    ) || {
        release_log "OWNER-ACTION NORTH_TEST_DATABASE_URL is not a valid PostgreSQL URL"
        return 2
    }

    env DATABASE_URL="$NORTH_TEST_DATABASE_URL" \
        NORTH_OTP_HMAC_KEY="$invalid_key" \
        NORTH_BIND_ADDR="127.0.0.1:$probe_port" \
        "$server_bin" >"$invalid_log" 2>&1 &
    server_pid=$!
    release_track_pid "$server_pid"
    for _ in $(seq 1 40); do
        if curl --silent --show-error --fail --max-time 0.2 "$probe_url" >/dev/null 2>&1; then
            listening=true
            break
        fi
        if ! kill -0 "$server_pid" 2>/dev/null; then
            break
        fi
        sleep 0.1
    done
    if kill -0 "$server_pid" 2>/dev/null; then
        kill TERM "$server_pid" 2>/dev/null || true
    fi
    if wait "$server_pid"; then
        startup_status=0
    else
        startup_status=$?
    fi

    if [[ "$listening" == true || "$startup_status" -eq 0 ]] ||
        ! grep -Fq "north-server startup failed: configuration" "$invalid_log"; then
        release_log "OWNER-ACTION packaged server did not reject invalid OTP before listen log=$invalid_log"
        return 1
    fi
    if grep -Fq "$invalid_key" "$invalid_log" ||
        grep -Fq "$NORTH_TEST_DATABASE_URL" "$invalid_log" ||
        { [[ -n "$database_password" ]] && grep -Fq "$database_password" "$invalid_log"; }; then
        release_log "OWNER-ACTION invalid-OTP startup leaked configuration log=$invalid_log"
        return 1
    fi
    release_log "PASS packaged-invalid-OTP-before-listen-redacted"
}

release_log "qualification evidence=packaged-migration-command"
run_bounded 60 env -u NORTH_OTP_HMAC_KEY DATABASE_URL="$NORTH_TEST_DATABASE_URL" \
    "$server_bin" migrate
release_log "PASS packaged-migration-command"

release_log "qualification evidence=fresh-install-migration"
run_bounded 900 env NORTH_TEST_DATABASE_URL="$NORTH_TEST_DATABASE_URL" \
    cargo test --locked -p north-server --test fresh_install -- --ignored
release_log "PASS fresh-install-migration parity"
release_check_invalid_otp_startup

if [[ -n "$oci_dir" ]]; then
    release_compose_start
fi

server_log="$RELEASE_TMPDIR/logs/server.log"
if [[ -z "$oci_dir" ]]; then
    (
        export DATABASE_URL="$NORTH_TEST_DATABASE_URL"
        export NORTH_BIND_ADDR="127.0.0.1:$server_port"
        export NORTH_OTP_HMAC_KEY="$otp_key"
        exec "$server_bin"
    ) >"$server_log" 2>&1 &
    server_pid=$!
    release_track_pid "$server_pid"
else
    docker compose --project-name "$release_compose_project" \
        --file "$release_compose_file" logs --follow --no-color north-server \
        >"$server_log" 2>&1 &
    server_logs_pid=$!
    release_track_pid "$server_logs_pid"
fi
wait_for_http "$server_url/healthz"
release_log "PASS server-runtime healthz"

web_log="$RELEASE_TMPDIR/logs/web.log"
if [[ -z "$oci_dir" ]]; then
    if [[ -n "$artifact_dir" ]]; then
        (
            cd "$web_dir"
            export HOSTNAME=127.0.0.1
            export PORT="$web_port"
            exec node server.js
        ) >"$web_log" 2>&1 &
    else
        (
            cd "$ROOT/apps/web"
            export NEXT_TELEMETRY_DISABLED=1
            exec npm run start -- --hostname 127.0.0.1 --port "$web_port"
        ) >"$web_log" 2>&1 &
    fi
    web_pid=$!
    release_track_pid "$web_pid"
fi
wait_for_http "$web_url/"
release_log "PASS web-runtime"

proxy_log="$RELEASE_TMPDIR/logs/proxy.log"
node "$ROOT/tests/release/http-proxy.mjs" \
    --server-url "$server_url" \
    --web-url "$web_url" \
    --cert-file "$release_tls_cert" \
    --key-file "$release_tls_key" \
    --port "$proxy_port" >"$proxy_log" 2>&1 &
proxy_pid=$!
release_track_pid "$proxy_pid"
wait_for_line "$proxy_log" '"url"'
proxy_info=$(grep -m1 '^{' "$proxy_log")
proxy_url=$(json_field "$proxy_info" url)
wait_for_http "$proxy_url/"
release_log "PASS same-origin-tls-proxy url=$proxy_url cert=$release_tls_cert"
wait_for_http "$server_url/healthz"

email="north-release-${RANDOM}@example.test"
cookie_jar="$RELEASE_TMPDIR/state/session.cookies"
code_request=$(curl --silent --show-error --fail --max-time 10 \
    -H 'content-type: application/json' \
    --data "{\"email\":\"$email\"}" "$server_url/auth/request-code")
: "$code_request"
verification_code=""
for _ in $(seq 1 60); do
    verification_code=$(sed -n "s/.*north verification code email=$email code=\([0-9][0-9]*\).*/\1/p" "$server_log" | tail -1 || true)
    if [[ -n "$verification_code" ]]; then break; fi
    sleep 1
done
[[ "$verification_code" =~ ^[0-9]{6}$ ]] || {
    release_log "OWNER-ACTION verification code was not found; private log=$server_log"
    exit 1
}
verify_headers="$RELEASE_TMPDIR/state/verify.headers"
curl --silent --show-error --fail --max-time 10 -c "$cookie_jar" -b "$cookie_jar" \
    -H 'content-type: application/json' \
    --data "{\"email\":\"$email\",\"code\":\"$verification_code\"}" \
    -D "$verify_headers" -o /dev/null "$server_url/auth/verify"
session_token=$(sed -n 's/^set-cookie: north_session=\([^;]*\).*/\1/ip' "$verify_headers" | tail -1)
[[ -n "$session_token" ]] || { release_log "OWNER-ACTION session cookie was not issued"; exit 1; }
release_log "PASS authenticated-owner-session log=$server_log"

setup_json=$(curl --silent --show-error --fail --max-time 10 \
    -H 'content-type: application/json' \
    --data '{"label":"North release qualification daemon"}' \
    "$server_url/daemon/setup/request")
request_token=$(json_field "$setup_json" request_token)
curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" \
    -H "Origin: $server_url" -H "Host: 127.0.0.1:$server_port" \
    -X POST "$server_url/daemon/setup/$request_token/approve" >/dev/null

claim_json=''
for _ in $(seq 1 60); do
    claim_json=$(curl --silent --show-error --max-time 10 "$server_url/daemon/setup/$request_token" || true)
    if [[ "$(json_field "$claim_json" status 2>/dev/null || true)" == "claimed" ]]; then break; fi
    sleep 1
done
daemon_id=$(json_field "$claim_json" daemon_id)
daemon_credential=$(json_field "$claim_json" credential)
daemon_state="$RELEASE_TMPDIR/state/daemon.json"
NORTH_RELEASE_DAEMON_CREDENTIAL="$daemon_credential" node - "$daemon_state" "$proxy_url" "$daemon_id" <<'NODE'
const fs = require("node:fs");
const [path, serverUrl, daemonId] = process.argv.slice(2);
const credential = process.env.NORTH_RELEASE_DAEMON_CREDENTIAL;
if (!credential) throw new Error("daemon credential missing");
fs.writeFileSync(path, JSON.stringify({
  server_url: serverUrl,
  daemon_id: daemonId,
  credential,
  capabilities: ["agent"],
}) + "\n", { mode: 0o600 });
NODE
release_log "PASS daemon-setup-provisioned"

transport_requirement_json=$(curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" \
    -H 'content-type: application/json' \
    --data '{"title":"Transport release qualification","description":"Live daemon protocol proof."}' \
    "$server_url/requirements")
transport_requirement_id=$(json_field "$transport_requirement_json" id)
transport_trigger_log="$RELEASE_TMPDIR/logs/transport-trigger.log"
(
    for _ in $(seq 1 60); do
        daemon_listing=$(curl --silent --show-error --max-time 10 \
            -H "Cookie: north_session=$session_token" "$server_url/daemons" || true)
        if node -e 'const daemons=JSON.parse(process.argv[1]); process.exit(daemons.some((daemon) => daemon.daemon_id === process.argv[2] && daemon.connected) ? 0 : 1)' \
            "$daemon_listing" "$daemon_id" 2>/dev/null; then
            structured_requirement=$(curl --silent --show-error --fail --max-time 10 \
                -H "Cookie: north_session=$session_token" \
                -H 'content-type: application/json' \
                -X PATCH \
                --data '{"expected_state_version":1,"summary":"Live transport evidence","acceptance_criteria":["Server accepts deterministic readiness facts."],"assumptions":["Qualification daemon is trusted."],"open_questions":[]}' \
                "$server_url/requirements/$transport_requirement_id")
            transport_state_version=$(json_field "$structured_requirement" state_version)
            message_json=$(curl --silent --show-error --fail --max-time 10 \
                -H "Cookie: north_session=$session_token" \
                -H 'content-type: application/json' \
                --data '{"body":"Transport clarification"}' \
                "$server_url/requirements/$transport_requirement_id/conversation/messages")
            message_id=$(json_field "$message_json" id)
            curl --silent --show-error --fail --max-time 10 \
                -H "Cookie: north_session=$session_token" \
                -H 'content-type: application/json' \
                --data "{\"message_id\":\"$message_id\",\"expected_state_version\":$transport_state_version}" \
                "$server_url/requirements/$transport_requirement_id/clarification/start" \
                >"$transport_trigger_log"
            exit 0
        fi
        sleep 1
    done
    printf 'transport qualification trigger timed out\\n' >"$transport_trigger_log"
    exit 1
) &
transport_trigger_pid=$!
release_track_pid "$transport_trigger_pid"
release_log "qualification evidence=trusted-wss-runtime"
# macOS rustls-native-certs reads user trust settings relative to HOME. Expose
# the real HOME only to WSS clients; keep XDG state and other children isolated.
native_trust_home=$HOME
if [[ "${OSTYPE:-}" == darwin* ]]; then
    native_trust_home=$RELEASE_ORIGINAL_HOME
fi
if ! run_bounded 900 env \
    HOME="$native_trust_home" \
    NORTH_RELEASE_WSS_URL="${proxy_url/https:/wss:}/daemon/ws" \
    NORTH_RELEASE_DAEMON_ID="$daemon_id" \
    NORTH_RELEASE_DAEMON_CREDENTIAL="$daemon_credential" \
    NORTH_RELEASE_REQUIREMENT_ID="$transport_requirement_id" \
    cargo test --locked -p north-transport-integration --test release_qualification -- --ignored; then
    release_log "OWNER-ACTION trusted-wss failed private_logs=$RELEASE_TMPDIR/logs"
    tail -30 "$proxy_log" "$transport_trigger_log" >&2 || true
    exit 1
fi
if ! wait "$transport_trigger_pid"; then
    release_log "OWNER-ACTION trusted-wss trigger failed private_logs=$transport_trigger_log"
    exit 1
fi
release_log "PASS daemon-supervisor-trusted-wss"
transport_readiness=$(curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" \
    "$server_url/requirements/$transport_requirement_id/readiness")
node -e 'const body=JSON.parse(process.argv[1]); if(body.assessment?.verdict !== "ready") process.exit(1)' \
    "$transport_readiness"
transport_session=$(curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" \
    "$server_url/requirements/$transport_requirement_id/session")
node -e 'const body=JSON.parse(process.argv[1]); if(body.session?.status !== "completed" || body.session?.phase !== "terminal") process.exit(1)' \
    "$transport_session"
release_log "PASS trusted-wss-canonical-projections"

daemon_log="$RELEASE_TMPDIR/logs/daemon.log"
(
    HOME="$native_trust_home" exec "$daemon_bin" start --state-file "$daemon_state" \
        --journal-file "$RELEASE_TMPDIR/state/daemon.journal.json" \
        --repository-cache-dir "$RELEASE_TMPDIR/state/repository-cache" \
        --repository-workspace-dir "$RELEASE_TMPDIR/state/repository-workspaces"
) >"$daemon_log" 2>&1 &
daemon_pid=$!
release_track_pid "$daemon_pid"

daemon_connected=false
for _ in $(seq 1 60); do
    daemons=$(curl --silent --show-error --fail --max-time 10 \
        -H "Cookie: north_session=$session_token" "$proxy_url/daemons" || true)
    if node -e 'const daemons=JSON.parse(process.argv[1]); process.exit(daemons.some((daemon) => daemon.connected) ? 0 : 1)' "$daemons" 2>/dev/null; then
        daemon_connected=true
        break
    fi
    sleep 1
done
if [[ "$daemon_connected" != true ]]; then
    release_log "OWNER-ACTION daemon did not connect private_logs=$RELEASE_TMPDIR/logs"
    tail -30 "$daemon_log" "$proxy_log" >&2 || true
    exit 1
fi
release_log "PASS daemon-runtime-connected log=$daemon_log"

release_log "qualification evidence=production-daemon-agent-runtime"
runtime_requirement_json=$(curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" \
    -H 'content-type: application/json' \
    --data '{"title":"Production daemon runtime qualification","description":"Deterministic agent fixture proof."}' \
    "$proxy_url/requirements")
runtime_requirement_id=$(json_field "$runtime_requirement_json" id)
runtime_structured=$(curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" \
    -H 'content-type: application/json' \
    -X PATCH \
    --data '{"expected_state_version":1,"summary":"Production daemon fixture evidence","acceptance_criteria":["Deterministic runtime facts reach server-owned readiness."],"assumptions":["Qualification daemon is trusted."],"open_questions":[]}' \
    "$proxy_url/requirements/$runtime_requirement_id")
runtime_state_version=$(json_field "$runtime_structured" state_version)
runtime_message=$(curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" \
    -H 'content-type: application/json' \
    --data '{"body":"Production daemon fixture clarification"}' \
    "$proxy_url/requirements/$runtime_requirement_id/conversation/messages")
runtime_message_id=$(json_field "$runtime_message" id)
runtime_trigger_log="$RELEASE_TMPDIR/logs/runtime-trigger.log"
curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" \
    -H 'content-type: application/json' \
    --data "{\"message_id\":\"$runtime_message_id\",\"expected_state_version\":$runtime_state_version}" \
    "$proxy_url/requirements/$runtime_requirement_id/clarification/start" \
    >"$runtime_trigger_log"
runtime_qualified=false
for _ in $(seq 1 60); do
    runtime_session=$(curl --silent --show-error --max-time 2 \
        -H "Cookie: north_session=$session_token" \
        "$proxy_url/requirements/$runtime_requirement_id/session" || true)
    if node -e 'const body = JSON.parse(process.argv[1]); if (body.session?.status !== "completed" || body.session?.phase !== "terminal") process.exit(1);' \
        "$runtime_session" >/dev/null 2>&1; then
        runtime_qualified=true
        break
    fi
    sleep 1
done
runtime_readiness=$(curl --silent --show-error --max-time 2 \
    -H "Cookie: north_session=$session_token" \
    "$proxy_url/requirements/$runtime_requirement_id/readiness" || true)
runtime_conversation=$(curl --silent --show-error --max-time 2 \
    -H "Cookie: north_session=$session_token" \
    "$proxy_url/requirements/$runtime_requirement_id/conversation?offset=0&limit=50" || true)
if [[ "$runtime_qualified" == true ]] && node -e 'const [readiness, session, conversation] = process.argv.slice(1).map((value) => JSON.parse(value)); if (readiness.assessment?.verdict !== "ready") process.exit(1); if (session.session?.status !== "completed" || session.session?.phase !== "terminal") process.exit(1); if (!conversation.messages?.some((message) => message.body === "Qualification clarification complete.")) process.exit(1);' \
    "$runtime_readiness" "$runtime_session" "$runtime_conversation" >/dev/null 2>&1; then
    runtime_qualified=true
else
    runtime_qualified=false
fi
if [[ "$runtime_qualified" != true ]]; then
    release_log "OWNER-ACTION production daemon fake-agent runtime did not qualify private_logs=$RELEASE_TMPDIR/logs"
    tail -30 "$daemon_log" "$proxy_log" "$runtime_trigger_log" >&2 || true
    exit 1
fi
release_log "PASS production-daemon-agent-runtime requirement=$runtime_requirement_id"

storage_state="$RELEASE_TMPDIR/state/playwright.storage.json"
auth_status=$(curl --silent --show-error --fail --max-time 10 \
    -H "Cookie: north_session=$session_token" -o /dev/null -w "%{http_code}" "$proxy_url/auth/me")
if [[ "$auth_status" != "200" ]]; then
    release_log "OWNER-ACTION proxy session check status=$auth_status private_logs=$RELEASE_TMPDIR/logs"
    tail -30 "$daemon_log" "$proxy_log" >&2 || true
    exit 1
fi
node - "$storage_state" "$session_token" <<'NODE'
const fs = require("node:fs");
const [path, value] = process.argv.slice(2);
if (!value) throw new Error("session cookie missing");
fs.writeFileSync(path, JSON.stringify({
  cookies: [{ name: "north_session", value, domain: "localhost", path: "/", secure: true, httpOnly: true }],
  origins: [],
}) + "\n", { mode: 0o600 });
NODE

release_log "qualification evidence=assembled-browser"
(
    # Chromium and Node TLS clients use macOS user trust for the local proxy.
    # Scope system CA loading and original HOME to Playwright.
    # XDG and browser state stay temporary.
    export HOME="$native_trust_home"
    export NODE_USE_SYSTEM_CA=1
    export NORTH_RELEASE_BASE_URL="$proxy_url"
    export NORTH_RELEASE_STORAGE_STATE="$storage_state"
    export NORTH_RELEASE_EMAIL="$email"
    export NORTH_RELEASE_OTP_CODE="$verification_code"
    export NORTH_RELEASE_DAEMON_CREDENTIAL="$daemon_credential"
    cd "$ROOT/apps/web"
    run_bounded 900 npm run test:e2e:release
)
release_log "PASS assembled-browser-sse-golden-path"

if [[ -n "$oci_dir" ]]; then
    release_log "qualification evidence=postgres-volume-persistence"
    compose_args=(--project-name "$release_compose_project" --file "$release_compose_file")
    volume_probe_value=$(openssl rand -hex 16)
    run_bounded 30 docker compose "${compose_args[@]}" exec --no-TTY postgres \
        psql --no-psqlrc --set ON_ERROR_STOP=1 --username "$POSTGRES_USER" \
        --dbname "$POSTGRES_DB" --command \
        "CREATE TABLE north_release_volume_probe (value text PRIMARY KEY); INSERT INTO north_release_volume_probe VALUES ('$volume_probe_value');"
    run_bounded 120 docker compose "${compose_args[@]}" up --detach --wait \
        --no-deps --force-recreate postgres
    persisted_volume_probe=$(docker compose "${compose_args[@]}" exec --no-TTY postgres \
        psql --no-psqlrc --set ON_ERROR_STOP=1 --tuples-only --no-align \
        --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
        --command 'SELECT value FROM north_release_volume_probe;')
    if [[ "$persisted_volume_probe" != "$volume_probe_value" ]]; then
        release_log "OWNER-ACTION PostgreSQL named volume did not preserve probe"
        exit 1
    fi
    run_bounded 30 docker compose "${compose_args[@]}" exec --no-TTY postgres \
        psql --no-psqlrc --set ON_ERROR_STOP=1 --username "$POSTGRES_USER" \
        --dbname "$POSTGRES_DB" --command 'DROP TABLE north_release_volume_probe;'
    release_log "PASS postgres-volume-persistence"
fi

release_log "PASS release qualification; private logs=$RELEASE_TMPDIR/logs"
}

[[ $# -gt 0 ]] || { usage; exit 2; }
mode=$1
shift
case "$mode" in
    package) release_package "$@" ;;
    cli-package) release_cli_package "$@" ;;
    qualify) release_qualify "$@" ;;
    *) printf 'release.sh: unknown mode: %s\n' "$mode" >&2; usage; exit 2 ;;
esac
