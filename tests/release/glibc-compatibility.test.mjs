import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { compareVersions, verifyGlibcCompatibility } from "../../scripts/glibc-compatibility.mjs";

function withReadelf(output, status, run) {
  const directory = mkdtempSync(join(tmpdir(), "north-glibc-check-"));
  const binary = join(directory, "north");
  const readelf = join(directory, "readelf");
  writeFileSync(binary, "fixture");
  writeFileSync(
    readelf,
    '#!/bin/sh\nprintf "%s\\n" "$FAKE_READELF_OUTPUT"\nexit "$FAKE_READELF_STATUS"\n',
  );
  chmodSync(readelf, 0o755);
  const previous = {
    PATH: process.env.PATH,
    FAKE_READELF_OUTPUT: process.env.FAKE_READELF_OUTPUT,
    FAKE_READELF_STATUS: process.env.FAKE_READELF_STATUS,
  };
  process.env.PATH = `${directory}:${process.env.PATH ?? ""}`;
  process.env.FAKE_READELF_OUTPUT = output;
  process.env.FAKE_READELF_STATUS = String(status);
  try {
    return run(binary);
  } finally {
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    rmSync(directory, { recursive: true, force: true });
  }
}

test("compares GLIBC symbol versions numerically", () => {
  assert.equal(compareVersions("2.2.5", "2.31"), -1);
  assert.equal(compareVersions("2.31", "2.31.0"), 0);
  assert.equal(compareVersions("2.32", "2.31"), 1);
});

test("accepts required symbols at or below baseline", () => {
  const highest = withReadelf("Version needs: GLIBC_2.2.5 GLIBC_2.17 GLIBC_2.31", 0, (binary) =>
    verifyGlibcCompatibility(binary, "2.31"),
  );
  assert.equal(highest, "2.31");
});

test("rejects newer, missing, or unreadable GLIBC requirements", () => {
  assert.throws(
    () => withReadelf("GLIBC_2.32", 0, (binary) => verifyGlibcCompatibility(binary, "2.31")),
    /requires GLIBC_2.32, exceeds declared glibc baseline 2.31/,
  );
  assert.throws(
    () => withReadelf("no version requirements", 0, (binary) => verifyGlibcCompatibility(binary, "2.31")),
    /found no required GLIBC symbol versions/,
  );
  assert.throws(
    () => withReadelf("broken", 1, (binary) => verifyGlibcCompatibility(binary, "2.31")),
    /readelf failed/,
  );
});
