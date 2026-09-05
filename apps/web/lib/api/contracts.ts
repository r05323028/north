import { InvalidServerDataError } from "@/lib/api/client";

// Shared parsers classify malformed canonical payloads for one browser error seam.
const requirementRunPhases = [
  "awaiting_assignment",
  "active",
  "terminal",
] as const;

export type ClarificationPhase = (typeof requirementRunPhases)[number];

export const clarificationStatuses = [
  "starting",
  "running",
  "completed",
  "unavailable",
] as const;

export type ClarificationStatus = (typeof clarificationStatuses)[number];

export type MessageKind = "requester" | "agent" | "system";
export const messageKinds = ["requester", "agent", "system"] as const;

export type Conversation = {
  id: string;
  requirement_id: string;
  created_at: string;
};

export type Message = {
  id: string;
  conversation_id: string;
  author_user_id: string | null;
  kind: MessageKind;
  body: string;
  created_at: string;
};

export type ConversationPage = Conversation & {
  messages: Message[];
  next_offset: number | null;
};

export type ClarificationRun = {
  run_id: string;
  requirement_id: string;
  start_message_id: string;
  phase: ClarificationPhase;
  status: ClarificationStatus;
  cancel_requested: boolean;
  created_at: string;
  updated_at: string;
  last_activity_at: string;
};

export type ReadinessVerdict = "ready" | "needs_clarification";
export type ReadinessOutcome = "accepted" | "rejected";

export type RepositoryCitation = {
  repository_id: string;
  commit_sha: string;
};

export type ReadinessView = {
  id: string;
  event_id: string;
  session_id: string;
  daemon_event_seq: number;
  requirement_revision: number;
  verdict: ReadinessVerdict;
  blockers: string[];
  assumptions: string[];
  repositories_reviewed: RepositoryCitation[];
  outcome: ReadinessOutcome;
  rejection_reason: string | null;
  assessed_at_ms: number;
  accepted_state_version: number | null;
  created_at: string;
  current: boolean;
};

export type ActivityItem = {
  id: number;
  event_id: string;
  session_id: string;
  activity: string;
  created_at: string;
};

export type CurrentUserRole =
  | "Owner"
  | "Admin"
  | "RequirementManager"
  | "Requester";

export type CurrentUser = {
  id: string;
  email: string;
  role: CurrentUserRole;
};

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function invalid(resource: string, field?: string): never {
  throw new InvalidServerDataError(
    field
      ? `Server returned invalid ${resource} field: ${field}`
      : `Server returned invalid ${resource}`,
  );
}

function recordFor(value: unknown, resource: string): Record<string, unknown> {
  return isRecord(value) ? value : invalid(resource);
}

function requiredString(
  record: Record<string, unknown>,
  resource: string,
  field: string,
): string {
  const value = record[field];
  return typeof value === "string" && value.length > 0
    ? value
    : invalid(resource, field);
}

function stringValue(
  record: Record<string, unknown>,
  resource: string,
  field: string,
): string {
  return typeof record[field] === "string"
    ? (record[field] as string)
    : invalid(resource, field);
}

function stringArray(
  record: Record<string, unknown>,
  resource: string,
  field: string,
): string[] {
  const value = record[field];
  return Array.isArray(value) && value.every((item) => typeof item === "string")
    ? [...value]
    : invalid(resource, field);
}

function safeInteger(
  record: Record<string, unknown>,
  resource: string,
  field: string,
  minimum: number,
): number {
  const value = record[field];
  return typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= minimum
    ? value
    : invalid(resource, field);
}

function nullableSafeInteger(
  record: Record<string, unknown>,
  resource: string,
  field: string,
  minimum: number,
): number | null {
  const value = record[field];
  if (value === null) return null;
  return typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= minimum
    ? value
    : invalid(resource, field);
}

function nullableString(
  record: Record<string, unknown>,
  resource: string,
  field: string,
): string | null {
  const value = record[field];
  return value === null
    ? null
    : typeof value === "string"
      ? value
      : invalid(resource, field);
}

function closedValue<T extends string>(
  record: Record<string, unknown>,
  resource: string,
  field: string,
  values: readonly T[],
): T {
  const value = record[field];
  return typeof value === "string" &&
    (values as readonly string[]).includes(value)
    ? (value as T)
    : invalid(resource, field);
}

function requiredBoolean(
  record: Record<string, unknown>,
  resource: string,
  field: string,
): boolean {
  return typeof record[field] === "boolean"
    ? record[field]
    : invalid(resource, field);
}

export function parseMessage(value: unknown): Message {
  const record = recordFor(value, "Message");
  const author = record.author_user_id;
  if (author !== null && typeof author !== "string") {
    invalid("Message", "author_user_id");
  }
  return {
    id: requiredString(record, "Message", "id"),
    conversation_id: requiredString(record, "Message", "conversation_id"),
    author_user_id: author,
    kind: closedValue(record, "Message", "kind", messageKinds),
    body: stringValue(record, "Message", "body"),
    created_at: requiredString(record, "Message", "created_at"),
  };
}

export function parseConversationPage(value: unknown): ConversationPage {
  const record = recordFor(value, "Conversation page");
  const nextOffset = record.next_offset;
  if (
    nextOffset !== null &&
    (typeof nextOffset !== "number" ||
      !Number.isSafeInteger(nextOffset) ||
      nextOffset < 0)
  ) {
    invalid("Conversation page", "next_offset");
  }
  const conversationId = requiredString(record, "Conversation page", "id");
  const messagesValue = record.messages;
  if (!Array.isArray(messagesValue)) invalid("Conversation page", "messages");
  const messages = messagesValue.map(parseMessage);
  if (messages.some((message) => message.conversation_id !== conversationId)) {
    invalid("Conversation page", "messages");
  }
  return {
    id: conversationId,
    requirement_id: requiredString(
      record,
      "Conversation page",
      "requirement_id",
    ),
    created_at: requiredString(record, "Conversation page", "created_at"),
    messages,
    next_offset: nextOffset,
  };
}

