import fs from "node:fs";
import path from "node:path";

const [artifactPath, expectedSha, releaseRef] = process.argv.slice(2);

function fail(message) {
  console.error(`verify-release-artifact: ${message}`);
  process.exit(1);
}

if (!artifactPath || !expectedSha || !releaseRef) {
  fail("usage: node verify-release-artifact.mjs DIR SOURCE_SHA RELEASE_TAG");
}
if (
  !/^[0-9a-f]{40}$/.test(expectedSha) ||
  !/^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$/.test(releaseRef)
) {
  fail("release tag or source SHA is invalid");
}

const root = path.resolve(artifactPath);
const manifestPath = path.join(root, "manifest.json");
const checksumsPath = path.join(root, "checksums.sha256");
for (const file of [root, manifestPath, checksumsPath]) {
  let stat;
  try {
    stat = fs.lstatSync(file);
  } catch {
    fail(`missing release artifact entry: ${file}`);
  }
  if (file === root ? !stat.isDirectory() : !stat.isFile()) {
    fail(`release artifact entry has invalid type: ${file}`);
  }
}

let manifest;
try {
  manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
} catch {
  fail("manifest.json is invalid JSON");
}
const version = releaseRef.slice(1);
if (
  manifest.source_commit !== expectedSha ||
  manifest.version !== version ||
  manifest.server_version !== version ||
  manifest.daemon_version !== version ||
  manifest.web_version !== version ||
  manifest.target !== "x86_64-unknown-linux-gnu"
) {
  fail("manifest does not match release version, source SHA, versions, or target");
}
if (
  typeof manifest.previous_version !== "string" ||
  !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(manifest.previous_version) ||
  typeof manifest.version_changed !== "boolean" ||
  manifest.version_changed !== (manifest.previous_version !== version)
) {
  fail("manifest first-parent version metadata is invalid");
}

const checksums = fs.readFileSync(checksumsPath, "utf8").trimEnd().split("\n");
const expectedFiles = checksums.map((line) => {
  const match = /^[0-9a-f]{64}  (.+)$/.exec(line);
  if (!match || !match[1].startsWith("./")) {
    fail("checksum manifest has invalid format");
  }
  const parts = match[1].slice(2).split("/");
  if (parts.some((part) => !part || part === "." || part === "..")) {
    fail("checksum manifest contains an unsafe path");
  }
  return match[1];
});
if (new Set(expectedFiles).size !== expectedFiles.length) {
  fail("checksum manifest contains duplicate paths");
}

const actualFiles = [];
function collectFiles(directory, prefix = "") {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const relative = `${prefix}${entry.name}`;
    const file = path.join(directory, entry.name);
    if (entry.isSymbolicLink()) fail(`symlink is not allowed: ${relative}`);
    if (entry.isDirectory()) collectFiles(file, `${relative}/`);
    else if (entry.isFile()) {
      if (fs.lstatSync(file).nlink !== 1) {
        fail(`hard-linked artifact entry is not allowed: ${relative}`);
      }
      actualFiles.push(`./${relative}`);
    }
    else fail(`non-regular artifact entry: ${relative}`);
  }
}
collectFiles(root);
const payloadFiles = actualFiles.filter(
  (file) => file !== "./checksums.sha256",
);
if (
  JSON.stringify(payloadFiles.sort()) !==
  JSON.stringify(expectedFiles.sort())
) {
  fail("artifact payload does not match checksum inventory");
}
