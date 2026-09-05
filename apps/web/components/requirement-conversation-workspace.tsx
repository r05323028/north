"use client";

import type { FormEvent } from "react";
import { useState } from "react";

import { ApiError } from "@/lib/api/client";
import {
  ClarificationUnavailableError,
  cancelClarification,
  dispatchClarificationMessage,
  startClarification,
} from "@/lib/api/clarification";
import type {
  ActivityItem,
  ClarificationRun,
  CurrentUser,
  Message,
  ReadinessView,
} from "@/lib/api/contracts";
import { postRequesterMessage } from "@/lib/api/conversations";
import {
  clarificationIntent,
  composerMode,
  runStatusMessage,
  type ComposerMode,
} from "@/lib/clarification-intent";
import { editRequirement, type Requirement } from "@/lib/requirements";
import { useRequirementConversationWorkspace } from "@/lib/use-requirement-conversation-workspace";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { StatusBadge } from "@/components/ui/status";
import { Textarea } from "@/components/ui/textarea";
import { PageHeader } from "@/components/north-shell";

type StructuredDraft = {
  title: string;
  description: string;
  summary: string;
  acceptance_criteria: string[];
  assumptions: string[];
  open_questions: string[];
};

type IntentPhase =
  | "idle"
  | "saving_message"
  | "starting"
  | "dispatching"
  | "cancelling";

function draftFromRequirement(requirement: Requirement): StructuredDraft {
  return {
    title: requirement.title,
    description: requirement.description,
    summary: requirement.summary,
    acceptance_criteria: [...requirement.acceptance_criteria],
    assumptions: [...requirement.assumptions],
    open_questions: [...requirement.open_questions],
  };
}

function listText(values: string[]): string {
  return values.join("\n");
}

function parseList(value: string): string[] {
  return value
    .split("\n")
    .map((item) => item.trim())
    .filter(Boolean);
}

function safeActionError(cause: unknown, fallback: string): string {
  if (cause instanceof ApiError && cause.code) {
    return `${fallback} (${cause.code})`;
  }
  if (cause instanceof Error && cause.name === "InvalidServerDataError") {
    return "Server returned invalid data";
  }
  return fallback;
}

function connectionLabel(
  state: "connecting" | "connected" | "reconnecting" | "closed_or_error",
): string {
  switch (state) {
    case "connecting":
      return "Live updates: connecting";
    case "connected":
      return "Live updates: connected";
    case "reconnecting":
      return "Live updates: reconnecting";
    case "closed_or_error":
      return "Live updates: closed or unavailable";
  }
}

function messageAuthor(
  message: Message,
  currentUser: CurrentUser | null,
): string {
  if (message.kind === "agent") return "Agent";
  if (message.kind === "system") return "System";
  return message.author_user_id === currentUser?.id ? "You" : "Requester";
}

