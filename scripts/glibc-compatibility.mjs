import { spawnSync } from "node:child_process";
import { basename, dirname, resolve } from "node:path";

export const LINUX_RELEASE_BUILDER_IMAGE =
  "rust:1.97.1-bullseye@sha256:02d78ca3f928195c2a907543de778adfd728ad7e2a24fdc6aef582b7c77842e0";

export function compareVersions(left, right) {
  const a = left.split(".").map(Number);
  const b = right.split(".").map(Number);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const difference = (a[index] ?? 0) - (b[index] ?? 0);
    if (difference !== 0) return Math.sign(difference);
  }
  return 0;
}

function readelfOutput(binary) {
  const readelfArgs = ["--version-info", "--wide", binary];
  const local = spawnSync("readelf", readelfArgs, { encoding: "utf8", maxBuffer: 10 * 1024 * 1024 });
  if (!local.error) {
    if (local.status !== 0) {
      throw new Error(`readelf failed for ${binary}: ${(local.stderr || "").trim()}`);
    }
    return local.stdout;
  }
  if (local.error.code !== "ENOENT") {
    throw new Error(`readelf could not inspect ${binary}: ${local.error.message}`);
  }

  const absolute = resolve(binary);
  const docker = spawnSync(
    "docker",
    [
      "run", "--rm", "--platform=linux/amd64",
      "--volume", `${dirname(absolute)}:/artifact:ro`,
      LINUX_RELEASE_BUILDER_IMAGE,
      "readelf", ...readelfArgs.slice(0, 2), `/artifact/${basename(absolute)}`,
    ],
    { encoding: "utf8", maxBuffer: 10 * 1024 * 1024 },
  );
  if (docker.error || docker.status !== 0) {
    const reason = docker.error?.message ?? (docker.stderr || "readelf failed").trim();
    throw new Error(`readelf unavailable and pinned Docker verification failed for ${binary}: ${reason}`);
  }
  return docker.stdout;
}

export function verifyGlibcCompatibility(binary, baseline) {
  if (!/^\d+(?:\.\d+)+$/.test(baseline)) {
    throw new Error(`invalid glibc baseline: ${baseline}`);
  }
  const output = readelfOutput(binary);
  const versions = [...new Set(
    [...output.matchAll(/\bGLIBC_(\d+(?:\.\d+)+)\b/g)].map((match) => match[1]),
  )];
  if (versions.length === 0) {
    throw new Error(`readelf found no required GLIBC symbol versions in ${binary}`);
  }
  const highest = versions.reduce((max, version) =>
    compareVersions(max, version) < 0 ? version : max,
  );
  if (compareVersions(highest, baseline) > 0) {
    throw new Error(`${binary} requires GLIBC_${highest}, exceeds declared glibc baseline ${baseline}`);
  }
  return highest;
}
