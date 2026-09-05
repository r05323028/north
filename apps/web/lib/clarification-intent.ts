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
  | { kind: "blocked"; reason: "awaiting_assignment" | "cancellation_pending" };

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
  return { kind: "dispatch", run_id: run.run_id };
}

export function runStatusMessage(run: ClarificationRun | null): string {
  if (!run) return "Ready for clarification";
  if (run.phase === "awaiting_assignment") {
    return "Runtime assignment unavailable. Retry same clarification or cancel.";
  }
  if (run.phase === "active" && run.cancel_requested) {
    return "Cancellation pending. Wait for canonical runtime completion.";
  }
  if (run.phase === "active" && run.status === "unavailable") {
    return "Pinned runtime unavailable. Later messages remain bound to this run.";
  }
  if (run.phase === "active") return "Clarification active";
  if (run.cancel_requested && run.status === "completed") {
    return "Cancellation completed";
  }
  if (run.cancel_requested && run.status === "unavailable") {
    return "Cancellation failed or unavailable";
  }
  if (run.status === "completed") {
    return "Clarification completed. Readiness is assessed separately.";
  }
  return "Clarification failed or unavailable";
}