function listSection(items: string[], title: string) {
  return (
    <section className="grid gap-2">
      <h3 className="font-semibold">{title}</h3>
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

function runFailureMessage(
  cause: unknown,
  action: "start" | "dispatch" | "cancel",
): string {
  if (action === "start") {
    if (cause instanceof ApiError && cause.status === 409) {
      return "Clarification start conflicted with another change. Canonical state was refreshed.";
    }
    return safeActionError(cause, "Clarification could not start");
  }
  if (action === "dispatch") {
    if (
      cause instanceof ApiError &&
      (cause.status === 404 || cause.status === 409)
    ) {
      return "Message saved, but it was not sent to this clarification run.";
    }
    return safeActionError(cause, "Message was saved, but dispatch failed");
  }
  return safeActionError(cause, "Cancellation failed");
}

function ConversationPanel({
  requirement,
  run,
  messages,
  activities,
  currentUser,
  connectionState,
  resourceError,
  activityError,
  loading,
  loadingMore,
  loadingActivityMore,
  nextOffset,
  activityNextOffset,
  onLoadMore,
  onLoadMoreActivity,
  onRefresh,
  onApplyRun,
  onApplyRequirement,
}: {
  requirement: Requirement | null;
  run: ClarificationRun | null;
  messages: Message[];
  activities: ActivityItem[];
  currentUser: CurrentUser | null;
  connectionState:
    | "connecting"
    | "connected"
    | "reconnecting"
    | "closed_or_error";
  resourceError?: string;
  activityError?: string;
  loading: boolean;
  loadingMore: boolean;
  loadingActivityMore: boolean;
  nextOffset: number | null;
  activityNextOffset: number | null;
  onLoadMore: () => Promise<void>;
  onLoadMoreActivity: () => Promise<void>;
  onRefresh: () => Promise<void>;
  onApplyRun: (run: ClarificationRun) => void;
  onApplyRequirement: (requirement: Requirement) => void;
}) {
  const [body, setBody] = useState("");
  const [intentPhase, setIntentPhase] = useState<IntentPhase>("idle");
  const [composerError, setComposerError] = useState<string | null>(null);
  const [lastPersistedMessageId, setLastPersistedMessageId] = useState<
    string | null
  >(null);

  const intent = clarificationIntent(run);
  const mode: ComposerMode = composerMode(run);
  const blocked = intent.kind === "blocked";
  const busy = intentPhase !== "idle";
  const inputDisabled = busy || blocked || !requirement || loading;
  const hasCancellation = run !== null && run.phase !== "terminal";

  async function refreshAfterIntent() {
    try {
      await onRefresh();
    } catch {
      // The canonical hook keeps stale state and exposes refreshError.
    }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = body.trim();
    if (!trimmed) {
      setComposerError("Message cannot be empty");
      return;
    }
    if (!requirement) {
      setComposerError("Requirement is not loaded");
      return;
    }
    if (intent.kind === "blocked") {
      setComposerError(
        intent.reason === "awaiting_assignment"
          ? "Runtime assignment is pending. Retry the same clarification or cancel it."
          : "Cancellation is pending. Wait for canonical runtime completion.",
      );
      return;
    }

    const expectedIntent = intent;
    setComposerError(null);
    setIntentPhase("saving_message");
    let persisted: Message;
    try {
      persisted = await postRequesterMessage(requirement.id, trimmed);
    } catch (cause) {
      setComposerError(safeActionError(cause, "Message could not be saved"));
      setIntentPhase("idle");
      return;
    }

    setLastPersistedMessageId(persisted.id);
    setBody("");
    try {
      if (expectedIntent.kind === "start") {
        setIntentPhase("starting");
        const started = await startClarification(requirement.id, {
          message_id: persisted.id,
          expected_state_version: requirement.state_version,
        });
        onApplyRun(started);
      } else if (expectedIntent.kind === "dispatch") {
        setIntentPhase("dispatching");
        const dispatched = await dispatchClarificationMessage(
          requirement.id,
          expectedIntent.run_id,
          persisted.id,
        );
        onApplyRun(dispatched);
      }
    } catch (cause) {
      if (cause instanceof ClarificationUnavailableError) {
        onApplyRequirement(cause.requirement);
        onApplyRun(cause.run);
        setComposerError(
          "Runtime unavailable before assignment. Retry the same clarification or cancel it.",
        );
      } else {
        setComposerError(
          runFailureMessage(
            cause,
            expectedIntent.kind === "start" ? "start" : "dispatch",
          ),
        );
      }
    } finally {
      await refreshAfterIntent();
      setIntentPhase("idle");
    }
  }

  async function retryStart() {
    if (!requirement || !run || run.phase !== "awaiting_assignment") return;
    setComposerError(null);
    setIntentPhase("starting");
    const startMessageId = run.start_message_id;
    try {
      const retried = await startClarification(requirement.id, {
        message_id: startMessageId,
        expected_state_version: requirement.state_version,
      });
      onApplyRun(retried);
    } catch (cause) {
      if (cause instanceof ClarificationUnavailableError) {
        onApplyRequirement(cause.requirement);
        onApplyRun(cause.run);
        setComposerError(
          "Runtime is still unavailable. Same clarification remains available for explicit retry.",
        );
      } else {
        setComposerError(runFailureMessage(cause, "start"));
      }
    } finally {
      await refreshAfterIntent();
      setIntentPhase("idle");
    }
  }

  async function cancelRun() {
    if (!run || run.phase === "terminal" || !requirement) return;
    const targetRunId = run.run_id;
    setComposerError(null);
    setIntentPhase("cancelling");
    try {
      const cancelled = await cancelClarification(requirement.id, targetRunId);
      onApplyRun(cancelled);
    } catch (cause) {
      setComposerError(runFailureMessage(cause, "cancel"));
    } finally {
      await refreshAfterIntent();
      setIntentPhase("idle");
    }
  }

  const status = busy
    ? intentPhase === "saving_message"
      ? "Saving message…"
      : intentPhase === "starting"
        ? "Starting clarification…"
        : intentPhase === "dispatching"
          ? "Sending message to known run…"
          : "Cancellation pending…"
    : runStatusMessage(run);

  return (
    <Card data-testid="conversation-pane">
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <CardTitle id="conversation-heading">Conversation</CardTitle>
            <CardDescription>
              Durable requester, agent, and system messages.
            </CardDescription>
          </div>
          <span
            aria-label={connectionLabel(connectionState)}
            className="text-xs text-muted-foreground"
            role="status"
          >
            {connectionLabel(connectionState)}
          </span>
        </div>
      </CardHeader>
      <CardContent className="grid gap-5 pt-0">
        {resourceError && (
          <div className="grid gap-2" role="alert">
            <p className="text-sm text-destructive">{resourceError}</p>
            <Button
              aria-label="Retry conversation load"
              size="sm"
              type="button"
              variant="outline"
              onClick={() => void onRefresh()}
            >
              Retry conversation load
            </Button>
          </div>
        )}
        {loading && messages.length === 0 ? (
          <p role="status">Loading conversation…</p>
        ) : messages.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No conversation messages yet.
          </p>
        ) : (
          <ol aria-label="Conversation messages" className="grid gap-3">
            {messages.map((message) => (
              <li
                className="rounded-md border p-3"
                data-message-kind={message.kind}
                key={message.id}
              >
                <div className="mb-1 flex flex-wrap items-baseline gap-2 text-xs text-muted-foreground">
                  <strong className="text-foreground">
                    {messageAuthor(message, currentUser)}
                  </strong>
                  <time dateTime={message.created_at}>
                    {message.created_at}
                  </time>
                </div>
                <p className="whitespace-pre-wrap text-sm">{message.body}</p>
              </li>
            ))}
          </ol>
        )}
        {nextOffset !== null && (
          <Button
            disabled={loadingMore}
            size="sm"
            type="button"
            variant="outline"
            onClick={() => void onLoadMore()}
          >
            {loadingMore ? "Loading older messages…" : "Load older messages"}
          </Button>
        )}
        <section
          aria-labelledby="activity-heading"
          className="grid gap-2 border-t pt-4"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h3 className="font-semibold" id="activity-heading">
              Activity
            </h3>
            <span className="text-xs text-muted-foreground">
              Server-published progress
            </span>
          </div>
          {activityError && (
            <p className="text-sm text-destructive" role="alert">
              {activityError}
            </p>
          )}
          {activities.length === 0 ? (
            <p className="text-sm text-muted-foreground">No activity yet.</p>
          ) : (
            <ul aria-label="Activity updates" className="grid gap-2">
              {activities.map((activity) => (
                <li
                  className="grid gap-1 rounded-md bg-muted/40 p-2 text-sm"
                  key={activity.id}
                >
                  <span>{activity.activity}</span>
                  <time
                    className="text-xs text-muted-foreground"
                    dateTime={activity.created_at}
                  >
                    {activity.created_at}
                  </time>
                </li>
              ))}
            </ul>
          )}
          {activityNextOffset !== null && (
            <Button
              disabled={loadingActivityMore}
              size="sm"
              type="button"
              variant="outline"
              onClick={() => void onLoadMoreActivity()}
            >
              {loadingActivityMore
                ? "Loading more activity…"
                : "Load more activity"}
            </Button>
          )}
        </section>
        <section
          aria-labelledby="clarification-status-heading"
          className="grid gap-3 border-t pt-4"
        >
          <h3 className="font-semibold" id="clarification-status-heading">
            Clarification status
          </h3>
          <p aria-live="polite" className="text-sm" role="status">
            {status}
          </p>
          {composerError && (
            <p className="text-sm text-destructive" role="alert">
              {composerError}
            </p>
          )}
          {lastPersistedMessageId &&
            composerError &&
            composerError.includes("saved") && (
              <p className="text-xs text-muted-foreground">
                Saved message remains in canonical history.
              </p>
            )}
          {mode === "awaiting_assignment" && (
            <div className="flex flex-wrap gap-2">
              <Button
                aria-label="Retry clarification start"
                disabled={busy}
                type="button"
                onClick={() => void retryStart()}
              >
                Retry same clarification
              </Button>
              <Button
                aria-label="Cancel clarification"
                disabled={busy}
                type="button"
                variant="outline"
                onClick={() => void cancelRun()}
              >
                Cancel clarification
              </Button>
            </div>
          )}
          {hasCancellation && mode !== "awaiting_assignment" && (
            <Button
              aria-label={
                run?.cancel_requested
                  ? "Repeat cancellation"
                  : "Cancel clarification"
              }
              disabled={busy || !requirement}
              type="button"
              variant="outline"
              onClick={() => void cancelRun()}
            >
              {run?.cancel_requested
                ? "Repeat cancellation"
                : "Cancel clarification"}
            </Button>
          )}
          <form
            aria-label="Send clarification message"
            className="grid gap-2"
            onSubmit={(event) => void submit(event)}
          >
            <label htmlFor="clarification-message">Message</label>
            <Textarea
              aria-describedby="clarification-status-heading"
              disabled={inputDisabled}
              id="clarification-message"
              placeholder={
                blocked
                  ? "Submission is temporarily disabled"
                  : "Ask for clarification…"
              }
              value={body}
              onChange={(event) => setBody(event.target.value)}
            />
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="text-xs text-muted-foreground">
                Message saves before runtime intent.
              </span>
              <Button
                aria-label="Send clarification message"
                disabled={inputDisabled}
                type="submit"
              >
                {busy ? "Working…" : "Send message"}
              </Button>
            </div>
          </form>
        </section>
      </CardContent>
    </Card>
  );
}

function ReadinessPanel({ readiness }: { readiness: ReadinessView | null }) {
  return (
    <section
      aria-labelledby="readiness-heading"
      className="grid gap-3 border-t pt-5"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="font-semibold" id="readiness-heading">
          Readiness
        </h3>
        <span className="text-xs text-muted-foreground">
          Canonical assessment
        </span>
      </div>
      {!readiness ? (
        <p className="text-sm text-muted-foreground">
          No readiness assessment yet.
        </p>
      ) : (
        <div className="grid gap-3 text-sm">
          <dl className="grid gap-2 sm:grid-cols-2">
            <div>
              <dt className="text-muted-foreground">Verdict</dt>
              <dd>{readiness.verdict}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Current</dt>
              <dd>{readiness.current ? "Current" : "Stale"}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Outcome</dt>
              <dd>{readiness.outcome}</dd>
            </div>
            {readiness.rejection_reason && (
              <div>
                <dt className="text-muted-foreground">Rejection reason</dt>
                <dd>{readiness.rejection_reason}</dd>
              </div>
            )}
            <div>
              <dt className="text-muted-foreground">Assessment time</dt>
              <dd>
                <time dateTime={readiness.created_at}>
                  {readiness.created_at}
                </time>
              </dd>
            </div>
          </dl>
          {listSection(readiness.blockers, "Blockers")}
          {listSection(readiness.assumptions, "Assessment assumptions")}
          <section className="grid gap-2">
            <h4 className="font-semibold">Repositories reviewed</h4>
            {readiness.repositories_reviewed.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                No repository citation.
              </p>
            ) : (
              <ul className="grid gap-2 text-sm">
                {readiness.repositories_reviewed.map((repository) => (
                  <li
                    className="grid gap-1 rounded-md border p-2"
                    key={`${repository.repository_id}-${repository.commit_sha}`}
                  >
                    <span>Repository: {repository.repository_id}</span>
                    <span className="font-mono text-xs break-all">
                      Commit: {repository.commit_sha}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}
    </section>
  );
}

function RequirementEditor({
  draft,
  pending,
  error,
  reconcileRequired,
  onChange,
  onSave,
  onCancel,
  onReconcile,
}: {
  draft: StructuredDraft;
  pending: boolean;
  error: string | null;
  reconcileRequired: boolean;
  onChange: (draft: StructuredDraft) => void;
  onSave: () => Promise<void>;
  onCancel: () => void;
  onReconcile: () => void;
}) {
  const updateText = (
    field: "title" | "description" | "summary",
    value: string,
  ) => {
    onChange({ ...draft, [field]: value });
  };
  const updateList = (
    field: "acceptance_criteria" | "assumptions" | "open_questions",
    value: string,
  ) => {
    onChange({ ...draft, [field]: parseList(value) });
  };

  return (
    <form
      className="grid gap-4 border-t pt-4"
      onSubmit={(event) => {
        event.preventDefault();
        void onSave();
      }}
    >
      {error && (
        <p className="text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
      {reconcileRequired && (
        <div
          className="grid gap-2 rounded-md border border-destructive/50 p-3"
          role="alert"
        >
          <p className="text-sm">
            Requirement changed on server. Reconcile draft with canonical data
            before saving.
          </p>
          <Button
            aria-label="Use latest canonical requirement"
            size="sm"
            type="button"
            variant="outline"
            onClick={onReconcile}
          >
            Use latest canonical values
          </Button>
        </div>
      )}
      <label className="grid gap-2" htmlFor="requirement-edit-title">
        <span>Title</span>
        <Input
          id="requirement-edit-title"
          value={draft.title}
          onChange={(event) => updateText("title", event.target.value)}
        />
      </label>
      <label className="grid gap-2" htmlFor="requirement-edit-description">
        <span>Description</span>
        <Textarea
          id="requirement-edit-description"
          value={draft.description}
          onChange={(event) => updateText("description", event.target.value)}
        />
      </label>
      <label className="grid gap-2" htmlFor="requirement-edit-summary">
        <span>Summary</span>
        <Textarea
          id="requirement-edit-summary"
          value={draft.summary}
          onChange={(event) => updateText("summary", event.target.value)}
        />
      </label>
      <label className="grid gap-2" htmlFor="requirement-edit-criteria">
        <span>Acceptance criteria (one per line)</span>
        <Textarea
          id="requirement-edit-criteria"
          value={listText(draft.acceptance_criteria)}
          onChange={(event) =>
            updateList("acceptance_criteria", event.target.value)
          }
        />
      </label>
      <label className="grid gap-2" htmlFor="requirement-edit-assumptions">
        <span>Assumptions (one per line)</span>
        <Textarea
          id="requirement-edit-assumptions"
          value={listText(draft.assumptions)}
          onChange={(event) => updateList("assumptions", event.target.value)}
        />
      </label>
      <label className="grid gap-2" htmlFor="requirement-edit-questions">
        <span>Open questions (one per line)</span>
        <Textarea
          id="requirement-edit-questions"
          value={listText(draft.open_questions)}
          onChange={(event) => updateList("open_questions", event.target.value)}
        />
      </label>
      <div className="flex flex-wrap gap-2">
        <Button
          aria-label="Save requirement"
          disabled={pending || reconcileRequired}
          type="submit"
        >
          {pending ? "Saving…" : "Save requirement"}
        </Button>
        <Button
          aria-label="Cancel requirement edit"
          disabled={pending}
          type="button"
          variant="outline"
          onClick={onCancel}
        >
          Cancel
        </Button>
      </div>
    </form>
  );
}

function LiveRequirementPanel({
  requirement,
  readiness,
  currentUser,
  resourceError,
  onApplyRequirement,
  onRefresh,
}: {
  requirement: Requirement | null;
  readiness: ReadinessView | null;
  currentUser: CurrentUser | null;
  resourceError?: string;
  onApplyRequirement: (requirement: Requirement) => void;
  onRefresh: () => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<StructuredDraft | null>(null);
  const [editPending, setEditPending] = useState(false);
  const [editError, setEditError] = useState<string | null>(null);
  const [reconcileRequired, setReconcileRequired] = useState(false);

  function beginEdit() {
    if (
      !requirement ||
      requirement.status === "accepted" ||
      requirement.status === "rejected"
    )
      return;
    setDraft(draftFromRequirement(requirement));
    setEditError(null);
    setReconcileRequired(false);
    setEditing(true);
  }

  function cancelEdit() {
    setEditing(false);
    setEditError(null);
    setReconcileRequired(false);
    if (requirement) setDraft(draftFromRequirement(requirement));
  }

  async function saveEdit() {
    if (!requirement || !draft || reconcileRequired) return;
    setEditPending(true);
    setEditError(null);
    try {
      const canonical = await editRequirement(requirement.id, {
        expected_state_version: requirement.state_version,
        ...draft,
      });
      onApplyRequirement(canonical);
      setDraft(draftFromRequirement(canonical));
      setEditing(false);
      setReconcileRequired(false);
      await onRefresh();
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 409) {
        setEditError("Requirement changed. Draft retained for reconciliation.");
        setReconcileRequired(true);
        await onRefresh();
      } else if (
        cause instanceof ApiError &&
        (cause.status === 400 || cause.status === 403)
      ) {
        setEditError(
          requirement.status === "accepted" || requirement.status === "rejected"
            ? "Server refused edit of terminal Requirement."
            : safeActionError(cause, "Requirement edit was refused"),
        );
      } else {
        setEditError(safeActionError(cause, "Requirement edit failed"));
      }
    } finally {
      setEditPending(false);
    }
  }

  function reconcile() {
    if (!requirement) return;
    setDraft(draftFromRequirement(requirement));
    setReconcileRequired(false);
    setEditError(null);
  }

  return (
    <Card data-testid="live-requirement-panel">
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <CardTitle id="live-requirement-heading">
              Live Requirement
            </CardTitle>
            <CardDescription>
              Canonical structured state, readiness, and revision.
            </CardDescription>
          </div>
          {requirement && <StatusBadge status={requirement.status} />}
        </div>
      </CardHeader>
      <CardContent className="grid gap-5 pt-0">
        {resourceError && (
          <p className="text-sm text-destructive" role="alert">
            {resourceError}
          </p>
        )}
        {!requirement ? (
          <p role="status">Loading canonical Requirement…</p>
        ) : (
          <>
            <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
              <span>
                {currentUser
                  ? `Signed in as ${currentUser.email} · ${currentUser.role}`
                  : "Current user unavailable"}
              </span>
              {requirement.status === "accepted" ||
              requirement.status === "rejected" ? (
                <span>Terminal Requirement · server controls edits</span>
              ) : (
                <Button
                  aria-label="Edit requirement"
                  size="sm"
                  type="button"
                  variant="outline"
                  onClick={beginEdit}
                >
                  Edit structured content
                </Button>
              )}
            </div>
            <section
              className="grid gap-4"
              aria-labelledby="requirement-overview-heading"
            >
              <h3 className="font-semibold" id="requirement-overview-heading">
                Canonical fields
              </h3>
              <div className="grid gap-2">
                <h4 className="font-semibold">Title</h4>
                <p className="text-sm">{requirement.title}</p>
              </div>
              <div className="grid gap-2">
                <h4 className="font-semibold">Description</h4>
                <p className="whitespace-pre-wrap text-sm">
                  {requirement.description}
                </p>
              </div>
              <div className="grid gap-2">
                <h4 className="font-semibold">Summary</h4>
                <p className="whitespace-pre-wrap text-sm">
                  {requirement.summary || "No summary."}
                </p>
              </div>
              {listSection(
                requirement.acceptance_criteria,
                "Acceptance criteria",
              )}
              {listSection(requirement.assumptions, "Assumptions")}
              {listSection(requirement.open_questions, "Open questions")}
            </section>
            <dl className="grid gap-3 border-t pt-4 text-sm sm:grid-cols-2">
              <div>
                <dt className="text-muted-foreground">Created by</dt>
                <dd>{requirement.created_by}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Created</dt>
                <dd>
                  <time dateTime={requirement.created_at}>
                    {requirement.created_at}
                  </time>
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Updated</dt>
                <dd>
                  <time dateTime={requirement.updated_at}>
                    {requirement.updated_at}
                  </time>
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Revision</dt>
                <dd>{requirement.revision}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">State version</dt>
                <dd>{requirement.state_version}</dd>
              </div>
            </dl>
            {editing && draft && (
              <RequirementEditor
                draft={draft}
                error={editError}
                onCancel={cancelEdit}
                onChange={setDraft}
                onReconcile={reconcile}
                onSave={saveEdit}
                pending={editPending}
                reconcileRequired={reconcileRequired}
              />
            )}
            <ReadinessPanel readiness={readiness} />
          </>
        )}
      </CardContent>
    </Card>
  );
}

export function RequirementConversationWorkspace({ id }: { id: string }) {
  const workspace = useRequirementConversationWorkspace(id);
  const { conversation } = workspace;
  const messages: Message[] = conversation?.messages ?? [];

  return (
    <div className="grid gap-4 pt-4">
      <PageHeader
        actions={
          workspace.requirement ? (
            <StatusBadge status={workspace.requirement.status} />
          ) : undefined
        }
        description="Conversation stays durable; structured state stays canonical."
        eyebrow="Requirement workspace"
        title={workspace.requirement?.title ?? "Requirement workspace"}
      />
      {workspace.initialError && (
        <div
          className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-destructive/50 p-3"
          role="alert"
        >
          <span className="text-sm text-destructive">
            {workspace.initialError}
          </span>
          <Button
            aria-label="Retry workspace load"
            size="sm"
            type="button"
            variant="outline"
            onClick={() => void workspace.refresh()}
          >
            Retry workspace load
          </Button>
        </div>
      )}
      {workspace.refreshError && (
        <div
          className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-destructive/50 p-3"
          role="alert"
        >
          <span className="text-sm text-destructive">
            Refresh failed. Showing last successful canonical data.
          </span>
          <Button
            aria-label="Retry canonical refresh"
            size="sm"
            type="button"
            variant="outline"
            onClick={() => void workspace.refresh()}
          >
            Retry canonical refresh
          </Button>
        </div>
      )}
      <div className="grid items-start gap-4 lg:grid-cols-[minmax(0,1.45fr)_minmax(320px,0.85fr)]">
        <section aria-labelledby="conversation-heading" className="min-w-0">
          <ConversationPanel
            activities={workspace.activities}
            activityError={workspace.resourceErrors.activity}
            activityNextOffset={workspace.activity_next_offset}
            connectionState={workspace.connectionState}
            currentUser={workspace.currentUser}
            loading={workspace.loading}
            loadingActivityMore={workspace.loadingActivityMore}
            loadingMore={workspace.loadingConversationMore}
            messages={messages}
            nextOffset={conversation?.next_offset ?? null}
            onApplyRequirement={workspace.applyRequirement}
            onApplyRun={workspace.applyRun}
            onLoadMore={workspace.loadMoreConversation}
            onLoadMoreActivity={workspace.loadMoreActivity}
            onRefresh={workspace.refresh}
            requirement={workspace.requirement}
            resourceError={workspace.resourceErrors.conversation}
            run={workspace.run}
          />
        </section>
        <section aria-labelledby="live-requirement-heading" className="min-w-0">
          <LiveRequirementPanel
            currentUser={workspace.currentUser}
            onApplyRequirement={workspace.applyRequirement}
            onRefresh={workspace.refresh}
            readiness={workspace.readiness}
            requirement={workspace.requirement}
            resourceError={
              workspace.resourceErrors.requirement ??
              workspace.resourceErrors.readiness
            }
          />
        </section>
      </div>
    </div>
  );
}
