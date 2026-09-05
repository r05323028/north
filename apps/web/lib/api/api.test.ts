import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "@/lib/api/client";
import {
  ClarificationUnavailableError,
  cancelClarification,
  dispatchClarificationMessage,
  getActivityPage,
  startClarification,
} from "@/lib/api/clarification";
import {
  getConversationPage,
  postRequesterMessage,
} from "@/lib/api/conversations";

function response(value: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    text: () => Promise.resolve(JSON.stringify(value)),
  };
}

const requirement = {
  id: "requirement/1",
  title: "Requirement",
  description: "Description",
  summary: "",
  acceptance_criteria: [],
  assumptions: [],
  open_questions: [],
  status: "draft",
  revision: 1,
  state_version: 1,
  created_by: "user-1",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

const run = {
  run_id: "run-1",
  requirement_id: "requirement/1",
  start_message_id: "message-1",
  phase: "awaiting_assignment",
  status: "unavailable",
  cancel_requested: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
};

afterEach(() => vi.unstubAllGlobals());

describe("workspace API operations", () => {
  it("uses canonical paged and persistence endpoints", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        response({
          id: "conversation-1",
          requirement_id: "requirement/1",
          created_at: "2026-01-01T00:00:00Z",
          messages: [],
          next_offset: 2,
        }),
      )
      .mockResolvedValueOnce(
        response({
          id: "message-1",
          conversation_id: "conversation-1",
          author_user_id: "user-1",
          kind: "requester",
          body: "question",
          created_at: "2026-01-01T00:00:00Z",
        }),
      );
    vi.stubGlobal("fetch", fetchMock);

    await getConversationPage("requirement/1", 0, 2);
    await postRequesterMessage("requirement/1", "question");

    expect(fetchMock.mock.calls[0][0]).toBe(
      "/requirements/requirement%2F1/conversation?offset=0&limit=2",
    );
    expect(fetchMock.mock.calls[1][0]).toBe(
      "/requirements/requirement%2F1/conversation/messages",
    );
    expect(fetchMock.mock.calls[1][1]).toEqual(
      expect.objectContaining({
        method: "POST",
        credentials: "include",
        body: JSON.stringify({ body: "question" }),
      }),
    );
  });

  it("retains HTTP status and server error code for conflicts", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(response({ error: "conflict" }, 409)),
    );
    await expect(
      dispatchClarificationMessage("requirement/1", "run-1", "message-1"),
    ).rejects.toMatchObject({ status: 409, code: "conflict" });
    await expect(
      dispatchClarificationMessage("requirement/1", "run-1", "message-1"),
    ).rejects.toBeInstanceOf(ApiError);
  });

  it("classifies unavailable start and retains canonical run identity", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        response(
          {
            error: "clarification_unavailable",
            requirement,
            session: run,
          },
          503,
        ),
      ),
    );

    const error = await startClarification("requirement/1", {
      message_id: "message-1",
      expected_state_version: 1,
    }).catch((cause: unknown) => cause);

    expect(error).toBeInstanceOf(ClarificationUnavailableError);
    expect(error).toMatchObject({
      status: 503,
      code: "clarification_unavailable",
    });
    expect((error as ClarificationUnavailableError).run.run_id).toBe("run-1");
    expect(
      (error as ClarificationUnavailableError).requirement.state_version,
    ).toBe(1);
  });

  it("uses explicit run and message identities for dispatch and cancellation", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        response(
          { session: { ...run, phase: "active", status: "running" } },
          202,
        ),
      )
      .mockResolvedValueOnce(
        response(
          { session: { ...run, phase: "active", status: "running" } },
          202,
        ),
      )
      .mockResolvedValueOnce(response({ activities: [], next_offset: null }));
    vi.stubGlobal("fetch", fetchMock);

    await dispatchClarificationMessage("requirement/1", "run/A", "message/1");
    await cancelClarification("requirement/1", "run/A");
    await getActivityPage("requirement/1", 4, 2);

    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      "/requirements/requirement%2F1/clarification/runs/run%2FA/messages/message%2F1/dispatch",
      "/requirements/requirement%2F1/clarification/runs/run%2FA/cancel",
      "/requirements/requirement%2F1/activity?offset=4&limit=2",
    ]);
  });
});
