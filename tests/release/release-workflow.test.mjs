import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const workflow = readFileSync(`${root}/.github/workflows/release.yml`, "utf8");

function job(name) {
  const start = workflow.search(new RegExp(`^  ${name}:\\n`, "m"));
  assert.notEqual(start, -1, `missing release job: ${name}`);
  const bodyStart = start + `  ${name}:\n`.length;
  const nextJob = /\n  [a-z][a-z0-9-]*:\n/g;
  nextJob.lastIndex = bodyStart;
  const next = nextJob.exec(workflow);
  return workflow.slice(start, next?.index ?? workflow.length);
}

test("tag release stays private until qualified SemVer images publish", () => {
  const draft = job("create-cli-release-draft");
  const images = job("publish-semver-images");
  const finalize = job("finalize-cli-release");
  assert.match(draft, /needs: \[build-package, release-qualification, verify-cli-archives\]/);
  assert.match(images, /create-cli-release-draft/);
  assert.match(images, /release-qualification/);
  assert.match(finalize, /publish-semver-images/);
  assert.match(images, /release\/scripts\/publish-release-images\.sh/);
  assert.match(finalize, /release\/scripts\/publish-cli-release\.sh/);
});

test("only strict SemVer publisher can write OCI images to GHCR", () => {
  const qualification = job("release-qualification");
  const draft = job("create-cli-release-draft");
  const images = job("publish-semver-images");
  const finalize = job("finalize-cli-release");
  assert.doesNotMatch(workflow, /^  publish-images:/m);
  assert.doesNotMatch(workflow, /Publish immutable SHA tags to GHCR/);
  assert.doesNotMatch(qualification, /packages: write|skopeo login|bash release\/scripts\/publish-release-images\.sh/);
  assert.match(images, /if:.*refs\/tags\/v/);
  assert.match(images, /permissions:\n      packages: write/);
  assert.match(images, /publish-release-images\.sh[\s\S]*release\/oci[\s\S]*"\$INITIAL_RELEASE"/);
  assert.equal(workflow.split("bash release/scripts/publish-release-images.sh").length - 1, 1);
  assert.match(job("build-package"), /OCI ref \(internal, not pushed\)/);
  for (const publisher of [draft, images, finalize]) {
    assert.doesNotMatch(publisher, /actions\/checkout@/);
  }
  assert.doesNotMatch(images, /contents: write/);
  for (const releaseJob of [draft, finalize]) {
    assert.match(releaseJob, /permissions:\n      contents: write/);
    assert.doesNotMatch(releaseJob, /packages: write/);
  }
});

test("tag validation gates all tag builds before publisher credentials", () => {
  const ci = job("ci-qualification");
  const packageBuild = job("build-package");
  const cliBuild = job("build-cli-archives");
  const images = job("publish-semver-images");
  assert.match(ci, /Validate release tag before build/);
  assert.match(ci, /id: validate_release_tag/);
  assert.match(ci, /fetch-tags: true/);
  assert.match(ci, /initial_release: \$\{\{ steps\.validate_release_tag\.outputs\.initial_release \}\}/);
  assert.doesNotMatch(ci, /packages:\s*write|skopeo login|publish-release-images\.sh/);
  assert.match(packageBuild, /needs: ci-qualification/);
  assert.match(cliBuild, /needs: ci-qualification/);
  assert.match(images, /- ci-qualification/);
  assert.match(images, /INITIAL_RELEASE: \$\{\{ needs\.ci-qualification\.outputs\.initial_release \}\}/);
  assert.match(cliBuild, /release\.sh cli-package/);
});

test("hosted PKI cleanup attempts every removal even if one cleanup command fails", () => {
  const qualification = job("release-qualification");
  const cleanupStart = qualification.indexOf("- name: Remove temporary PKI trust and files");
  assert.notEqual(cleanupStart, -1);
  const cleanup = qualification.slice(cleanupStart);
  assert.match(cleanup, /if: always\(\)/);
  assert.match(cleanup, /cleanup_failed=0/);
  assert.ok(cleanup.includes('sudo rm -f "$trust_file" || cleanup_failed=1'));
  assert.ok(cleanup.includes('rm -rf "$RUNNER_TEMP/release-tls" || cleanup_failed=1'));
  assert.ok(cleanup.includes('sudo update-ca-certificates || cleanup_failed=1'));
  assert.ok(cleanup.includes('exit "$cleanup_failed"'));
});

test("concurrency serializes reruns by tag ref or source SHA", () => {
  const concurrencyExpression = [
    "  group: >-",
    "    release-${{ github.ref_type == 'tag' && github.ref ||",
    "    inputs.source_sha || github.sha }}",
  ].join("\n");
  assert.ok(workflow.includes(concurrencyExpression));
  assert.ok(workflow.includes("cancel-in-progress: false"));
});
