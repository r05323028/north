import type { ClarificationRun } from "@/lib/api/contracts";

export type ComposerMode =
  | "idle"
  | "active"
  | "awaiting_assignment"
  | "cancellation_pending"
  | "completed"
  | "failed";

export type ClarificationIntent =
  | { kind: "start" }
  | { kind: "dispatch"; run_id: string }
  | {
      kind: "blocked";
      reason: "awaiting_assignment" | "cancellation_pending" | "retrying";
    };

export function composerMode(run: ClarificationRun | null): ComposerMode {
  if (!run) return "idle";
  if (run.phase === "awaiting_assignment") return "awaiting_assignment";
  if (run.phase === "active" && run.cancel_requested) {
    return "cancellation_pending";
  }
  if (run.phase === "active") return "active";
  return run.status === "completed" ? "completed" : "failed";
}

export function clarificationIntent(
  run: ClarificationRun | null,
): ClarificationIntent {
  if (!run || run.phase === "terminal") return { kind: "start" };
  if (run.phase === "awaiting_assignment") {
    return { kind: "blocked", reason: "awaiting_assignment" };
  }
  if (run.cancel_requested) {
    return { kind: "blocked", reason: "cancellation_pending" };
  }
  if (run.retrying || run.status === "retrying") {
    return { kind: "blocked", reason: "retrying" };
  }
  return { kind: "dispatch", run_id: run.run_id };
}

export function runStatusMessage(run: ClarificationRun | null): string {
  if (!run) return "Ready for clarification";
  if (run.phase === "awaiting_assignment") {
    return "Runtime assignment unavailable. Retry same clarification or cancel.";
  }
  if (run.phase === "active") {
    if (run.cancel_requested) {
      return "Cancellation pending. Wait for canonical runtime completion.";
    }
    if (run.status === "unavailable") {
      return "Pinned runtime unavailable. Later messages remain bound to this run.";
    }
    if (run.retrying || run.status === "retrying") {
      return `Clarification retrying. Attempt ${run.attempt_count} is waiting for next run.`;
    }
    return "Clarification active";
  }
  if (run.cancel_requested) {
    return run.status === "completed"
      ? "Cancellation completed"
      : "Cancellation failed or unavailable";
  }
  if (run.status === "completed") {
    return "Clarification completed. Readiness is assessed separately.";
  }
  if (run.failed && run.failure_reason) {
    return `Clarification failed: ${run.failure_reason}. Start new clarification to retry.`;
  }
  return "Clarification failed or unavailable";
}
