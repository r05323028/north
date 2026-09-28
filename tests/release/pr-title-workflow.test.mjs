import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const root = new URL("../../", import.meta.url);
const ci = readFileSync(new URL(".github/workflows/ci.yml", root), "utf8");
const title = readFileSync(new URL(".github/workflows/pr-title.yml", root), "utf8");

test("PR title edits rerun lightweight required check without rerunning full CI", () => {
  for (const event of ["opened", "synchronize", "reopened", "ready_for_review", "review_requested", "edited"]) {
    assert.match(title, new RegExp(`^\\s+- ${event}$`, "m"));
  }
  assert.match(title, /name: PR title \(Conventional Commit\)/);
  assert.match(title, /github\.event\.pull_request\.title/);
  assert.match(title, /check-commit-message\.sh/);
  assert.doesNotMatch(ci, /- edited/);
  assert.doesNotMatch(ci, /pr-title/);
});
