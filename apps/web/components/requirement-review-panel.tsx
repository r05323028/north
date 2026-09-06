"use client";

import { useState } from "react";

import { ApiError } from "@/lib/api/client";
import type { CurrentUser, ReviewPacket } from "@/lib/api/contracts";
import {
  acceptRequirementReview,
  rejectRequirementReview,
  reopenRequirement,
  requestRequirementChanges,
} from "@/lib/api/review";
import type { Requirement } from "@/lib/requirements";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

type ReviewAction = "accept" | "request_changes" | "reject" | "reopen";

type RequirementReviewPanelProps = {
  requirement: Requirement | null;
  reviewPacket: ReviewPacket | null;
  currentUser: CurrentUser | null;
  resourceError?: string;
  refreshing: boolean;
  onRefreshAction: () => Promise<void>;
};

function isReviewer(currentUser: CurrentUser | null): boolean {
  return (
    currentUser?.role === "RequirementManager" ||
    currentUser?.role === "Admin" ||
    currentUser?.role === "Owner"
  );
}

function packetIsCurrent(
  requirement: Requirement | null,
  packet: ReviewPacket | null,
): packet is ReviewPacket {
  return (
    requirement?.status === "ready" &&
    packet !== null &&
    packet.requirement_revision === requirement.revision &&
    packet.requirement_state_version === requirement.state_version
  );
}

function identityKey(
  requirement: Requirement | null,
  packet: ReviewPacket | null,
): string | null {
  if (requirement?.status === "ready" && packetIsCurrent(requirement, packet)) {
    return `ready:${requirement.state_version}:${packet.assessment_id}`;
  }
  if (requirement?.status === "rejected") {
    return `reopen:${requirement.state_version}`;
  }
  return null;
}

function reviewActionError(cause: unknown): string {
  if (cause instanceof ApiError) {
    if (cause.status === 409) {
      return "Review became stale. Canonical state was refreshed; acknowledge the refreshed review before retrying.";
    }
    if (cause.status === 401 || cause.status === 403) {
      return "You are not authorized to perform this review action.";
    }
    if (cause.status === 400) {
      return "Review action was refused by the server.";
    }
    if (cause.status === 404) {
      return "Requirement or review packet no longer exists.";
    }
  }
  if (cause instanceof Error && cause.name === "InvalidServerDataError") {
    return "Server returned invalid review data.";
  }
  return "Review action failed. Refresh and try again.";
}

