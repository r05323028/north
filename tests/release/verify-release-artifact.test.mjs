import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const verifier = path.resolve("scripts/verify-release-artifact.mjs");
const sourceSha = "0123456789abcdef0123456789abcdef01234567";
const releaseTag = "v1.2.3";
const releaseVersion = releaseTag.slice(1);

function manifest(overrides = {}) {
  return {
    source_commit: sourceSha,
    version: releaseVersion,
    previous_version: "1.2.2",
    version_changed: true,
    server_version: releaseVersion,
    daemon_version: releaseVersion,
    web_version: releaseVersion,
    target: "x86_64-unknown-linux-gnu",
    ...overrides,
  };
}

function writeChecksums(root) {
  const paths = ["./manifest.json"];
  for (const entry of fs.readdirSync(path.join(root, "bin"), {
    withFileTypes: true,
  })) {
    if (entry.isFile()) paths.push(`./bin/${entry.name}`);
  }
  const lines = paths.sort().map((relative) => {
    const bytes = fs.readFileSync(path.join(root, relative.slice(2)));
    const digest = createHash("sha256").update(bytes).digest("hex");
    return `${digest}  ${relative}`;
  });
  fs.writeFileSync(
    path.join(root, "checksums.sha256"),
    `${lines.join("\n")}\n`,
  );
}

function runVerifier(root, sha = sourceSha, tag = releaseTag) {
  return spawnSync(process.execPath, [verifier, root, sha, tag], {
    encoding: "utf8",
  });
}

function verify(root) {
  const checksums = spawnSync(
    "shasum",
    ["-a", "256", "-c", "checksums.sha256"],
    { cwd: root, encoding: "utf8" },
  );
  return checksums.status === 0 ? runVerifier(root) : checksums;
}

function rejected(result) {
  assert.notEqual(result.status, 0, result.stderr || result.stdout);
}

