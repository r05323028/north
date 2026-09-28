import assert from "node:assert/strict";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { execFileSync, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const releaseFiles = [
  ".github/workflows/release.yml",
  "Dockerfile.server",
  "apps/web/Dockerfile",
  "docker-compose.yaml",
  "scripts/release.sh",
  "scripts/validate-release-tag.sh",
  "scripts/publish-cli-release.sh",
  "scripts/publish-release-images.sh",
  "scripts/verify-release-artifact.mjs",
  "scripts/verify-release-images.mjs",
  "scripts/verify-cli-archive.mjs",
  "scripts/glibc-compatibility.mjs",
];

function git(repository, ...args) {
  return execFileSync("git", args, { cwd: repository, encoding: "utf8" }).trim();
}

function makeFixture({
  previousVersion = "0.0.0",
  version = "0.1.0",
  webVersion = version,
  lightweight = false,
  staleReleaseCode = false,
  previousTag = "",
  otherBranchTag = false,
  secondTagOnReleaseCommit = false,
} = {}) {
  const temp = mkdtempSync(join(tmpdir(), "north-tag-validation-"));
  const repository = join(temp, "repo");
  mkdirSync(repository);
  git(repository, "init", "--initial-branch=main");
  git(repository, "config", "user.name", "Release Test");
  git(repository, "config", "user.email", "release-test@example.invalid");
  for (const directory of [".github/workflows", "apps/web", "crates/north-daemon/src", "scripts"]) {
    mkdirSync(join(repository, directory), { recursive: true });
  }
  writeFileSync(
    join(repository, "Cargo.toml"),
    `[workspace]\nmembers = ["crates/north-daemon"]\nresolver = "2"\n\n[workspace.package]\nversion = "${previousVersion}"\nedition = "2024"\n`,
  );
  writeFileSync(
    join(repository, "crates/north-daemon/Cargo.toml"),
    `[package]\nname = "north-daemon"\nversion.workspace = true\nedition.workspace = true\n`,
  );
  writeFileSync(join(repository, "crates/north-daemon/src/lib.rs"), "// release fixture\n");
  writeFileSync(join(repository, "apps/web/package.json"), JSON.stringify({ version: previousVersion }));
  for (const file of releaseFiles) {
    const destination = join(repository, file);
    mkdirSync(join(destination, ".."), { recursive: true });
    if (file.startsWith("scripts/")) {
      copyFileSync(join(root, file), destination);
    } else {
      writeFileSync(destination, `fixture: ${file}\n`);
    }
  }
  git(repository, "add", ".");
  git(repository, "commit", "-m", "previous release");
  if (previousTag) git(repository, "tag", previousTag);
  if (otherBranchTag) {
    git(repository, "switch", "-c", "other-release");
    writeFileSync(
      join(repository, "Cargo.toml"),
      `[workspace]\nmembers = ["crates/north-daemon"]\nresolver = "2"\n\n[workspace.package]\nversion = "0.2.0"\nedition = "2024"\n`,
    );
    writeFileSync(join(repository, "apps/web/package.json"), JSON.stringify({ version: "0.2.0" }));
    git(repository, "add", ".");
    git(repository, "commit", "-m", "side-branch release");
    git(repository, "tag", "-a", "v0.2.0", "-m", "side release");
    git(repository, "switch", "main");
  }

  writeFileSync(
    join(repository, "Cargo.toml"),
    `[workspace]\nmembers = ["crates/north-daemon"]\nresolver = "2"\n\n[workspace.package]\nversion = "${version}"\nedition = "2024"\n`,
  );
  writeFileSync(join(repository, "apps/web/package.json"), JSON.stringify({ version: webVersion }));
  writeFileSync(join(repository, "crates/north-daemon/src/lib.rs"), `// release version bump ${version}\\n`);
  git(repository, "add", ".");
  git(repository, "commit", "-m", "release version bump");
  const sourceSha = git(repository, "rev-parse", "HEAD");
  if (lightweight) git(repository, "tag", `v${version}`, sourceSha);
  else git(repository, "tag", "-a", `v${version}`, "-m", "release", sourceSha);
  if (secondTagOnReleaseCommit) {
    git(repository, "tag", "-a", "v0.2.0", "-m", "another strict tag", sourceSha);
  }

  if (staleReleaseCode) {
    writeFileSync(join(repository, "scripts/release.sh"), "#!/usr/bin/env bash\n# stale main release code\n");
    git(repository, "add", "scripts/release.sh");
    git(repository, "commit", "-m", "change release code after tag");
  }
  const mainSha = git(repository, "rev-parse", "HEAD");
  git(repository, "remote", "add", "origin", repository);
  git(repository, "update-ref", "refs/remotes/origin/main", mainSha);
  return { temp, repository, sourceSha, version };
}

function run(fixture, { tag = `v${fixture.version}`, sha = fixture.sourceSha } = {}) {
  return spawnSync("bash", [join(fixture.repository, "scripts/validate-release-tag.sh"), tag], {
    cwd: fixture.repository,
    encoding: "utf8",
    env: {
      ...process.env,
      GITHUB_SHA: sha,
      GITHUB_REF: `refs/tags/${tag}`,
      GITHUB_OUTPUT: join(fixture.temp, "github-output"),
    },
  });
}

function cleanup(fixture) {
  rmSync(fixture.temp, { recursive: true, force: true });
}

test("accepts annotated and lightweight tags at current main version bump", () => {
  for (const lightweight of [false, true]) {
    const fixture = makeFixture({ lightweight });
    try {
      const result = run(fixture);
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /validated release tag/);
    } finally {
      cleanup(fixture);
    }
  }
});

