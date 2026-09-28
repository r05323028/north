import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const publisher = fileURLToPath(
  new URL("../../scripts/publish-release-images.sh", import.meta.url),
);
const sourceCommit = "a".repeat(40);
const owner = "NorthExample";
const ownerLower = owner.toLowerCase();
const shaTag = `sha-${sourceCommit}`;
const version = "0.1.0";
const manifest = JSON.stringify({ schemaVersion: 2, config: {} });
const manifestDigest = `sha256:${createHash("sha256").update(manifest).digest("hex")}`;

function registryKey(image) {
  return createHash("sha256").update(image).digest("hex");
}

function makeFixture(versionChanged = true) {
  const root = mkdtempSync(join(tmpdir(), "north-release-publish-"));
  const ociDir = join(root, "oci");
  const binDir = join(root, "bin");
  const registryDir = join(root, "registry");
  mkdirSync(ociDir);
  mkdirSync(binDir);
  mkdirSync(registryDir);
  for (const service of ["north-server", "north-web"]) {
    writeFileSync(join(ociDir, `${service}.oci.tar`), "archive");
  }
  writeFileSync(
    join(ociDir, "images.json"),
    JSON.stringify({
      version,
      version_changed: versionChanged,
      sha_tag: shaTag,
      source_commit: sourceCommit,
      images: ["north-server", "north-web"].map((service) => ({
        service,
        manifest_digest: manifestDigest,
      })),
    }),
  );
  const copyLog = join(root, "copies.log");
  const fakeSkopeo = join(binDir, "skopeo");
  writeFileSync(fakeSkopeo, [
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    "key() { printf '%s' \"$1\" | sha256sum | awk '{print $1}'; }",
    "for argument in \"$@\"; do remote=\"$argument\"; done",
    "remote=$(printf '%s' \"$remote\" | sed 's#^docker://##')",
    "if [[ \"$1\" == inspect ]]; then",
    "  file=\"$MOCK_REGISTRY_DIR/$(key \"$remote\")\"",
    "  if [[ -f \"$file\" ]]; then cat \"$file\"; else echo 'manifest unknown' >&2; exit 1; fi",
    "elif [[ \"$1\" == copy ]]; then",
    "  echo \"$remote\" >> \"$MOCK_COPY_LOG\"",
    "  if [[ \"${MOCK_FAIL_COPY_REMOTE:-}\" == \"$remote\" ]]; then echo 'injected copy failure' >&2; exit 1; fi",
    "  printf '%s' \"$MOCK_MANIFEST\" > \"$MOCK_REGISTRY_DIR/$(key \"$remote\")\"",
    "else",
    "  exit 2",
    "fi",
  ].join("\n"));
  chmodSync(fakeSkopeo, 0o755);
  return { root, ociDir, registryDir, binDir, copyLog };
}

function imageRef(service, tag) {
  return `ghcr.io/${ownerLower}/${service}:${tag}`;
}

function putRegistry(fixture, service, tag, bytes = manifest) {
  writeFileSync(
    join(fixture.registryDir, registryKey(imageRef(service, tag))),
    bytes,
  );
}

