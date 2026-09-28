import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import {
  chmodSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const verifier = fileURLToPath(
  new URL("../../scripts/verify-cli-archive.mjs", import.meta.url),
);
const version = "0.1.0";
const sourceSha = "0123456789abcdef0123456789abcdef01234567";
const target = "x86_64-unknown-linux-gnu";
const archiveName = `north-cli-v${version}-${target}.tar.gz`;

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function makeFixture({ extraEntry = false, omitDaemon = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), "north-cli-archive-"));
  const archive = join(root, archiveName);
  const platform = "linux/amd64";
  const manifest = {
    version,
    source_commit: sourceSha,
    target,
    platform,
    binaries: ["north", "north-daemon"],
  };
  writeFileSync(join(root, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  writeFileSync(join(root, "north"), `#!/bin/sh\nprintf 'north ${version}\\n'\n`);
  writeFileSync(join(root, "north-daemon"), `#!/bin/sh\nprintf 'north-daemon ${version}\\n'\n`);
  chmodSync(join(root, "north"), 0o755);
  chmodSync(join(root, "north-daemon"), 0o755);
  if (extraEntry) writeFileSync(join(root, "unexpected"), "extra");
  const files = ["north", "north-daemon", "manifest.json"].filter(
    (file) => !(omitDaemon && file === "north-daemon"),
  );
  writeFileSync(
    join(root, "checksums.sha256"),
    files.map((file) => `${sha256(readFileSync(join(root, file)))}  ${file}\n`).join(""),
  );
  const entries = [...files, "checksums.sha256", ...(extraEntry ? ["unexpected"] : [])];
  if (omitDaemon) entries.splice(entries.indexOf("north-daemon"), 1);
  execFileSync("tar", ["-czf", archive, "-C", root, ...entries]);
  writeFileSync(`${archive}.sha256`, `${sha256(readFileSync(archive))}  ${archiveName}\n`);
  return { root, archive };
}

function run(fixture, args = []) {
  return spawnSync(
    process.execPath,
    [verifier, fixture.archive, sourceSha, version, target, ...args],
    { encoding: "utf8" },
  );
}

function cleanup(fixture) {
  rmSync(fixture.root, { recursive: true, force: true });
}

test("verifies extracted CLI pair, checksums, metadata, and native version smoke", () => {
  const fixture = makeFixture();
  try {
    const result = run(fixture, ["--execute"]);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /verified CLI archive/);
  } finally {
    cleanup(fixture);
  }
});

test("rejects unexpected archive entries and missing daemon binary", () => {
  for (const fixture of [makeFixture({ extraEntry: true }), makeFixture({ omitDaemon: true })]) {
    try {
      const result = run(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /CLI archive contains missing or unexpected entries/);
    } finally {
      cleanup(fixture);
    }
  }
});

test("rejects archive and metadata mismatches", () => {
  const fixture = makeFixture();
  try {
    writeFileSync(`${fixture.archive}.sha256`, `${"0".repeat(64)}  ${archiveName}\n`);
    const result = run(fixture);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /CLI archive SHA-256 mismatch/);

    const sourceMismatch = spawnSync(
      process.execPath,
      [verifier, fixture.archive, "f".repeat(40), version, target],
      { encoding: "utf8" },
    );
    assert.notEqual(sourceMismatch.status, 0);
  } finally {
    cleanup(fixture);
  }
});
