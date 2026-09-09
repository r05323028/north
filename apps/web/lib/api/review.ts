import { requestJson } from "@/lib/api/client";
import type { ReviewPacket } from "@/lib/api/contracts";
import { parseReviewPacket } from "@/lib/api/contracts";
import { parseRequirement, type Requirement } from "@/lib/requirements";

export type ReadyReviewRequest = {
  assessment_id: string;
  expected_state_version: number;
};

export type RequestChangesReviewRequest = ReadyReviewRequest & {
  feedback: string;
};

export type ReopenReviewRequest = {
  expected_state_version: number;
};

function requirementReviewPath(requirementId: string, action: string): string {
  return `/requirements/${encodeURIComponent(requirementId)}/${action}`;
}

async function postRequirementReview(
  requirementId: string,
  action: string,
  body: ReadyReviewRequest | RequestChangesReviewRequest | ReopenReviewRequest,
): Promise<Requirement> {
  return parseRequirement(
    await requestJson(requirementReviewPath(requirementId, action), {
      method: "POST",
      body: JSON.stringify(body),
    }),
  );
}

export async function getReviewPacket(
  requirementId: string,
): Promise<ReviewPacket> {
  return parseReviewPacket(
    await requestJson(
      `/requirements/${encodeURIComponent(requirementId)}/review-packet`,
    ),
  );
}

export function acceptRequirementReview(
  requirementId: string,
  input: ReadyReviewRequest,
): Promise<Requirement> {
  return postRequirementReview(requirementId, "accept", input);
}

export function rejectRequirementReview(
  requirementId: string,
  input: ReadyReviewRequest,
): Promise<Requirement> {
  return postRequirementReview(requirementId, "reject", input);
}

export function requestRequirementChanges(
  requirementId: string,
  input: RequestChangesReviewRequest,
): Promise<Requirement> {
  return postRequirementReview(requirementId, "request-changes", input);
}

export function reopenRequirement(
  requirementId: string,
  input: ReopenReviewRequest,
): Promise<Requirement> {
  return postRequirementReview(requirementId, "reopen", input);
}
