import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const sourceRoot = fileURLToPath(new URL("../../", import.meta.url));
const builderImage = "rust:1.97.1-bullseye@sha256:02d78ca3f928195c2a907543de778adfd728ad7e2a24fdc6aef582b7c77842e0";

function executable(path, content) {
  writeFileSync(path, content);
  chmodSync(path, 0o755);
}

function makeFixture() {
  const temp = mkdtempSync(join(tmpdir(), "north-release-package-test-"));
  const root = join(temp, "repo");
  const bin = join(temp, "bin");
  mkdirSync(join(root, "scripts"), { recursive: true });
  mkdirSync(join(root, "apps/web"), { recursive: true });
  mkdirSync(bin);
  for (const script of [
    "release.sh",
    "verify-release-artifact.mjs",
    "verify-cli-archive.mjs",
    "glibc-compatibility.mjs",
  ]) {
    copyFileSync(join(sourceRoot, "scripts", script), join(root, "scripts", script));
  }
  writeFileSync(join(root, ".gitignore"), "target/\ndist/\n");
  writeFileSync(join(root, "apps/web/.gitignore"), ".next/\n");
  writeFileSync(join(root, "README.md"), "release source\n");
  writeFileSync(join(root, "Cargo.lock"), "# fixture\n");
  writeFileSync(join(root, "Cargo.toml"), '[workspace.package]\nversion = "0.0.9"\n');
  writeFileSync(join(root, "apps/web/package.json"), `${JSON.stringify({ version: "0.0.9" })}\n`);
  const git = (args) => spawnSync("git", args, { cwd: root, encoding: "utf8" });
  assert.equal(git(["init", "-q", "-b", "main"]).status, 0);
  assert.equal(git(["config", "user.name", "Release Test"]).status, 0);
  assert.equal(git(["config", "user.email", "release@example.invalid"]).status, 0);
  assert.equal(git(["add", "."]).status, 0);
  assert.equal(git(["commit", "-qm", "previous version"]).status, 0);
  writeFileSync(join(root, "Cargo.toml"), '[workspace.package]\nversion = "0.1.0"\n');
  writeFileSync(join(root, "apps/web/package.json"), `${JSON.stringify({ version: "0.1.0" })}\n`);
  assert.equal(git(["add", "."]).status, 0);
  assert.equal(git(["commit", "-qm", "release version"]).status, 0);
  const sourceSha = git(["rev-parse", "HEAD"]).stdout.trim();

  executable(join(bin, "cargo"), `#!/bin/bash
set -euo pipefail
printf '%s\\n' "$*" >> "$FAKE_CARGO_LOG"
if [[ " $* " == *" metadata "* ]]; then
  printf '%s\\n' '{"packages":[{"name":"north-server","version":"0.1.0"},{"name":"north-daemon","version":"0.1.0"}]}'
  exit 0
fi
if [[ " $* " != *" build "* ]]; then echo "unexpected cargo args: $*" >&2; exit 2; fi
build_args=$*
target=
while (($#)); do
  if [[ "$1" == --target ]]; then target=$2; shift 2; else shift; fi
done
out="$FAKE_REPO_ROOT/target/$target/release"
mkdir -p "$out"
make_binary() {
  local name=$1
  printf '#!/bin/sh\\nif [ "\${1:-}" = "--version" ]; then printf "%%s\\\\n" "%s 0.1.0"; fi\\n' "$name" > "$out/$name"
  chmod 0755 "$out/$name"
}
if [[ " $build_args " == *" -p north-server "* ]]; then make_binary north-server; fi
make_binary north-daemon
if [[ " $build_args " == *" --bin north "* ]]; then make_binary north; fi
if [[ "\${FAKE_CARGO_MUTATE:-0}" == 1 ]]; then printf 'mutated\\n' >> "$FAKE_REPO_ROOT/README.md"; fi
`);
  executable(join(bin, "rustup"), '#!/bin/sh\n[ "$1 $2 $3" = "target list --installed" ] && printf \'%s\\n\' "$FAKE_RUST_TARGET"\n');
  executable(join(bin, "getconf"), '#!/bin/sh\n[ "$1" = GNU_LIBC_VERSION ] && printf \'glibc 2.31\\n\'\n');
  executable(join(bin, "readelf"), '#!/bin/sh\nprintf \'Version needs: GLIBC_2.2.5 GLIBC_2.31\\n\'\n');
  executable(join(bin, "docker"), `#!/bin/bash
set -euo pipefail
while (($#)); do
  case "$1" in
    run|--rm|--platform=*) shift ;;
    --user|--volume|--workdir|--env) shift 2 ;;
    *) break ;;
  esac
done
[[ "$1" == "$NORTH_TEST_BUILDER_IMAGE" ]] || { echo "unexpected builder image: $1" >&2; exit 2; }
shift
exec "$@"
`);
  executable(join(bin, "npm"), '#!/bin/bash\nset -euo pipefail\n[[ "$1 $2" == "run build" ]] || exit 2\nmkdir -p .next/standalone .next/static\nprintf \'web\\n\' > .next/standalone/server.js\nprintf \'static\\n\' > .next/static/app.js\n');

  return { temp, root, bin, sourceSha, log: join(temp, "cargo.log") };
}

