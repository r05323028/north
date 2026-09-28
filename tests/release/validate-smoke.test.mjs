import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));

test("release qualification forwards package version-change metadata to OCI verifier", () => {
  const releaseScript = readFileSync(`${root}/scripts/release.sh`, "utf8");
  assert.ok(
    releaseScript.includes('version_changed=$(node -p "String(require(process.argv[1]).version_changed)" "$manifest")'),
    "qualification must read version_changed from package manifest",
  );
  assert.ok(
    releaseScript.includes('"$oci_dir" "$source_commit" "v$version" "$version_changed" "$NORTH_RELEASE_IMAGE_OWNER"'),
    "OCI verifier must receive version_changed before owner",
  );
});

test("OCI server image migrates fresh Compose database before startup", () => {
  const releaseScript = readFileSync(`${root}/scripts/release.sh`, "utf8");
  const databaseStart = releaseScript.indexOf("up --detach --wait postgres");
  const migration = releaseScript.indexOf("run --rm --no-deps north-server migrate");
  const stackStart = releaseScript.indexOf(
    'release_log "PASS compose-runtime-started project=$release_compose_project"',
  );
  assert.ok(databaseStart >= 0 && migration > databaseStart && stackStart > migration);
});

test("smoke refuses to fall back when release artifact is unset", () => {
  const env = { ...process.env };
  delete env.NORTH_RELEASE_ARTIFACT_DIR;
  delete env.NORTH_RELEASE_OCI_DIR;
  const result = spawnSync("bash", ["scripts/validate.sh", "smoke"], {
    cwd: root,
    env,
    encoding: "utf8",
  });

  assert.equal(result.status, 2, result.stderr || result.stdout);
  assert.match(result.stderr, /smoke requires NORTH_RELEASE_ARTIFACT_DIR/);
  assert.doesNotMatch(result.stderr, /release\.sh qualify/);
});