export function parseClarificationRun(value: unknown): ClarificationRun {
  const record = recordFor(value, "Clarification run");
  return {
    run_id: requiredString(record, "Clarification run", "run_id"),
    requirement_id: requiredString(
      record,
      "Clarification run",
      "requirement_id",
    ),
    start_message_id: requiredString(
      record,
      "Clarification run",
      "start_message_id",
    ),
    phase: closedValue(
      record,
      "Clarification run",
      "phase",
      requirementRunPhases,
    ),
    status: closedValue(
      record,
      "Clarification run",
      "status",
      clarificationStatuses,
    ),
    cancel_requested: requiredBoolean(
      record,
      "Clarification run",
      "cancel_requested",
    ),
    created_at: requiredString(record, "Clarification run", "created_at"),
    updated_at: requiredString(record, "Clarification run", "updated_at"),
    last_activity_at: requiredString(
      record,
      "Clarification run",
      "last_activity_at",
    ),
  };
}

function parseSessionWrapper(
  value: unknown,
  resource: string,
): ClarificationRun {
  const record = recordFor(value, resource);
  return parseClarificationRun(record.session);
}

export function parseSessionResponse(value: unknown): ClarificationRun | null {
  const record = recordFor(value, "Session response");
  return record.session === null ? null : parseClarificationRun(record.session);
}

export function parseClarificationMutationResponse(
  value: unknown,
): ClarificationRun {
  return parseSessionWrapper(value, "Clarification response");
}

export function parseReadinessResponse(value: unknown): ReadinessView | null {
  const record = recordFor(value, "Readiness response");
  if (record.assessment === null) return null;
  const assessment = recordFor(record.assessment, "Readiness assessment");
  const repositories = assessment.repositories_reviewed;
  if (!Array.isArray(repositories)) {
    invalid("Readiness assessment", "repositories_reviewed");
  }
  return {
    id: requiredString(assessment, "Readiness assessment", "id"),
    event_id: requiredString(assessment, "Readiness assessment", "event_id"),
    session_id: requiredString(
      assessment,
      "Readiness assessment",
      "session_id",
    ),
    daemon_event_seq: safeInteger(
      assessment,
      "Readiness assessment",
      "daemon_event_seq",
      1,
    ),
    requirement_revision: safeInteger(
      assessment,
      "Readiness assessment",
      "requirement_revision",
      1,
    ),
    verdict: closedValue(assessment, "Readiness assessment", "verdict", [
      "ready",
      "needs_clarification",
    ]),
    blockers: stringArray(assessment, "Readiness assessment", "blockers"),
    assumptions: stringArray(assessment, "Readiness assessment", "assumptions"),
    repositories_reviewed: repositories.map((repository) => {
      const item = recordFor(repository, "Repository citation");
      return {
        repository_id: requiredString(
          item,
          "Repository citation",
          "repository_id",
        ),
        commit_sha: requiredString(item, "Repository citation", "commit_sha"),
      };
    }),
    outcome: closedValue(assessment, "Readiness assessment", "outcome", [
      "accepted",
      "rejected",
    ]),
    rejection_reason: nullableString(
      assessment,
      "Readiness assessment",
      "rejection_reason",
    ),
    assessed_at_ms: safeInteger(
      assessment,
      "Readiness assessment",
      "assessed_at_ms",
      0,
    ),
    accepted_state_version: nullableSafeInteger(
      assessment,
      "Readiness assessment",
      "accepted_state_version",
      1,
    ),
    created_at: requiredString(
      assessment,
      "Readiness assessment",
      "created_at",
    ),
    current: requiredBoolean(assessment, "Readiness assessment", "current"),
  };
}

export function parseActivityPage(value: unknown): {
  activities: ActivityItem[];
  next_offset: number | null;
} {
  const record = recordFor(value, "Activity response");
  const activitiesValue = record.activities;
  const nextOffset = record.next_offset;
  if (!Array.isArray(activitiesValue))
    invalid("Activity response", "activities");
  if (
    nextOffset !== null &&
    (typeof nextOffset !== "number" ||
      !Number.isSafeInteger(nextOffset) ||
      nextOffset < 0)
  ) {
    invalid("Activity response", "next_offset");
  }
  return {
    activities: activitiesValue.map((value) => {
      const activity = recordFor(value, "Activity item");
      return {
        id: safeInteger(activity, "Activity item", "id", 1),
        event_id: requiredString(activity, "Activity item", "event_id"),
        session_id: requiredString(activity, "Activity item", "session_id"),
        activity: stringValue(activity, "Activity item", "activity"),
        created_at: requiredString(activity, "Activity item", "created_at"),
      };
    }),
    next_offset: nextOffset,
  };
}

export function parseCurrentUser(value: unknown): CurrentUser {
  const record = recordFor(value, "Current user");
  return {
    id: requiredString(record, "Current user", "id"),
    email: requiredString(record, "Current user", "email"),
    role: closedValue(record, "Current user", "role", [
      "Owner",
      "Admin",
      "RequirementManager",
      "Requester",
    ]),
  };
}

export function isClarificationTerminal(run: ClarificationRun | null): boolean {
  return run?.phase === "terminal";
}

export function isClarificationInputBlocked(
  run: ClarificationRun | null,
): boolean {
  return (
    run?.phase === "awaiting_assignment" ||
    (run?.phase === "active" && run.cancel_requested)
  );
}
