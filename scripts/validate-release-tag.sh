#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'validate-release-tag: %s\n' "$1" >&2
    exit 1
}

if [[ $# -ne 1 ]]; then
    printf 'usage: %s TAG\n' "${0##*/}" >&2
    exit 2
fi
release_tag=$1
semver_re='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
[[ "$release_tag" =~ $semver_re ]] || fail "tag must be strict vX.Y.Z SemVer"
source_sha=${GITHUB_SHA:-}
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail "GITHUB_SHA must be a full commit SHA"
[[ "${GITHUB_REF:-}" == "refs/tags/$release_tag" ]] || fail "tag does not match GITHUB_REF"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
tag_commit=$(git rev-parse --verify "refs/tags/$release_tag^{commit}" 2>/dev/null) ||
    fail "tag does not resolve to a commit"
[[ "$tag_commit" == "$source_sha" ]] || fail "tag target does not match GITHUB_SHA"
main_sha=$(git rev-parse --verify 'refs/remotes/origin/main^{commit}' 2>/dev/null) ||
    fail "origin/main is unavailable; fetch full history"
git merge-base --is-ancestor "$source_sha" "$main_sha" ||
    fail "tag target is not reachable from origin/main"

release_paths=(
    .github/workflows/release.yml
    Dockerfile.server
    apps/web/Dockerfile
    docker-compose.yaml
    scripts/release.sh
    scripts/validate-release-tag.sh
    scripts/publish-cli-release.sh
    scripts/publish-release-images.sh
    scripts/verify-release-artifact.mjs
    scripts/verify-release-images.mjs
    scripts/verify-cli-archive.mjs
)
git diff --quiet "$source_sha" "$main_sha" -- "${release_paths[@]}" ||
    fail "tagged release code differs from origin/main"

workspace_version() {
    awk '
        /^\[workspace\.package\][[:space:]]*$/ { in_workspace = 1; next }
        /^\[/ { in_workspace = 0 }
        in_workspace && /^[[:space:]]*version[[:space:]]*=/ {
            value = $3
            gsub(/"/, "", value)
            print value
            exit
        }
    ' "${1:--}"
}

cargo_version=$(cargo metadata --no-deps --format-version 1 | node -e '
let input = "";
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => {
  const pkg = JSON.parse(input).packages.find(({ name }) => name === "north-daemon");
  if (!pkg) process.exit(1);
  process.stdout.write(pkg.version);
});') || fail "unable to read north CLI/daemon package version"
web_version=$(node -p "require('./apps/web/package.json').version") ||
    fail "unable to read web package version"
version=${release_tag#v}
[[ "$cargo_version" == "$version" && "$web_version" == "$version" ]] ||
    fail "tag version does not match Cargo CLI/daemon and web versions"
if ! git rev-parse --verify "$source_sha^1" >/dev/null 2>&1; then
    fail "tag target has no first parent"
fi
previous_cargo_version=$(git show "$source_sha^1:Cargo.toml" | workspace_version) ||
    fail "unable to read first-parent Cargo version"
previous_web_version=$(git show "$source_sha^1:apps/web/package.json" | node -e '
let input = "";
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => process.stdout.write(JSON.parse(input).version));
') || fail "unable to read first-parent web version"
[[ "$previous_cargo_version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ &&
    "$previous_web_version" == "$previous_cargo_version" ]] ||
    fail "first-parent Cargo and web versions are missing or mismatched"
tag_refs=$(git tag --list 'v*') || fail "unable to list fetched tag refs"
initial_release=true
while IFS= read -r existing_tag; do
    [[ -n "$existing_tag" ]] || continue
    [[ "$existing_tag" == "$release_tag" ]] && continue
    if [[ "$existing_tag" =~ $semver_re ]]; then
        initial_release=false
        break
    fi
done <<< "$tag_refs"
[[ "$previous_cargo_version" != "$version" || "$initial_release" == true ]] ||
    fail "tag target is not a first-parent version bump and another strict SemVer tag exists"
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf 'initial_release=%s\n' "$initial_release" >> "$GITHUB_OUTPUT"
fi

if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    {
        printf '## Tag source validation\n\n'
        printf -- '- Tag: %s\n' "$release_tag"
        printf -- '- Source commit: %s\n' "$source_sha"
        printf -- '- Cargo/CLI/daemon/web version: %s\n' "$version"
        printf -- '- First-parent version: %s\n' "$previous_cargo_version"
        printf -- '- Initial release (no other strict tag ref currently exists): %s\n' "$initial_release"
        printf -- '- Release code matches origin/main: yes\n'
    } >> "$GITHUB_STEP_SUMMARY"
fi
printf 'validated release tag: %s %s initial_release=%s\n' "$release_tag" "$source_sha" "$initial_release"
