#!/usr/bin/env node
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import {
  createReadStream,
  lstatSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { basename, join } from "node:path";
import { verifyGlibcCompatibility } from "./glibc-compatibility.mjs";
import { tmpdir } from "node:os";

function fail(message) {
  throw new Error(message);
}

async function digest(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function main() {
  const [archive, sourceSha, version, target, executeArg] = process.argv.slice(2);
  if (!archive || !sourceSha || !version || !target || (executeArg && executeArg !== "--execute")) {
    fail("usage: verify-cli-archive.mjs ARCHIVE SOURCE_SHA VERSION TARGET [--execute]");
  }
  if (!/^[0-9a-f]{40}$/.test(sourceSha)) fail("invalid CLI source SHA");
  if (!/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(version)) {
    fail("invalid CLI version");
  }
  const platforms = {
    "x86_64-unknown-linux-gnu": "linux/amd64",
    "x86_64-apple-darwin": "darwin/amd64",
    "aarch64-apple-darwin": "darwin/arm64",
  };
  if (!platforms[target]) fail("unsupported CLI target");
  if (basename(archive) !== `north-cli-v${version}-${target}.tar.gz`) {
    fail("CLI archive filename does not match version and target");
  }
  if (!lstatSync(archive).isFile()) fail("CLI archive is not a regular file");

  const sidecar = readFileSync(`${archive}.sha256`, "utf8").trim();
  const sidecarMatch = /^([0-9a-f]{64})\s+(.+)$/.exec(sidecar);
  if (!sidecarMatch || sidecarMatch[2] !== basename(archive)) fail("invalid CLI archive SHA-256 sidecar");
  if ((await digest(archive)) !== sidecarMatch[1]) fail("CLI archive SHA-256 mismatch");

  const entries = execFileSync("tar", ["-tzf", archive], { encoding: "utf8" })
    .trim()
    .split(/\r?\n/)
    .map((entry) => entry.replace(/^\.\//, ""));
  const allowedEntries = ["checksums.sha256", "manifest.json", "north", "north-daemon"];
  if (
    entries.length !== allowedEntries.length ||
    new Set(entries).size !== entries.length ||
    entries.some((entry) => !allowedEntries.includes(entry))
  ) {
    fail("CLI archive contains missing or unexpected entries");
  }

  const directory = mkdtempSync(join(tmpdir(), "north-cli-verify-"));
  try {
    execFileSync("tar", ["-xzf", archive, "-C", directory]);
    for (const entry of allowedEntries) {
      if (!lstatSync(join(directory, entry)).isFile()) fail(`invalid CLI archive entry: ${entry}`);
    }
    const manifest = JSON.parse(readFileSync(join(directory, "manifest.json"), "utf8"));
    if (
      manifest.version !== version ||
      manifest.source_commit !== sourceSha ||
      manifest.target !== target ||
      manifest.platform !== platforms[target] ||
      manifest.glibc_baseline !== (target === "x86_64-unknown-linux-gnu" ? "2.31" : null) ||
      JSON.stringify(manifest.binaries) !== JSON.stringify(["north", "north-daemon"])
    ) {
      fail("CLI archive metadata does not match expected source, version, and target");
    }

    const checksums = new Map();
    for (const line of readFileSync(join(directory, "checksums.sha256"), "utf8").trim().split(/\r?\n/)) {
      const match = /^([0-9a-f]{64})\s+\*?(north|north-daemon|manifest\.json)$/.exec(line);
      if (!match || checksums.has(match[2])) fail("invalid CLI checksum manifest");
      checksums.set(match[2], match[1]);
    }
    if (checksums.size !== 3) fail("CLI checksum manifest is incomplete");
    for (const file of ["north", "north-daemon", "manifest.json"]) {
      if ((await digest(join(directory, file))) !== checksums.get(file)) {
        fail(`CLI checksum mismatch: ${file}`);
      }
    }

    if (target === "x86_64-unknown-linux-gnu") {
      verifyGlibcCompatibility(join(directory, "north"), "2.31");
      verifyGlibcCompatibility(join(directory, "north-daemon"), "2.31");
    }

    if (executeArg === "--execute") {
      for (const [file, name] of [["north", "north"], ["north-daemon", "north-daemon"]]) {
        const mode = lstatSync(join(directory, file)).mode;
        if ((mode & 0o111) === 0) fail(`${file} is not executable`);
        const result = spawnSync(join(directory, file), ["--version"], { encoding: "utf8" });
        if (result.error || result.status !== 0 || result.stdout.trim() !== `${name} ${version}`) {
          fail(`${file} version smoke check failed`);
        }
      }
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
  process.stdout.write(`verified CLI archive: ${version} ${target} ${sourceSha}\n`);
}

main().catch((error) => {
  process.stderr.write(`CLI archive verification failed: ${error.message}\n`);
  process.exitCode = 1;
});
