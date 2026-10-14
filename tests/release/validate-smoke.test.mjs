import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));

test("Linux package uses pinned builder without requiring host Rust target", () => {
  const releaseScript = readFileSync(`${root}/scripts/release.sh`, "utf8");
  assert.ok(
    releaseScript.includes(
      'if [[ "$target" != "x86_64-unknown-linux-gnu" ]] && ! rustup target list --installed | grep -Fx "$target" >/dev/null; then',
    ),
    "Linux target is provided by the pinned builder, not host rustup",
  );
});

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

test("OCI server image migrates a fresh Compose database during startup", () => {
  const releaseScript = readFileSync(`${root}/scripts/release.sh`, "utf8");
  const composeStart = releaseScript.indexOf("release_compose_start() {");
  const composeEnd = releaseScript.indexOf('\n}\n\nif [[ -n "$artifact_dir" ]]', composeStart);
  assert.ok(composeStart >= 0 && composeEnd > composeStart);
  const compose = releaseScript.slice(composeStart, composeEnd);
  const databaseStart = compose.indexOf("up --detach --wait postgres");
  const stackStart = compose.indexOf("up --detach --wait", databaseStart + 1);
  assert.ok(databaseStart >= 0 && stackStart > databaseStart);
  assert.doesNotMatch(compose.slice(databaseStart, stackStart), /north-server migrate/);
  assert.ok(compose.includes("SELECT version FROM seaql_migrations"));
  assert.ok(compose.includes("m0001_initial_schema"));
});

test("release qualification runs migration checks against extracted server binary", () => {
  const releaseScript = readFileSync(`${root}/scripts/release.sh`, "utf8");
  const migrationTest = readFileSync(`${root}/crates/north-server/tests/fresh_install.rs`, "utf8");
  assert.ok(releaseScript.includes('NORTH_TEST_RELEASE_SERVER_BINARY="$server_bin"'));
  assert.ok(migrationTest.includes('env::var_os("NORTH_TEST_RELEASE_SERVER_BINARY")'));
  assert.ok(migrationTest.includes("async fn server_startup_migrates_fresh_schema_before_listening"));
  assert.ok(migrationTest.includes("async fn server_startup_without_ddl_privilege_fails_before_listening"));
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
