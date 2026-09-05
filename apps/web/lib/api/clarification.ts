import { requestJson, ApiError } from "@/lib/api/client";
import {
  parseActivityPage,
  parseClarificationMutationResponse,
  parseReadinessResponse,
  parseSessionResponse,
  type ActivityItem,
  type ClarificationRun,
  type ReadinessView,
} from "@/lib/api/contracts";
import { parseRequirement, type Requirement } from "@/lib/requirements";

export class ClarificationUnavailableError extends ApiError {
  readonly requirement: Requirement;
  readonly run: ClarificationRun;

  constructor(
    error: ApiError,
    requirement: Requirement,
    run: ClarificationRun,
  ) {
    super(error.status, error.code, error.message, error.body);
    this.name = "ClarificationUnavailableError";
    this.requirement = requirement;
    this.run = run;
  }
}

function clarificationPath(requirementId: string): string {
  return `/requirements/${encodeURIComponent(requirementId)}/clarification`;
}

function parseUnavailable(error: ApiError): ClarificationUnavailableError {
  if (error.status !== 503 || error.code !== "clarification_unavailable") {
    throw error;
  }
  const body = error.body;
  if (typeof body !== "object" || body === null || Array.isArray(body)) {
    throw new Error(
      "Server returned invalid clarification_unavailable response",
    );
  }
  const record = body as Record<string, unknown>;
  if (record.error !== "clarification_unavailable") {
    throw new Error(
      "Server returned invalid clarification_unavailable response",
    );
  }
  const requirement = parseRequirement(record.requirement);
  const run = parseClarificationMutationResponse({ session: record.session });
  return new ClarificationUnavailableError(error, requirement, run);
}

export async function getLatestClarificationRun(
  requirementId: string,
): Promise<ClarificationRun | null> {
  return parseSessionResponse(
    await requestJson(
      `/requirements/${encodeURIComponent(requirementId)}/session`,
    ),
  );
}

export const getSession = getLatestClarificationRun;

export async function getReadiness(
  requirementId: string,
): Promise<ReadinessView | null> {
  return parseReadinessResponse(
    await requestJson(
      `/requirements/${encodeURIComponent(requirementId)}/readiness`,
    ),
  );
}

export async function getActivityPage(
  requirementId: string,
  offset = 0,
  limit = 50,
): Promise<{ activities: ActivityItem[]; next_offset: number | null }> {
  if (
    !Number.isSafeInteger(offset) ||
    offset < 0 ||
    !Number.isSafeInteger(limit) ||
    limit < 1 ||
    limit > 100
  ) {
    throw new RangeError("Activity page must use a bounded offset and limit");
  }
  const query = `?offset=${offset}&limit=${limit}`;
  return parseActivityPage(
    await requestJson(
      `/requirements/${encodeURIComponent(requirementId)}/activity${query}`,
    ),
  );
}

export async function startClarification(
  requirementId: string,
  input: { message_id: string; expected_state_version: number },
): Promise<ClarificationRun> {
  try {
    return parseClarificationMutationResponse(
      await requestJson(`${clarificationPath(requirementId)}/start`, {
        method: "POST",
        body: JSON.stringify(input),
      }),
    );
  } catch (cause) {
    if (cause instanceof ApiError) throw parseUnavailable(cause);
    throw cause;
  }
}

export async function dispatchClarificationMessage(
  requirementId: string,
  runId: string,
  messageId: string,
): Promise<ClarificationRun> {
  return parseClarificationMutationResponse(
    await requestJson(
      `${clarificationPath(requirementId)}/runs/${encodeURIComponent(runId)}/messages/${encodeURIComponent(messageId)}/dispatch`,
      { method: "POST" },
    ),
  );
}

export async function cancelClarification(
  requirementId: string,
  runId: string,
): Promise<ClarificationRun> {
  return parseClarificationMutationResponse(
    await requestJson(
      `${clarificationPath(requirementId)}/runs/${encodeURIComponent(runId)}/cancel`,
      { method: "POST" },
    ),
  );
}
