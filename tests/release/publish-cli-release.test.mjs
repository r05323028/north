import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmodSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { execFileSync, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const publisher = join(root, "scripts/publish-cli-release.sh");
const version = "0.1.0";
const tag = `v${version}`;
const sourceSha = "0123456789abcdef0123456789abcdef01234567";
const targets = {
  "x86_64-unknown-linux-gnu": "linux/amd64",
  "x86_64-apple-darwin": "darwin/amd64",
  "aarch64-apple-darwin": "darwin/arm64",
};

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function makeCliAssets(directory) {
  for (const [target, platform] of Object.entries(targets)) {
    const stage = mkdtempSync(join(directory, "stage-"));
    const files = ["north", "north-daemon"];
    for (const name of files) {
      writeFileSync(join(stage, name), `#!/bin/sh\n# ${name} ${target}\n`);
      chmodSync(join(stage, name), 0o755);
    }
    writeFileSync(
      join(stage, "manifest.json"),
      `${JSON.stringify({ version, source_commit: sourceSha, target, platform, glibc_baseline: target === "x86_64-unknown-linux-gnu" ? "2.31" : null, binaries: files }, null, 2)}\n`,
    );
    const checksummed = [...files, "manifest.json"];
    writeFileSync(
      join(stage, "checksums.sha256"),
      checksummed.map((name) => `${sha256(readFileSync(join(stage, name)))}  ${name}\n`).join(""),
    );
    const name = `north-cli-v${version}-${target}.tar.gz`;
    const archive = join(directory, name);
    execFileSync("tar", ["-czf", archive, "-C", stage, ...checksummed, "checksums.sha256"]);
    writeFileSync(`${archive}.sha256`, `${sha256(readFileSync(archive))}  ${name}\n`);
    rmSync(stage, { recursive: true, force: true });
  }
}

function makeFakeGh(bin, stateFile) {
  const gh = join(bin, "gh");
  writeFileSync(gh, `#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");
const args = process.argv.slice(2);
const [group, command, tag] = args;
const releaseTag = process.env.GH_FAKE_TAG;
const statePath = process.env.GH_FAKE_STATE;
const state = JSON.parse(fs.readFileSync(statePath, "utf8"));
const fail = (message) => { process.stderr.write(message + "\\n"); process.exit(1); };
const option = (name) => args[args.indexOf(name) + 1];
const save = () => fs.writeFileSync(statePath, JSON.stringify(state));
if (group === "api") {
  const endpoint = args.at(-1);
  if (endpoint === \`repos/owner/repo/git/ref/tags/\${releaseTag}\`) {
    state.tagRefCalls = (state.tagRefCalls ?? 0) + 1;
    if (Number(process.env.GH_FAKE_TAG_SHA_ON_CALL) === state.tagRefCalls) state.tagRef.object.sha = "f".repeat(40);
    save();
    process.stdout.write(JSON.stringify(state.tagRef));
  } else if (endpoint.startsWith("repos/owner/repo/git/tags/")) {
    const object = state.tagObjects?.[endpoint.split("/").at(-1)];
    if (!object) fail("tag object not found");
    process.stdout.write(JSON.stringify(object));
  } else if (endpoint === "repos/owner/repo/releases/tags/" + releaseTag) {
    const status = Number(process.env.GH_FAKE_RELEASE_STATUS ?? (state.release ? 200 : 404));
    const message = ({ 200: "OK", 404: "Not Found", 503: "Service Unavailable" })[status] ?? "Error";
    process.stdout.write("HTTP/2.0 " + status + " " + message + \"\\n\");
    if (status !== 200) process.exit(1);
  } else fail("unexpected GitHub API endpoint: " + endpoint);
} else if (group !== "release") fail("unexpected gh command");
else if (command === "view") {
  if (process.env.GH_FAKE_VIEW_ERROR) fail("temporary release view failure");
  if (!state.release || state.release.tagName !== tag) fail("release not found");
  const { tagName, targetCommitish, isDraft, assets } = state.release;
  process.stdout.write(JSON.stringify({ tagName, targetCommitish, isDraft, assets: Object.keys(assets).map((name) => ({ name })) }));
} else if (command === "create") {
  if (!args.includes("--verify-tag")) fail("release creation must verify existing tag");
  if (state.release) fail("release already exists");
  state.release = { tagName: tag, targetCommitish: option("--target"), isDraft: true, assets: {} };
  state.operations.push("create");
  save();
} else if (command === "upload") {
  const source = args[3];
  const name = path.basename(source);
  if (!state.release || state.release.assets[name]) fail("asset already exists");
  state.release.assets[name] = fs.readFileSync(source).toString("base64");
  state.operations.push("upload:" + name);
  save();
} else if (command === "download") {
  const name = option("--pattern");
  const directory = option("--dir");
  const data = state.release?.assets[name];
  if (!data) fail("asset not found");
  fs.mkdirSync(directory, { recursive: true });
  fs.writeFileSync(path.join(directory, name), Buffer.from(data, "base64"));
} else if (command === "edit") {
  if (!state.release || state.release.tagName !== tag) fail("release not found");
  state.release.isDraft = false;
  state.operations.push("publish");
  save();
} else fail("unexpected gh release command: " + command);
`);
  chmodSync(gh, 0o755);
  const readelf = join(bin, "readelf");
  writeFileSync(readelf, "#!/bin/sh\nprintf 'GLIBC_2.31\\n'\n");
  chmodSync(readelf, 0o755);
  writeFileSync(stateFile, JSON.stringify({
    release: null,
    operations: [],
    tagRef: { ref: `refs/tags/${tag}`, object: { sha: sourceSha, type: "commit" } },
    tagRefCalls: 0,
    tagObjects: {},
  }));
}

function makeFixture() {
  const temp = mkdtempSync(join(tmpdir(), "north-cli-release-"));
  const assets = join(temp, "assets");
  const bin = join(temp, "bin");
  const stateFile = join(temp, "gh-state.json");
  execFileSync("mkdir", ["-p", assets, bin]);
  makeCliAssets(assets);
  makeFakeGh(bin, stateFile);
  return { temp, assets, bin, stateFile };
}

function run(fixture, mode, extraEnv = {}) {
  return spawnSync("bash", [publisher, mode, tag, sourceSha, fixture.assets], {
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${fixture.bin}:${process.env.PATH}`,
      GITHUB_REPOSITORY: "owner/repo",
      GH_TOKEN: "test-token",
      GH_FAKE_TAG: tag,
      GH_FAKE_STATE: fixture.stateFile,
      ...extraEnv,
    },
  });
}

function state(fixture) {
  return JSON.parse(readFileSync(fixture.stateFile, "utf8"));
}

function cleanup(fixture) {
  rmSync(fixture.temp, { recursive: true, force: true });
}

test("draft asset publication retries byte-for-byte and finalizes explicitly", () => {
  const fixture = makeFixture();
  try {
    const first = run(fixture, "draft");
    assert.equal(first.status, 0, first.stderr);
    assert.equal(Object.keys(state(fixture).release.assets).length, 6);
    assert.equal(state(fixture).release.isDraft, true);

    const retry = run(fixture, "draft");
    assert.equal(retry.status, 0, retry.stderr);
    assert.equal(state(fixture).operations.filter((op) => op.startsWith("upload:")).length, 6);

    const finalize = run(fixture, "finalize");
    assert.equal(finalize.status, 0, finalize.stderr);
    assert.equal(state(fixture).release.isDraft, false);
    assert.equal(state(fixture).operations.filter((op) => op === "publish").length, 1);
  } finally {
    cleanup(fixture);
  }
});

test("does not create Release when lookup fails for reasons other than 404", () => {
  const fixture = makeFixture();
  try {
    const result = run(fixture, "draft", { GH_FAKE_VIEW_ERROR: "1", GH_FAKE_RELEASE_STATUS: "503" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /unable to inspect GitHub Release state/);
    assert.deepEqual(state(fixture).operations, []);
  } finally {
    cleanup(fixture);
  }
});

test("rechecks tag immediately before asset upload and finalization", () => {
  const beforeUpload = makeFixture();
  try {
    const result = run(beforeUpload, "draft", { GH_FAKE_TAG_SHA_ON_CALL: "3" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /GitHub tag resolves to .* expected source SHA/);
    assert.deepEqual(state(beforeUpload).operations, ["create"]);
    assert.equal(Object.keys(state(beforeUpload).release.assets).length, 0);
  } finally {
    cleanup(beforeUpload);
  }

  const beforeFinalize = makeFixture();
  try {
    const draft = run(beforeFinalize, "draft");
    assert.equal(draft.status, 0, draft.stderr);
    const moveAt = state(beforeFinalize).tagRefCalls + 2;
    const finalize = run(beforeFinalize, "finalize", { GH_FAKE_TAG_SHA_ON_CALL: String(moveAt) });
    assert.notEqual(finalize.status, 0);
    assert.match(finalize.stderr, /GitHub tag resolves to .* expected source SHA/);
    assert.equal(state(beforeFinalize).release.isDraft, true);
    assert.equal(state(beforeFinalize).operations.includes("publish"), false);
  } finally {
    cleanup(beforeFinalize);
  }
});

test("rejects wrong tag commit before creating or mutating a Release", () => {
  const fixture = makeFixture();
  try {
    const current = state(fixture);
    current.tagRef.object.sha = "f".repeat(40);
    writeFileSync(fixture.stateFile, JSON.stringify(current));

    const result = run(fixture, "draft");
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /GitHub tag resolves to .* expected source SHA/);
    assert.equal(state(fixture).release, null);
    assert.deepEqual(state(fixture).operations, []);
  } finally {
    cleanup(fixture);
  }
});

test("dereferences annotated tags and ignores Release targetCommitish", () => {
  const fixture = makeFixture();
  try {
    const current = state(fixture);
    const tagObjectSha = "a".repeat(40);
    current.tagRef.object = { sha: tagObjectSha, type: "tag" };
    current.tagObjects[tagObjectSha] = { object: { sha: sourceSha, type: "commit" } };
    writeFileSync(fixture.stateFile, JSON.stringify(current));

    const first = run(fixture, "draft");
    assert.equal(first.status, 0, first.stderr);
    const existing = state(fixture);
    existing.release.targetCommitish = "main";
    writeFileSync(fixture.stateFile, JSON.stringify(existing));

    const retry = run(fixture, "draft");
    assert.equal(retry.status, 0, retry.stderr);
  } finally {
    cleanup(fixture);
  }
});

test("rejects conflicting draft asset without overwriting it", () => {
  const fixture = makeFixture();
  try {
    const first = run(fixture, "draft");
    assert.equal(first.status, 0, first.stderr);
    const current = state(fixture);
    const assetName = Object.keys(current.release.assets)[0];
    current.release.assets[assetName] = Buffer.from("conflict").toString("base64");
    writeFileSync(fixture.stateFile, JSON.stringify(current));

    const retry = run(fixture, "draft");
    assert.notEqual(retry.status, 0);
    assert.match(retry.stderr, /conflicts with qualified CLI artifact/);
    assert.equal(state(fixture).release.assets[assetName], Buffer.from("conflict").toString("base64"));
    assert.equal(state(fixture).operations.filter((op) => op.startsWith("upload:")).length, 6);
  } finally {
    cleanup(fixture);
  }
});

test("refuses to finalize an incomplete draft", () => {
  const fixture = makeFixture();
  try {
    const draft = run(fixture, "draft");
    assert.equal(draft.status, 0, draft.stderr);
    const current = state(fixture);
    delete current.release.assets[Object.keys(current.release.assets)[0]];
    writeFileSync(fixture.stateFile, JSON.stringify(current));

    const finalize = run(fixture, "finalize");
    assert.notEqual(finalize.status, 0);
    assert.match(finalize.stderr, /missing CLI assets/);
    assert.equal(state(fixture).release.isDraft, true);
    assert.equal(state(fixture).operations.includes("publish"), false);
  } finally {
    cleanup(fixture);
  }
});

test("missing CLI artifact blocks draft creation", () => {
  const fixture = makeFixture();
  try {
    unlinkSync(join(fixture.assets, `north-cli-v${version}-aarch64-apple-darwin.tar.gz`));
    const result = run(fixture, "draft");
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /missing CLI archive or checksum/);
    assert.equal(state(fixture).release, null);
    assert.deepEqual(state(fixture).operations, []);
  } finally {
    cleanup(fixture);
  }
});