function run(fixture, command, mutate = false) {
  let target = "x86_64-unknown-linux-gnu";
  if (process.platform === "darwin") {
    target = process.arch === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin";
  }
  const output = join(fixture.temp, `${command}-output`);
  const env = {
    ...process.env,
    PATH: `${fixture.bin}:${process.env.PATH}`,
    CARGO_HOME: join(fixture.temp, "cargo-cache"),
    FAKE_CARGO_LOG: fixture.log,
    FAKE_REPO_ROOT: fixture.root,
    FAKE_RUST_TARGET: command === "package" ? "x86_64-unknown-linux-gnu" : target,
    NORTH_TEST_BUILDER_IMAGE: builderImage,
    NORTH_RELEASE_SOURCE_SHA: fixture.sourceSha,
    NORTH_RELEASE_OUTPUT_DIR: output,
    NORTH_CLI_OUTPUT_DIR: output,
    NORTH_RELEASE_TARGET: "x86_64-unknown-linux-gnu",
    ...(mutate ? { FAKE_CARGO_MUTATE: "1" } : {}),
  };
  const args = command === "package"
    ? [join(fixture.root, "scripts/release.sh"), "package"]
    : [join(fixture.root, "scripts/release.sh"), "cli-package", target];
  return { result: spawnSync("bash", args, { cwd: fixture.root, encoding: "utf8", env }), output, target };
}

function cleanup(fixture) {
  rmSync(fixture.temp, { recursive: true, force: true });
}

test("all release Cargo operations are lockfile-locked", () => {
  for (const path of ["scripts/release.sh", "scripts/validate-release-tag.sh"]) {
    const source = readFileSync(join(sourceRoot, path), "utf8");
    const commands = source.split("\n").filter((line) => /\bcargo\s+(?:metadata|build|test|check|run)\b/.test(line));
    assert.ok(commands.length > 0, `${path} has release Cargo commands`);
    for (const command of commands) assert.match(command, /\s--locked(?:\s|$)/, `${path}: ${command}`);
  }
});

test("server and CLI release package paths use locked Cargo and verify symbols after packaging", () => {
  for (const command of ["package", "cli-package"]) {
    const fixture = makeFixture();
    try {
      const { result, output, target } = run(fixture, command);
      assert.equal(result.status, 0, result.stderr || result.stdout);
      const cargo = readFileSync(fixture.log, "utf8");
      assert.match(cargo, /metadata --locked --no-deps/);
      assert.match(cargo, /build --locked --release/);
      if (command === "package") {
        assert.equal(JSON.parse(readFileSync(join(output, "manifest.json"), "utf8")).glibc_baseline, "2.31");
      } else {
        const archive = join(output, `north-cli-v0.1.0-${target}.tar.gz`);
        const entries = spawnSync("tar", ["-xOf", archive, "manifest.json"], { encoding: "utf8" });
        assert.equal(entries.status, 0, entries.stderr);
        assert.equal(JSON.parse(entries.stdout).glibc_baseline, target === "x86_64-unknown-linux-gnu" ? "2.31" : null);
      }
    } finally {
      cleanup(fixture);
    }
  }
});

test("both release package paths refuse to label Cargo-mutated source", () => {
  for (const command of ["package", "cli-package"]) {
    const fixture = makeFixture();
    try {
      const { result, output } = run(fixture, command, true);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /source worktree changed after Cargo build/);
      assert.equal(spawnSync("test", ["-e", join(output, "manifest.json")]).status, 1);
    } finally {
      cleanup(fixture);
    }
  }
});