function evidenceList(items: string[], title: string) {
  return (
    <section className="grid gap-2">
      <h4 className="font-semibold">{title}</h4>
      {items.length === 0 ? (
        <p className="text-sm text-muted-foreground">No entries.</p>
      ) : (
        <ul className="list-disc space-y-1 pl-5 text-sm">
          {items.map((item, index) => (
            <li key={`${title}-${index}`}>{item}</li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function RequirementReviewPanel({
  requirement,
  reviewPacket,
  currentUser,
  resourceError,
  refreshing,
  onRefreshAction,
}: RequirementReviewPanelProps) {
  const currentPacket = packetIsCurrent(requirement, reviewPacket)
    ? reviewPacket
    : null;
  const currentIdentity = identityKey(requirement, currentPacket);
  const [acknowledgement, setAcknowledgement] = useState<{
    identity: string | null;
    required: boolean;
  }>({ identity: null, required: false });
  const [invalidatedPacket, setInvalidatedPacket] =
    useState<ReviewPacket | null>(null);
  const [pendingAction, setPendingAction] = useState<ReviewAction | null>(null);
  const [feedback, setFeedback] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const [staleNotice, setStaleNotice] = useState(false);

  const acknowledgementRequired =
    acknowledgement.required ||
    (acknowledgement.identity !== null &&
      acknowledgement.identity !== currentIdentity);
  const acknowledgedIdentity =
    acknowledgement.identity === currentIdentity
      ? acknowledgement.identity
      : null;

  const reviewer = isReviewer(currentUser);
  const ready = requirement?.status === "ready";
  const rejected = requirement?.status === "rejected";
  const acknowledgementValid =
    !acknowledgementRequired || acknowledgedIdentity === currentIdentity;
  const packetUsable =
    currentPacket !== null &&
    invalidatedPacket !== currentPacket &&
    !refreshing;
  const reviewControlsDisabled =
    !reviewer ||
    pendingAction !== null ||
    refreshing ||
    !acknowledgementValid ||
    (ready && (!currentPacket || !packetUsable));

  function acknowledgeRefreshedState() {
    if (!currentIdentity) return;
    setAcknowledgement({ identity: currentIdentity, required: false });
    setStaleNotice(false);
    setActionError(null);
  }

  function markStale() {
    setAcknowledgement({ identity: null, required: true });
    setInvalidatedPacket(currentPacket);
    setStaleNotice(true);
  }

  async function refreshSafely() {
    try {
      await onRefreshAction();
    } catch {
      // The workspace retains canonical data and exposes refresh errors.
    }
  }

  async function submitAction(action: ReviewAction) {
    if (!requirement || pendingAction !== null || reviewControlsDisabled) {
      return;
    }

    const normalizedFeedback = feedback.trim();
    if (action === "request_changes") {
      if (!normalizedFeedback) {
        setActionError("Request Changes feedback cannot be empty.");
        return;
      }
      if (normalizedFeedback.length > 10_000) {
        setActionError("Request Changes feedback is too long.");
        return;
      }
    }

    setActionError(null);
    setPendingAction(action);
    try {
      if (action === "reopen") {
        await reopenRequirement(requirement.id, {
          expected_state_version: requirement.state_version,
        });
      } else {
        if (!currentPacket) return;
        const input = {
          assessment_id: currentPacket.assessment_id,
          expected_state_version: currentPacket.requirement_state_version,
        };
        if (action === "accept") {
          await acceptRequirementReview(requirement.id, input);
        } else if (action === "reject") {
          await rejectRequirementReview(requirement.id, input);
        } else {
          await requestRequirementChanges(requirement.id, {
            ...input,
            feedback: normalizedFeedback,
          });
          setFeedback("");
        }
        setStaleNotice(false);
      }
      await refreshSafely();
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 409) {
        markStale();
        setActionError(reviewActionError(cause));
        await refreshSafely();
      } else {
        setActionError(reviewActionError(cause));
      }
    } finally {
      setPendingAction(null);
    }
  }

  const acknowledgementLabel = ready
    ? "Review refreshed packet"
    : "Review refreshed Requirement";

  return (
    <section
      aria-labelledby="human-review-heading"
      className="grid gap-3 border-t pt-5"
      data-testid="human-review-panel"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="font-semibold" id="human-review-heading">
          Human review
        </h3>
        <span className="text-xs text-muted-foreground">
          Server-authoritative decision
        </span>
      </div>
      {resourceError && ready && !currentPacket && (
        <div className="grid gap-2" role="alert">
          <p className="text-sm text-destructive">{resourceError}</p>
          <Button
            aria-label="Retry review packet load"
            disabled={refreshing}
            size="sm"
            type="button"
            variant="outline"
            onClick={() => void refreshSafely()}
          >
            Retry review packet load
          </Button>
        </div>
      )}
      {(staleNotice || acknowledgementRequired) && currentIdentity && (
        <div
          className="grid gap-2 rounded-md border border-destructive/50 p-3"
          role="alert"
        >
          <p className="text-sm">
            {staleNotice
              ? "Review packet or Requirement changed. Old review action was not retried."
              : "Canonical review identity changed. Acknowledge refreshed state before deciding."}
          </p>
          {acknowledgementRequired && (
            <Button
              aria-label={acknowledgementLabel}
              disabled={refreshing || (ready && !currentPacket)}
              size="sm"
              type="button"
              onClick={acknowledgeRefreshedState}
            >
              {acknowledgementLabel}
            </Button>
          )}
        </div>
      )}
      {actionError && (
        <p aria-live="polite" className="text-sm text-destructive" role="alert">
          {actionError}
        </p>
      )}
      {ready && !currentPacket && !resourceError && (
        <p className="text-sm text-muted-foreground" role="status">
          Loading current review packet…
        </p>
      )}
      {ready && currentPacket && (
        <div className="grid gap-4 rounded-md border p-3">
          <p className="text-sm text-muted-foreground">
            Review current Requirement evidence before choosing a decision.
          </p>
          <dl className="grid gap-3 text-sm sm:grid-cols-2">
            <div>
              <dt className="text-muted-foreground">Goal</dt>
              <dd className="whitespace-pre-wrap">{currentPacket.goal}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Scope</dt>
              <dd className="whitespace-pre-wrap">{currentPacket.scope}</dd>
            </div>
            <div className="sm:col-span-2">
              <dt className="text-muted-foreground">Summary</dt>
              <dd className="whitespace-pre-wrap">{currentPacket.summary}</dd>
            </div>
          </dl>
          {evidenceList(
            currentPacket.acceptance_criteria,
            "Acceptance criteria",
          )}
          {evidenceList(currentPacket.assumptions, "Requirement assumptions")}
          {evidenceList(currentPacket.open_questions, "Open questions")}
          {evidenceList(currentPacket.blockers, "Assessment blockers")}
          {evidenceList(
            currentPacket.assessment_assumptions,
            "Assessment assumptions",
          )}
          <section className="grid gap-2">
            <h4 className="font-semibold">Repositories reviewed</h4>
            {currentPacket.repositories_reviewed.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                No repository citation.
              </p>
            ) : (
              <ul className="grid gap-2 text-sm">
                {currentPacket.repositories_reviewed.map((repository) => (
                  <li
                    className="grid gap-1 rounded-md border p-2"
                    key={`${repository.repository_id}-${repository.commit_sha}`}
                  >
                    <span>Repository: {repository.repository_id}</span>
                    <span className="break-all font-mono text-xs">
                      Commit: {repository.commit_sha}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>
          {reviewer ? (
            <div className="grid gap-3 border-t pt-3">
              <div className="flex flex-wrap gap-2">
                <Button
                  aria-label="Accept Requirement"
                  disabled={reviewControlsDisabled}
                  type="button"
                  onClick={() => void submitAction("accept")}
                >
                  {pendingAction === "accept" ? "Accepting…" : "Accept"}
                </Button>
                <Button
                  aria-label="Reject Requirement"
                  disabled={reviewControlsDisabled}
                  type="button"
                  variant="outline"
                  onClick={() => void submitAction("reject")}
                >
                  {pendingAction === "reject" ? "Rejecting…" : "Reject"}
                </Button>
              </div>
              <label className="grid gap-2" htmlFor="review-feedback">
                <span>Request Changes feedback</span>
                <Textarea
                  aria-describedby="review-feedback-help"
                  disabled={pendingAction !== null || refreshing}
                  id="review-feedback"
                  value={feedback}
                  onChange={(event) => setFeedback(event.target.value)}
                />
                <span
                  className="text-xs text-muted-foreground"
                  id="review-feedback-help"
                >
                  Explain changes needed. Feedback clears only after successful
                  submission.
                </span>
              </label>
              <Button
                aria-label="Request Changes"
                disabled={reviewControlsDisabled}
                type="button"
                variant="outline"
                onClick={() => void submitAction("request_changes")}
              >
                {pendingAction === "request_changes"
                  ? "Requesting changes…"
                  : "Request Changes"}
              </Button>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              Review actions are available to Requirement Manager, Admin, and
              Owner roles.
            </p>
          )}
        </div>
      )}
      {rejected && (
        <div className="grid gap-3 rounded-md border p-3">
          <p className="text-sm">
            Requirement is Rejected. Reopen it to return to clarification.
          </p>
          {reviewer ? (
            <Button
              aria-label="Reopen Requirement"
              disabled={reviewControlsDisabled}
              type="button"
              onClick={() => void submitAction("reopen")}
            >
              {pendingAction === "reopen" ? "Reopening…" : "Reopen"}
            </Button>
          ) : (
            <p className="text-sm text-muted-foreground">
              Reopen is available to Requirement Manager, Admin, and Owner
              roles.
            </p>
          )}
        </div>
      )}
      {!ready && !rejected && (
        <p className="text-sm text-muted-foreground">
          Human review actions appear when Requirement is Ready.
        </p>
      )}
    </section>
  );
}
