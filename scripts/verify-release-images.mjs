#!/usr/bin/env node
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { createReadStream, lstatSync, readFileSync } from "node:fs";
import { join } from "node:path";

function fail(message) {
  throw new Error(message);
}

function archiveEntry(archive, entry) {
  return execFileSync("tar", ["-xOf", archive, entry], {
    maxBuffer: 1024 * 1024,
  });
}

function readMetadata(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    fail("invalid images.json");
  }
}

function readArchiveJson(archive, entry) {
  try {
    return JSON.parse(archiveEntry(archive, entry).toString("utf8"));
  } catch {
    fail(`invalid ${entry} in OCI archive`);
  }
}

async function fileDigest(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function main() {
  const [directory, expectedCommit, releaseRef, versionChangedArg, owner] = process.argv.slice(2);
  if (!directory || !expectedCommit || !releaseRef || !versionChangedArg || !owner) {
    fail("usage: verify-release-images.mjs <oci-dir> <source-sha> <version-ref> <version-changed> <owner>");
  }
  if (!/^[0-9a-f]{40}$/.test(expectedCommit)) fail("invalid source SHA");
  const tag = /^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.exec(releaseRef);
  if (!tag) fail("invalid release tag");

  const metadata = readMetadata(join(directory, "images.json"));
  if (versionChangedArg !== "true" && versionChangedArg !== "false") {
    fail("invalid first-parent version-change flag");
  }
  const versionChanged = versionChangedArg === "true";
  const shaTag = `sha-${expectedCommit}`;

  if (
    metadata.version !== tag.slice(1).join(".") ||
    metadata.version_changed !== versionChanged ||
    metadata.sha_tag !== shaTag ||
    metadata.source_commit !== expectedCommit ||
    metadata.target !== "linux/amd64"
  ) {
    fail("OCI metadata does not match release source, version, or target");
  }

  const expectedServices = ["north-server", "north-web"];
  if (
    !Array.isArray(metadata.images) ||
    metadata.images.length !== expectedServices.length ||
    expectedServices.some((service) => !metadata.images.some((image) => image.service === service))
  ) {
    fail("OCI metadata must contain exactly north-server and north-web");
  }

  const checksumLines = readFileSync(join(directory, "checksums.sha256"), "utf8").trim().split("\n");
  const checksums = new Map();
  for (const line of checksumLines) {
    const match = /^([0-9a-f]{64})\s+\*?(north-(?:server|web)\.oci\.tar)$/.exec(line);
    if (!match || checksums.has(match[2])) fail("invalid OCI checksum manifest");
    checksums.set(match[2], match[1]);
  }
  if (checksums.size !== expectedServices.length) fail("OCI checksum manifest is incomplete");

  for (const service of expectedServices) {
    const image = metadata.images.find((entry) => entry.service === service);
    const archiveName = `${service}.oci.tar`;
    const expectedImage = `ghcr.io/${owner.toLowerCase()}/${service}:${shaTag}`;
    if (
      image.image !== expectedImage ||
      image.archive !== archiveName ||
      !/^sha256:[0-9a-f]{64}$/.test(image.manifest_digest) ||
      !/^[0-9a-f]{64}$/.test(image.archive_sha256)
    ) {
      fail(`invalid OCI metadata for ${service}`);
    }

    const archive = join(directory, archiveName);
    const stat = lstatSync(archive);
    if (!stat.isFile() || stat.isSymbolicLink()) fail(`invalid OCI archive file: ${archiveName}`);
    const actualArchiveDigest = await fileDigest(archive);
    if (actualArchiveDigest !== image.archive_sha256 || checksums.get(archiveName) !== actualArchiveDigest) {
      fail(`OCI archive checksum mismatch: ${archiveName}`);
    }

    const layout = readArchiveJson(archive, "oci-layout");
    if (layout.imageLayoutVersion !== "1.0.0") fail(`invalid OCI layout: ${archiveName}`);
    const index = readArchiveJson(archive, "index.json");
    if (!Array.isArray(index.manifests) || index.manifests.length !== 1) {
      fail(`expected one OCI image manifest: ${archiveName}`);
    }
    const descriptor = index.manifests[0];
    if (descriptor.annotations?.["org.opencontainers.image.ref.name"] !== shaTag) {
      fail(`OCI archive reference mismatch: ${archiveName}`);
    }
    if (
      descriptor.digest !== image.manifest_digest ||
      descriptor.platform?.os !== "linux" ||
      descriptor.platform?.architecture !== "amd64"
    ) {
      fail(`OCI manifest digest or platform mismatch: ${archiveName}`);
    }
    const [, algorithm, encodedDigest] = /^(sha256):([0-9a-f]{64})$/.exec(descriptor.digest) ?? [];
    if (!algorithm) fail(`unsupported OCI manifest digest: ${archiveName}`);
    const manifest = archiveEntry(archive, `blobs/${algorithm}/${encodedDigest}`);
    if (`${algorithm}:${createHash(algorithm).update(manifest).digest("hex")}` !== descriptor.digest) {
      fail(`OCI manifest content digest mismatch: ${archiveName}`);
    }
  }
  process.stdout.write(`verified OCI payload: ${shaTag} linux/amd64\n`);
}

main().catch((error) => {
  process.stderr.write(`release OCI verification failed: ${error.message}\n`);
  process.exitCode = 1;
});
