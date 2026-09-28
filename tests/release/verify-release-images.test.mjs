import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("../../scripts/verify-release-images.mjs", import.meta.url));
const sourceCommit = "a".repeat(40);
const releaseRef = "v0.1.0";
const owner = "NorthExample";

function digest(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function makeBundle(versionChanged = true, archiveReference = `sha-${sourceCommit}`) {
  const root = mkdtempSync(join(tmpdir(), "north-release-images-"));
  const images = [];
  for (const service of ["north-server", "north-web"]) {
    const layout = join(root, service);
    mkdirSync(join(layout, "blobs", "sha256"), { recursive: true });
    const manifest = Buffer.from(JSON.stringify({ schemaVersion: 2, layers: [] }));
    const manifestDigest = digest(manifest);
    writeFileSync(join(layout, "oci-layout"), JSON.stringify({ imageLayoutVersion: "1.0.0" }));
    writeFileSync(join(layout, "index.json"), JSON.stringify({
      schemaVersion: 2,
      manifests: [{
        mediaType: "application/vnd.oci.image.manifest.v1+json",
        digest: `sha256:${manifestDigest}`,
        size: manifest.length,
        platform: { os: "linux", architecture: "amd64" },
        annotations: { "org.opencontainers.image.ref.name": archiveReference },
      }],
    }));
    writeFileSync(join(layout, "blobs", "sha256", manifestDigest), manifest);
    const archive = join(root, `${service}.oci.tar`);
    execFileSync("tar", ["-cf", archive, "-C", layout, "oci-layout", "index.json", "blobs"]);
    const archiveSha = digest(readFileSync(archive));
    images.push({
      service,
      image: `ghcr.io/northexample/${service}:sha-${sourceCommit}`,
      archive: `${service}.oci.tar`,
      manifest_digest: `sha256:${manifestDigest}`,
      archive_sha256: archiveSha,
    });
  }
  writeFileSync(join(root, "images.json"), JSON.stringify({
    version: "0.1.0",
    version_changed: versionChanged,
    sha_tag: `sha-${sourceCommit}`,
    source_commit: sourceCommit,
    target: "linux/amd64",
    images,
  }));
  writeFileSync(join(root, "checksums.sha256"), images.map((image) =>
    `${image.archive_sha256}  ${image.archive}`,
  ).join("\n") + "\n");
  return root;
}

function verify(directory, expectedCommit = sourceCommit, versionChanged = true) {
  return spawnSync(process.execPath, [script, directory, expectedCommit, releaseRef, String(versionChanged), owner], { encoding: "utf8" });
}

test("verifies exact OCI images, source, platform, manifest digest, and archives", () => {
  const bundle = makeBundle();
  try {
    const result = verify(bundle);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /verified OCI payload/);
  } finally {
    rmSync(bundle, { recursive: true, force: true });
  }
});

test("rejects OCI archives not named with the qualified SHA tag", () => {
  const bundle = makeBundle(true, "v0.1.0");
  try {
    const result = verify(bundle);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /OCI archive reference mismatch/);
  } finally {
    rmSync(bundle, { recursive: true, force: true });
  }
});

test("verifies first-parent version metadata independently from image tags", () => {
  const bundle = makeBundle(false);
  try {
    const result = verify(bundle, sourceCommit, false);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /sha-/);
    const mismatched = verify(bundle, sourceCommit, true);
    assert.notEqual(mismatched.status, 0);
  } finally {
    rmSync(bundle, { recursive: true, force: true });
  }
});

test("rejects modified OCI archive", () => {
  const bundle = makeBundle();
  try {
    writeFileSync(join(bundle, "north-web.oci.tar"), "tampered");
    const result = verify(bundle);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /checksum mismatch/);
  } finally {
    rmSync(bundle, { recursive: true, force: true });
  }
});

test("rejects metadata from another source revision", () => {
  const bundle = makeBundle();
  try {
    const result = verify(bundle, "b".repeat(40));
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /source, version, or target/);
  } finally {
    rmSync(bundle, { recursive: true, force: true });
  }
});
