#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'publish-cli-release: %s\n' "$1" >&2
    exit 1
}

usage() {
    printf 'usage: %s <draft|finalize> TAG SOURCE_SHA ASSET_DIR\n' "${0##*/}" >&2
}

[[ $# -eq 4 ]] || { usage; exit 2; }
mode=$1
release_tag=$2
source_sha=$3
asset_dir=$4
[[ "$mode" == draft || "$mode" == finalize ]] || { usage; exit 2; }
[[ "$release_tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
    fail "tag must be strict vX.Y.Z SemVer"
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail "source SHA must be full"
repo=${GITHUB_REPOSITORY:-}
[[ "$repo" =~ ^[^/]+/[^/]+$ ]] || fail "GITHUB_REPOSITORY must be owner/name"
version=${release_tag#v}
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

expected_targets=(
    x86_64-unknown-linux-gnu
    x86_64-apple-darwin
    aarch64-apple-darwin
)
expected_assets=()
for target in "${expected_targets[@]}"; do
    archive="north-cli-v${version}-${target}.tar.gz"
    archive_path="$asset_dir/$archive"
    sidecar_path="$archive_path.sha256"
    [[ -f "$archive_path" && -f "$sidecar_path" ]] || fail "missing CLI archive or checksum: $archive"
    node "$root/scripts/verify-cli-archive.mjs" "$archive_path" "$source_sha" "$version" "$target"
    expected_assets+=("$archive" "$archive.sha256")
done
[[ ${#expected_assets[@]} -eq 6 ]] || fail "expected exactly six CLI release assets"

expected_name() {
    local candidate=$1 name
    for name in "${expected_assets[@]}"; do
        [[ "$candidate" == "$name" ]] && return 0
    done
    return 1
}

release_field() {
    node - "$1" "$2" <<'NODE'
const fs = require("node:fs");
const [path, field] = process.argv.slice(2);
const value = JSON.parse(fs.readFileSync(path, "utf8"))[field];
if (value !== undefined && value !== null) process.stdout.write(String(value));
NODE
}

release_asset_names() {
    node - "$1" <<'NODE'
const fs = require("node:fs");
const assets = JSON.parse(fs.readFileSync(process.argv[2], "utf8")).assets;
if (!Array.isArray(assets) || assets.some(({ name }) => typeof name !== "string") ||
    new Set(assets.map(({ name }) => name)).size !== assets.length) process.exit(1);
for (const { name } of assets) process.stdout.write(`${name}\n`);
NODE
}

release_has_asset() {
    node - "$1" "$2" <<'NODE'
const fs = require("node:fs");
const [path, name] = process.argv.slice(2);
const assets = JSON.parse(fs.readFileSync(path, "utf8")).assets;
process.exitCode = Array.isArray(assets) && assets.some((asset) => asset.name === name) ? 0 : 1;
NODE
}

check_metadata() {
    local release_file=$1
    [[ "$(release_field "$release_file" tagName)" == "$release_tag" ]] ||
        fail "GitHub Release tag does not match"
    [[ "$(release_field "$release_file" targetCommitish)" == "$source_sha" ]] ||
        fail "GitHub Release target does not match source SHA"
}

check_assets() {
    local release_file=$1 require_complete=$2 name count=0 names
    names=$(release_asset_names "$release_file") || fail "invalid GitHub Release asset list"
    if [[ -n "$names" ]]; then
        while IFS= read -r name; do
            [[ -n "$name" ]] || continue
            expected_name "$name" || fail "unexpected GitHub Release asset: $name"
            count=$((count + 1))
        done <<< "$names"
    fi
    [[ $count -le ${#expected_assets[@]} ]] || fail "unexpected GitHub Release assets"
    if [[ "$require_complete" == true && $count -ne ${#expected_assets[@]} ]]; then
        fail "GitHub Release is missing CLI assets"
    fi
}

view_release() {
    gh release view "$release_tag" --repo "$repo" \
        --json isDraft,tagName,targetCommitish,assets
}

check_remote_assets() {
    local release_file=$1 temp_dir=$2 name
    for name in "${expected_assets[@]}"; do
        if release_has_asset "$release_file" "$name"; then
            rm -f "$temp_dir/$name"
            gh release download "$release_tag" --repo "$repo" \
                --pattern "$name" --dir "$temp_dir"
            if ! cmp -s "$asset_dir/$name" "$temp_dir/$name"; then
                fail "GitHub Release asset conflicts with qualified CLI artifact: $name"
            fi
        fi
    done
}

release_file=$(mktemp)
temp_dir=$(mktemp -d)
trap 'rm -f "$release_file"; rm -rf "$temp_dir"' EXIT
if ! view_release > "$release_file" 2>/dev/null; then
    [[ "$mode" == draft ]] || fail "GitHub Release draft is missing"
    gh release create "$release_tag" --repo "$repo" --draft \
        --target "$source_sha" --title "$release_tag" --generate-notes --verify-tag
    view_release > "$release_file"
fi
check_metadata "$release_file"
check_assets "$release_file" false

if [[ "$mode" == draft ]]; then
    [[ "$(release_field "$release_file" isDraft)" == true ]] ||
        fail "existing GitHub Release is not a draft"
    check_remote_assets "$release_file" "$temp_dir"
    for name in "${expected_assets[@]}"; do
        if ! release_has_asset "$release_file" "$name"; then
            gh release upload "$release_tag" "$asset_dir/$name" --repo "$repo"
            gh release download "$release_tag" --repo "$repo" \
                --pattern "$name" --dir "$temp_dir"
            if ! cmp -s "$asset_dir/$name" "$temp_dir/$name"; then
                fail "uploaded GitHub Release asset differs from qualified artifact: $name"
            fi
            view_release > "$release_file"
            check_metadata "$release_file"
            check_assets "$release_file" false
        fi
    done
    check_assets "$release_file" true
    printf 'CLI assets uploaded to draft GitHub Release: %s\n' "$release_tag"
    exit 0
fi

check_assets "$release_file" true
check_remote_assets "$release_file" "$temp_dir"
if [[ "$(release_field "$release_file" isDraft)" == true ]]; then
    gh release edit "$release_tag" --repo "$repo" --draft=false
    view_release > "$release_file"
    check_metadata "$release_file"
    [[ "$(release_field "$release_file" isDraft)" == false ]] || fail "GitHub Release remains a draft"
else
    [[ "$(release_field "$release_file" isDraft)" == false ]] || fail "GitHub Release state is invalid"
fi
printf 'GitHub Release published: %s\n' "$release_tag"
