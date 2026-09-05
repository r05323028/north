import { describe, expect, it } from "vitest";

import {
  parseActivityPage,
  parseClarificationMutationResponse,
  parseClarificationRun,
  parseConversationPage,
  parseCurrentUser,
  parseMessage,
  parseReadinessResponse,
  parseSessionResponse,
} from "@/lib/api/contracts";

const message = {
  id: "message-1",
  conversation_id: "conversation-1",
  author_user_id: "user-1",
  kind: "requester",
  body: "Need clarification",
  created_at: "2026-01-01T00:00:00Z",
};

const run = {
  run_id: "run-1",
  requirement_id: "requirement-1",
  start_message_id: "message-1",
  phase: "active",
  status: "running",
  cancel_requested: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
};

const assessment = {
  id: "assessment-1",
  event_id: "event-1",
  session_id: "run-1",
  daemon_event_seq: 1,
  requirement_revision: 1,
  verdict: "needs_clarification",
  blockers: ["scope"],
  assumptions: [],
  repositories_reviewed: [
    { repository_id: "repo-1", commit_sha: "a".repeat(40) },
  ],
  outcome: "rejected",
  rejection_reason: "scope",
  assessed_at_ms: 1,
  accepted_state_version: null,
  created_at: "2026-01-01T00:00:00Z",
  current: false,
};

describe("shared browser contracts", () => {
  it("accepts current snake_case conversation and session projections", () => {
    const page = parseConversationPage({
      id: "conversation-1",
      requirement_id: "requirement-1",
      created_at: "2026-01-01T00:00:00Z",
      messages: [message],
      next_offset: 1,
    });
    expect(page.messages[0]).toEqual(message);
    expect(parseSessionResponse({ session: run })).toEqual(run);
    expect(parseClarificationMutationResponse({ session: run })).toEqual(run);
  });

  it.each([
    [
      "message kind",
      () => parseMessage({ ...message, kind: "clarification_question" }),
    ],
    ["run phase", () => parseClarificationRun({ ...run, phase: "queued" })],
    ["run status", () => parseClarificationRun({ ...run, status: "failed" })],
    [
      "role",
      () =>
        parseCurrentUser({ id: "u", email: "u@example.com", role: "Member" }),
    ],
    [
      "readiness verdict",
      () =>
        parseReadinessResponse({
          assessment: { ...assessment, verdict: "maybe" },
        }),
    ],
  ])("rejects unknown closed enum values (%s)", (_name, parse) => {
    expect(parse).toThrow(/invalid/i);
  });

  it("rejects malformed arrays, identity mismatches, and unsafe offsets", () => {
    expect(() =>
      parseConversationPage({
        id: "conversation-1",
        requirement_id: "requirement-1",
        created_at: "now",
        messages: [{ ...message, conversation_id: "other" }],
        next_offset: null,
      }),
    ).toThrow(/messages/);
    expect(() =>
      parseActivityPage({
        activities: [],
        next_offset: Number.MAX_SAFE_INTEGER + 1,
      }),
    ).toThrow(/next_offset/);
    expect(() =>
      parseReadinessResponse({
        assessment: { ...assessment, accepted_state_version: 0 },
      }),
    ).toThrow(/accepted_state_version/);
    expect(() =>
      parseCurrentUser({
        id: "u",
        email: "u@example.com",
        role: "Requester",
        user_id: "u",
      }),
    ).not.toThrow();
  });

  it("keeps absent readiness assessment distinct from a rejected assessment", () => {
    expect(parseReadinessResponse({ assessment: null })).toBeNull();
    expect(parseReadinessResponse({ assessment })).toEqual(assessment);
  });
});