test("release artifact verification fails closed", () => {
  const root = fs.mkdtempSync(
    path.join(os.tmpdir(), "north-release-verifier-"),
  );
  try {
    fs.mkdirSync(path.join(root, "bin"));
    fs.writeFileSync(path.join(root, "bin/north-server"), "server");
    fs.writeFileSync(path.join(root, "bin/north-daemon"), "daemon");
    fs.writeFileSync(
      path.join(root, "manifest.json"),
      `${JSON.stringify(manifest())}\n`,
    );
    writeChecksums(root);
    assert.equal(verify(root).status, 0);

    fs.writeFileSync(path.join(root, "bin/north-server"), "changed");
    rejected(verify(root));
    fs.writeFileSync(path.join(root, "bin/north-server"), "server");

    rejected(runVerifier(root, "f".repeat(40)));
    rejected(runVerifier(root, sourceSha, "v1.2.4"));
    for (const tag of [
      "v01.2.3",
      "v1.02.3",
      "v1.2.03",
      "v1.2.3-rc.1",
      "v1.2.3+build",
    ]) {
      rejected(runVerifier(root, sourceSha, tag));
    }
    fs.writeFileSync(
      path.join(root, "manifest.json"),
      `${JSON.stringify(manifest({ target: "aarch64-unknown-linux-gnu" }))}\n`,
    );
    writeChecksums(root);
    rejected(runVerifier(root));
    for (const overrides of [
      { version: "0.2.0" },
      { version_changed: false },
      { previous_version: "invalid" },
    ]) {
      fs.writeFileSync(
        path.join(root, "manifest.json"),
        `${JSON.stringify(manifest(overrides))}\n`,
      );
      writeChecksums(root);
      rejected(runVerifier(root));
    }
    fs.writeFileSync(
      path.join(root, "manifest.json"),
      `${JSON.stringify(manifest())}\n`,
    );
    writeChecksums(root);

    const checksumPath = path.join(root, "checksums.sha256");
    const validChecksums = fs.readFileSync(checksumPath, "utf8");
    const firstLine = validChecksums.split("\n")[0];
    fs.writeFileSync(checksumPath, `${firstLine}\n`);
    rejected(runVerifier(root));
    fs.writeFileSync(checksumPath, `${validChecksums}${firstLine}\n`);
    rejected(runVerifier(root));
    fs.writeFileSync(checksumPath, "malformed checksum\n");
    rejected(runVerifier(root));
    fs.writeFileSync(checksumPath, `${"0".repeat(64)}  /etc/passwd\n`);
    rejected(runVerifier(root));
    fs.writeFileSync(checksumPath, `${"0".repeat(64)}  ./../outside\n`);
    rejected(runVerifier(root));
    fs.writeFileSync(checksumPath, validChecksums);

    fs.writeFileSync(path.join(root, "unexpected"), "extra");
    rejected(verify(root));
    fs.rmSync(path.join(root, "unexpected"));

    fs.symlinkSync(
      path.join(root, "bin/north-server"),
      path.join(root, "bin/link"),
    );
    rejected(runVerifier(root));
    fs.rmSync(path.join(root, "bin/link"));

    fs.linkSync(
      path.join(root, "bin/north-server"),
      path.join(root, "bin/hardlink"),
    );
    writeChecksums(root);
    rejected(runVerifier(root));
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("package requires clean source and derives version change from first parent without a tag", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "north-release-package-"));
  const script = path.resolve("scripts/release.sh");
  const currentVersion = JSON.parse(
    fs.readFileSync("apps/web/package.json", "utf8"),
  ).version;
  const gitShim = [
    "git() {",
    "  case \"$*\" in",
    "    'rev-parse HEAD') printf '%s' \"$NORTH_TEST_SOURCE_SHA\" ;;",
    "    'rev-parse '*^1) printf '%s' \"$NORTH_TEST_PARENT_SHA\" ;;",
    "    'status --porcelain --untracked-files=all') printf '%s' \"$NORTH_TEST_GIT_STATUS\" ;;",
    "    'show '*:Cargo.toml) printf '%s' \"$NORTH_TEST_PARENT_CARGO\" ;;",
    "    'show '*:apps/web/package.json) printf '%s' \"$NORTH_TEST_PARENT_WEB\" ;;",
    "    *) printf '%s' \"unexpected git command: $*\" >&2; return 99 ;;",
    "  esac",
    "}",
    "export -f git",
    "exec \"$NORTH_TEST_RELEASE_SCRIPT\" package",
  ].join("\n");
  const baseEnv = { ...process.env };
  for (const key of [
    "NORTH_RELEASE_TAG",
    "NORTH_RELEASE_VERSION",
    "NORTH_RELEASE_SOURCE_SHA",
    "NORTH_RELEASE_TARGET",
    "NORTH_RELEASE_OUTPUT_DIR",
    "GITHUB_SHA",
  ]) {
    delete baseEnv[key];
  }
  baseEnv.NORTH_TEST_RELEASE_SCRIPT = script;
  const sourceSha = "0123456789012345678901234567890123456789";
  try {
    for (const [name, gitStatus, parentVersion, expectedChange] of [
      ["dirty", " M tracked-file", currentVersion, undefined],
      ["same-version", "", currentVersion, "version_changed=false"],
      ["version-bump", "", "0.0.9", "version_changed=true"],
    ]) {
      const output = path.join(root, name);
      const result = spawnSync("bash", ["-c", gitShim], {
        encoding: "utf8",
        env: {
          ...baseEnv,
          NORTH_TEST_GIT_STATUS: gitStatus,
          NORTH_TEST_SOURCE_SHA: sourceSha,
          NORTH_TEST_PARENT_SHA: "abcdef0123456789abcdef0123456789abcdef01",
          NORTH_TEST_PARENT_CARGO: `[workspace.package]\nversion = "${parentVersion}"\n`,
          NORTH_TEST_PARENT_WEB: JSON.stringify({ version: parentVersion }),
          NORTH_RELEASE_SOURCE_SHA: sourceSha,
          NORTH_RELEASE_TARGET: "north-test-target-not-installed",
          NORTH_RELEASE_OUTPUT_DIR: output,
        },
      });
      assert.equal(result.status, 2, result.stderr || result.stdout);
      if (name === "dirty") {
        assert.match(result.stderr, /worktree is not clean/);
      } else {
        assert.match(result.stderr, /Rust target is not installed/);
        assert.match(result.stdout, new RegExp(expectedChange));
      }
      assert.equal(fs.existsSync(output), false);
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
