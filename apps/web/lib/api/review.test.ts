import { afterEach, describe, expect, it, vi } from "vitest";

import {
  acceptRequirementReview,
  getReviewPacket,
  rejectRequirementReview,
  reopenRequirement,
  requestRequirementChanges,
} from "@/lib/api/review";

function response(value: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    text: () => Promise.resolve(JSON.stringify(value)),
  };
}

const requirement = {
  id: "requirement-1",
  title: "Account recovery",
  description: "Users recover access.",
  summary: "Self-service recovery.",
  acceptance_criteria: ["Recovery link expires"],
  assumptions: ["Email exists"],
  open_questions: [],
  status: "ready",
  revision: 4,
  state_version: 9,
  created_by: "owner-1",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
};

const packet = {
  assessment_id: "assessment-1",
  requirement_revision: 4,
  requirement_state_version: 9,
  goal: "Recover access",
  scope: "Email recovery",
  summary: "Self-service recovery.",
  acceptance_criteria: ["Recovery link expires"],
  assumptions: ["Email exists"],
  open_questions: [],
  blockers: [],
  assessment_assumptions: ["Provider is configured"],
  repositories_reviewed: [
    { repository_id: "repository-1", commit_sha: "a".repeat(40) },
  ],
};

afterEach(() => vi.unstubAllGlobals());

describe("human review API", () => {
  it("loads and parses the canonical Review Packet", async () => {
    const fetchMock = vi.fn().mockResolvedValue(response(packet));
    vi.stubGlobal("fetch", fetchMock);

    await expect(getReviewPacket("requirement/1")).resolves.toEqual(packet);
    expect(fetchMock).toHaveBeenCalledWith(
      "/requirements/requirement%2F1/review-packet",
      expect.objectContaining({ credentials: "include" }),
    );
  });

  it("serializes exact Ready mutation payloads", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(response(requirement))
      .mockResolvedValue(response(requirement))
      .mockResolvedValue(response(requirement));
    vi.stubGlobal("fetch", fetchMock);
    const input = {
      assessment_id: packet.assessment_id,
      expected_state_version: packet.requirement_state_version,
    };

    await acceptRequirementReview("requirement-1", input);
    await rejectRequirementReview("requirement-1", input);
    await requestRequirementChanges("requirement-1", {
      ...input,
      feedback: "Clarify recovery expiry.",
    });

    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      "/requirements/requirement-1/accept",
      "/requirements/requirement-1/reject",
      "/requirements/requirement-1/request-changes",
    ]);
    expect(
      fetchMock.mock.calls.map(([, init]) => JSON.parse(init.body as string)),
    ).toEqual([
      input,
      input,
      { ...input, feedback: "Clarify recovery expiry." },
    ]);
  });

  it("serializes Reopen without assessment_id", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        response({ ...requirement, status: "discussing", state_version: 10 }),
      );
    vi.stubGlobal("fetch", fetchMock);

    await reopenRequirement("requirement-1", {
      expected_state_version: 9,
    });

    expect(JSON.parse(fetchMock.mock.calls[0][1].body as string)).toEqual({
      expected_state_version: 9,
    });
    expect(
      JSON.parse(fetchMock.mock.calls[0][1].body as string),
    ).not.toHaveProperty("assessment_id");
  });

  it("preserves stale HTTP errors for repair handling", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(response({ error: "conflict" }, 409)),
    );

    await expect(
      acceptRequirementReview("requirement-1", {
        assessment_id: "assessment-1",
        expected_state_version: 9,
      }),
    ).rejects.toMatchObject({ status: 409, code: "conflict" });
  });
});