function run(fixture, targetTag, initialRelease = "false", failCopyRemote = "") {
  const args = [publisher, fixture.ociDir, owner];
  if (targetTag) args.push(targetTag);
  if (initialRelease !== null) args.push(initialRelease);
  return spawnSync("bash", args, {
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${fixture.binDir}:${process.env.PATH}`,
      MOCK_REGISTRY_DIR: fixture.registryDir,
      MOCK_COPY_LOG: fixture.copyLog,
      MOCK_MANIFEST: manifest,
      MOCK_FAIL_COPY_REMOTE: failCopyRemote,
    },
  });
}

function copyCount(fixture) {
  return existsSync(fixture.copyLog)
    ? readFileSync(fixture.copyLog, "utf8").trim().split(/\r?\n/).length
    : 0;
}

function cleanup(fixture) {
  rmSync(fixture.root, { recursive: true, force: true });
}

test("requires an explicit strict SemVer target before any registry write", () => {
  const fixture = makeFixture();
  try {
    for (const target of [undefined, shaTag]) {
      const result = run(fixture, target);
      assert.notEqual(result.status, 0);
      assert.equal(copyCount(fixture), 0);
    }
    for (const service of ["north-server", "north-web"]) {
      assert.equal(existsSync(join(fixture.registryDir, registryKey(imageRef(service, shaTag)))), false);
    }
  } finally {
    cleanup(fixture);
  }
});

test("requires a valid initial-release flag before registry writes", () => {
  const fixture = makeFixture(false);
  const semverTag = `v${version}`;
  try {
    const missing = run(fixture, semverTag, null);
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr, /usage:/);

    const malformed = run(fixture, semverTag, "TRUE");
    assert.notEqual(malformed.status, 0);
    assert.match(malformed.stderr, /initial-release flag must be true or false/);
    assert.equal(copyCount(fixture), 0);
  } finally {
    cleanup(fixture);
  }
});

test("accepts existing SemVer tags only when digest matches", () => {
  const fixture = makeFixture();
  const semverTag = `v${version}`;
  try {
    for (const service of ["north-server", "north-web"]) {
      putRegistry(fixture, service, semverTag);
    }
    const result = run(fixture, semverTag);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(copyCount(fixture), 0);
  } finally {
    cleanup(fixture);
  }
});

test("rejects conflicting SemVer digest", () => {
  const conflict = makeFixture();
  const semverTag = `v${version}`;
  try {
    putRegistry(conflict, "north-server", semverTag, "different manifest");
    const result = run(conflict, semverTag);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /different digest/);
  } finally {
    cleanup(conflict);
  }
});

test("publishes fresh tag-qualified OCI archives to matching SemVer refs", () => {
  const fixture = makeFixture();
  const semverTag = `v${version}`;
  try {
    const result = run(fixture, semverTag);
    assert.equal(result.status, 0, result.stderr);
    for (const service of ["north-server", "north-web"]) {
      assert.equal(readFileSync(join(fixture.registryDir, registryKey(imageRef(service, semverTag))), "utf8"), manifest);
      assert.equal(existsSync(join(fixture.registryDir, registryKey(imageRef(service, shaTag)))), false);
    }
    assert.equal(copyCount(fixture), 2);
  } finally {
    cleanup(fixture);
  }
});

test("preflights both SemVer destinations before copying either image", () => {
  const fixture = makeFixture();
  const semverTag = `v${version}`;
  try {
    putRegistry(fixture, "north-web", semverTag, "different manifest");
    const result = run(fixture, semverTag);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /different digest/);
    assert.equal(copyCount(fixture), 0);
    assert.equal(existsSync(join(fixture.registryDir, registryKey(imageRef("north-server", semverTag)))), false);
  } finally {
    cleanup(fixture);
  }
});

test("SemVer retry recovers partial publication without replacing matching image", () => {
  const fixture = makeFixture();
  const semverTag = `v${version}`;
  const webRef = imageRef("north-web", semverTag);
  try {
    const first = run(fixture, semverTag, "false", webRef);
    assert.notEqual(first.status, 0);
    assert.match(first.stderr, /injected copy failure/);
    const serverDigest = readFileSync(join(fixture.registryDir, registryKey(imageRef("north-server", semverTag))), "utf8");
    assert.equal(serverDigest, manifest);
    assert.equal(existsSync(join(fixture.registryDir, registryKey(webRef))), false);

    const retry = run(fixture, semverTag);
    assert.equal(retry.status, 0, retry.stderr);
    assert.equal(readFileSync(join(fixture.registryDir, registryKey(imageRef("north-server", semverTag))), "utf8"), serverDigest);
    assert.equal(readFileSync(join(fixture.registryDir, registryKey(webRef)), "utf8"), manifest);
    assert.equal(copyCount(fixture), 3);
  } finally {
    cleanup(fixture);
  }
});

test("rejects SemVer tag/version mismatch and non-bump artifact", () => {
  const mismatch = makeFixture();
  try {
    const result = run(mismatch, "v0.2.0");
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /target tag does not match OCI version/);
    assert.equal(copyCount(mismatch), 0);
  } finally {
    cleanup(mismatch);
  }

  const noBump = makeFixture(false);
  try {
    const rejected = run(noBump, `v${version}`, "false");
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /requires a first-parent version bump/);
    assert.equal(copyCount(noBump), 0);

    const initial = run(noBump, `v${version}`, "true");
    assert.equal(initial.status, 0, initial.stderr);
    assert.equal(copyCount(noBump), 2);
  } finally {
    cleanup(noBump);
  }
});

test("rejects SHA ref even when package version did not change", () => {
  const fixture = makeFixture(false);
  try {
    const result = run(fixture, shaTag);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /strict vX.Y.Z SemVer/);
    assert.equal(copyCount(fixture), 0);
  } finally {
    cleanup(fixture);
  }
});