test("rejects malformed or mismatched event tags and commits", () => {
  const fixture = makeFixture();
  try {
    const malformed = run(fixture, { tag: "v01.0.0" });
    assert.notEqual(malformed.status, 0);
    assert.match(malformed.stderr, /strict vX.Y.Z SemVer/);

    const mismatch = run(fixture, { sha: "f".repeat(40) });
    assert.notEqual(mismatch.status, 0);
    assert.match(mismatch.stderr, /tag target does not match GITHUB_SHA/);
  } finally {
    cleanup(fixture);
  }
});

test("rejects stale release code and web version mismatch", () => {
  const stale = makeFixture({ staleReleaseCode: true });
  try {
    const result = run(stale);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /release code differs from origin\/main/);
  } finally {
    cleanup(stale);
  }

  const mismatch = makeFixture({ webVersion: "0.2.0" });
  try {
    const result = run(mismatch);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /does not match Cargo CLI\/daemon and web versions/);
  } finally {
    cleanup(mismatch);
  }
});

test("allows initial strict tag to reuse the first-parent version", () => {
  const fixture = makeFixture({ previousVersion: "0.1.0", version: "0.1.0" });
  try {
    const result = run(fixture);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /initial_release=true/);
    assert.equal(readFileSync(join(fixture.temp, "github-output"), "utf8"), "initial_release=true\n");
  } finally {
    cleanup(fixture);
  }
});

test("later release still requires first-parent version bump", () => {
  const fixture = makeFixture({
    previousVersion: "0.1.0",
    version: "0.2.0",
    previousTag: "v0.1.0",
  });
  try {
    const result = run(fixture);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /initial_release=false/);
    assert.equal(readFileSync(join(fixture.temp, "github-output"), "utf8"), "initial_release=false\n");
  } finally {
    cleanup(fixture);
  }
});

test("other strict tag refs block unchanged-version exception", () => {
  for (const extraTag of ["another branch", "same commit"]) {
    const fixture = makeFixture({
      previousVersion: "0.1.0",
      version: "0.1.0",
      otherBranchTag: extraTag === "another branch",
      secondTagOnReleaseCommit: extraTag === "same commit",
    });
    try {
      const result = run(fixture);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /another strict SemVer tag exists/);
    } finally {
      cleanup(fixture);
    }
  }
});
