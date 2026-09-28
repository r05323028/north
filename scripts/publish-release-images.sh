#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'publish-release-images: %s\n' "$1" >&2
    exit 1
}

if [[ $# -ne 4 ]]; then
    printf 'usage: %s OCI_DIR OWNER vX.Y.Z INITIAL_RELEASE\n' "${0##*/}" >&2
    exit 2
fi
oci_dir=$1
owner=${2,,}
target_tag=$3
initial_release=$4
[[ "$initial_release" == true || "$initial_release" == false ]] ||
    fail "initial-release flag must be true or false"
[[ "$target_tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
    fail "target tag must be strict vX.Y.Z SemVer"
[[ "$owner" =~ ^[a-z0-9][a-z0-9-]*$ ]] || fail "invalid package owner"
metadata_file="$oci_dir/images.json"
[[ -d "$oci_dir" && -f "$metadata_file" ]] || fail "OCI metadata is missing"

metadata=$(node - "$metadata_file" <<'NODE'
const fs = require("node:fs");
const metadata = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
if (
  !/^[0-9a-f]{40}$/.test(metadata.source_commit) ||
  metadata.sha_tag !== `sha-${metadata.source_commit}` ||
  !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(metadata.version) ||
  typeof metadata.version_changed !== "boolean"
) process.exit(1);
process.stdout.write([metadata.source_commit, metadata.sha_tag, metadata.version, metadata.version_changed].join("\t"));
NODE
) || fail "invalid OCI image metadata"
IFS=$'\t' read -r source_sha sha_tag version version_changed <<< "$metadata"
[[ "$sha_tag" == "sha-$source_sha" ]] || fail "OCI SHA tag is inconsistent"
[[ "$target_tag" == "v$version" ]] || fail "target tag does not match OCI version"
[[ "$version_changed" == true || "$initial_release" == true ]] ||
    fail "SemVer publication requires a first-parent version bump unless this is the initial release"

services=(north-server north-web)
states=()
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

expected_digest() {
    node - "$metadata_file" "$1" <<'NODE'
const fs = require("node:fs");
const [path, service] = process.argv.slice(2);
const images = JSON.parse(fs.readFileSync(path, "utf8")).images;
const image = Array.isArray(images) && images.find((entry) => entry.service === service);
if (!image || !/^sha256:[0-9a-f]{64}$/.test(image.manifest_digest)) process.exit(1);
process.stdout.write(image.manifest_digest);
NODE
}

record_digest() {
    local image=$1 digest=$2
    if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
        printf '| %s | %s |\n' "$image" "$digest" >> "$GITHUB_STEP_SUMMARY"
    fi
    printf '%s: %s\n' "$image" "$digest"
}

# Preflight both destinations before copying either image; matching refs are retry-safe.
for service in "${services[@]}"; do
    archive="$oci_dir/$service.oci.tar"
    [[ -f "$archive" ]] || fail "missing OCI archive: $archive"
    expected=$(expected_digest "$service") || fail "missing OCI digest for $service"
    image="ghcr.io/$owner/$service:$target_tag"
    remote="docker://$image"
    raw_file="$tmpdir/$service.json"
    error_file="$tmpdir/$service.error"
    if skopeo inspect --raw "$remote" >"$raw_file" 2>"$error_file"; then
        actual="sha256:$(sha256sum "$raw_file" | awk '{print $1}')"
        [[ "$actual" == "$expected" ]] || fail "existing destination tag has different digest: $image"
        states+=(present)
    else
        if ! grep -Eiq 'manifest unknown|name unknown' "$error_file"; then
            cat "$error_file" >&2
            fail "unable to inspect destination: $image"
        fi
        states+=(missing)
    fi
done

for index in 0 1; do
    service=${services[$index]}
    archive="$oci_dir/$service.oci.tar"
    image="ghcr.io/$owner/$service:$target_tag"
    remote="docker://$image"
    expected=$(expected_digest "$service")
    if [[ "${states[$index]}" == missing ]]; then
        skopeo copy --preserve-digests "oci-archive:$archive:$sha_tag" "$remote"
    fi
    actual=$(skopeo inspect --raw "$remote" | sha256sum | awk '{print "sha256:" $1}')
    [[ "$actual" == "$expected" ]] || fail "registry digest mismatch for $image"
    suffix=""
    [[ "${states[$index]}" == present ]] && suffix=" (already present)"
    record_digest "$image" "$actual$suffix"
done
