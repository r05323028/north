import { describe, expect, it } from "vitest";

import {
  clarificationIntent,
  composerMode,
  runStatusMessage,
} from "@/lib/clarification-intent";
import type { ClarificationRun } from "@/lib/api/contracts";

function run(overrides: Partial<ClarificationRun> = {}): ClarificationRun {
  return {
    run_id: "run-a",
    requirement_id: "requirement-1",
    start_message_id: "message-1",
    phase: "active",
    status: "running",
    cancel_requested: false,
    attempt_count: 1,
    next_retry_at: null,
    failure_reason: null,
    retrying: false,
    failed: false,
    created_at: "now",
    updated_at: "now",
    last_activity_at: "now",
    ...overrides,
  };
}

describe("clarification composer intent", () => {
  it("starts without a run and dispatches only to known active run", () => {
    expect(clarificationIntent(null)).toEqual({ kind: "start" });
    expect(clarificationIntent(run({ run_id: "run-a" }))).toEqual({
      kind: "dispatch",
      run_id: "run-a",
    });
    expect(clarificationIntent(run({ run_id: "run-b" }))).not.toEqual({
      kind: "dispatch",
      run_id: "run-a",
    });
  });

  it("blocks awaiting assignment and cancellation-pending slots", () => {
    expect(clarificationIntent(run({ phase: "awaiting_assignment" }))).toEqual({
      kind: "blocked",
      reason: "awaiting_assignment",
    });
    expect(clarificationIntent(run({ cancel_requested: true }))).toEqual({
      kind: "blocked",
      reason: "cancellation_pending",
    });
  });

  it("blocks dispatch while retry is waiting", () => {
    const retrying = run({
      status: "retrying",
      retrying: true,
      attempt_count: 2,
      next_retry_at: "2026-01-03T00:00:05Z",
    });
    expect(clarificationIntent(retrying)).toEqual({
      kind: "blocked",
      reason: "retrying",
    });
    expect(runStatusMessage(retrying)).toContain("Attempt 2");
  });

  it("allows a new start only after terminal outcome", () => {
    expect(clarificationIntent(run({ phase: "terminal" }))).toEqual({
      kind: "start",
    });
    expect(composerMode(run({ phase: "terminal", status: "completed" }))).toBe(
      "completed",
    );
    expect(
      composerMode(run({ phase: "terminal", status: "unavailable" })),
    ).toBe("failed");
    expect(
      runStatusMessage(run({ phase: "terminal", status: "completed" })),
    ).toContain("Readiness");
    expect(
      runStatusMessage(
        run({
          phase: "terminal",
          status: "failed",
          failed: true,
          failure_reason: "retry_exhausted",
        }),
      ),
    ).toContain("retry_exhausted");
  });
});
