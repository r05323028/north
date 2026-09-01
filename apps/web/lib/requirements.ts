export const requirementStatuses = [
  "draft",
  "discussing",
  "ready",
  "accepted",
  "rejected",
] as const;

export type RequirementStatus = (typeof requirementStatuses)[number];

export const requirementStatusLabels = {
  draft: "Draft",
  discussing: "Discussing",
  ready: "Ready",
  accepted: "Accepted",
  rejected: "Rejected",
} satisfies Record<RequirementStatus, string>;

export type RequirementSort = "updated" | "updated_asc";

export type Requirement = {
  id: string;
  title: string;
  description: string;
  summary: string;
  acceptance_criteria: string[];
  assumptions: string[];
  open_questions: string[];
  status: RequirementStatus;
  revision: number;
  state_version: number;
  created_by: string;
  created_at: string;
  updated_at: string;
};

export type RequirementQuery = {
  search?: string;
  status?: RequirementStatus;
  created_by?: string;
  sort?: RequirementSort;
};

export type CreateRequirementInput = {
  title: string;
  description: string;
};

export function isRequirementStatus(
  value: unknown,
): value is RequirementStatus {
  return (
    typeof value === "string" &&
    (requirementStatuses as readonly string[]).includes(value)
  );
}

function invalidRequirementField(field: string): never {
  throw new Error(`Server returned invalid Requirement field: ${field}`);
}

function requiredString(value: Record<string, unknown>, field: string): string {
  const fieldValue = value[field];
  return typeof fieldValue === "string"
    ? fieldValue
    : invalidRequirementField(field);
}

function requiredStringArray(
  value: Record<string, unknown>,
  field: string,
): string[] {
  const fieldValue = value[field];
  return Array.isArray(fieldValue) &&
    fieldValue.every((item) => typeof item === "string")
    ? fieldValue
    : invalidRequirementField(field);
}

function requiredNumber(value: Record<string, unknown>, field: string): number {
  const fieldValue = value[field];
  return typeof fieldValue === "number" && Number.isFinite(fieldValue)
    ? fieldValue
    : invalidRequirementField(field);
}

export function parseRequirement(value: unknown): Requirement {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Server returned invalid Requirement");
  }
  const record = value as Record<string, unknown>;
  const status = record.status;
  if (!isRequirementStatus(status)) {
    throw new Error(
      `Server returned invalid Requirement status: ${String(status)}`,
    );
  }
  return {
    id: requiredString(record, "id"),
    title: requiredString(record, "title"),
    description: requiredString(record, "description"),
    summary: requiredString(record, "summary"),
    acceptance_criteria: requiredStringArray(record, "acceptance_criteria"),
    assumptions: requiredStringArray(record, "assumptions"),
    open_questions: requiredStringArray(record, "open_questions"),
    status,
    revision: requiredNumber(record, "revision"),
    state_version: requiredNumber(record, "state_version"),
    created_by: requiredString(record, "created_by"),
    created_at: requiredString(record, "created_at"),
    updated_at: requiredString(record, "updated_at"),
  };
}

function parseRequirementList(value: unknown): Requirement[] {
  if (!Array.isArray(value)) {
    throw new Error("Server returned invalid Requirement collection");
  }
  return value.map(parseRequirement);
}

export function requirementsUrl(query: RequirementQuery = {}): string {
  const params = new URLSearchParams();
  const search = query.search?.trim();
  const creator = query.created_by?.trim();

  if (search) params.set("search", search);
  if (query.status) params.set("status", query.status);
  if (creator) params.set("created_by", creator);
  if (query.sort) params.set("sort", query.sort);

  const encoded = params.toString();
  return encoded ? `/requirements?${encoded}` : "/requirements";
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers);
  if (!headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }

  const response = await fetch(path, {
    credentials: "include",
    ...init,
    headers,
  });
  const body = await response.text();

  if (!response.ok) {
    let message = `Request failed (${response.status})`;
    if (body) {
      try {
        const parsed: unknown = JSON.parse(body);
        if (
          typeof parsed === "object" &&
          parsed !== null &&
          "error" in parsed &&
          typeof parsed.error === "string"
        ) {
          message = parsed.error;
        }
      } catch {
        // Keep status fallback when server has no JSON error body.
      }
    }
    throw new Error(message);
  }

  if (!body) return undefined as T;
  try {
    return JSON.parse(body) as T;
  } catch {
    throw new Error("Server returned invalid JSON");
  }
}

export async function listRequirements(
  query: RequirementQuery = {},
): Promise<Requirement[]> {
  return parseRequirementList(await request<unknown>(requirementsUrl(query)));
}

export async function listRequirementsAtPath(
  path: string,
): Promise<Requirement[]> {
  return parseRequirementList(await request<unknown>(path));
}

export async function getRequirement(id: string): Promise<Requirement> {
  return parseRequirement(
    await request<unknown>(`/requirements/${encodeURIComponent(id)}`),
  );
}

export async function createRequirement(
  input: CreateRequirementInput,
): Promise<Requirement> {
  return parseRequirement(
    await request<unknown>("/requirements", {
      method: "POST",
      body: JSON.stringify({
        title: input.title,
        description: input.description,
      }),
    }),
  );
}

export type RequirementGroups = {
  [Status in RequirementStatus]: Requirement[];
};

export function groupRequirements(
  requirements: Requirement[],
): RequirementGroups {
  const groups: RequirementGroups = {
    draft: [],
    discussing: [],
    ready: [],
    accepted: [],
    rejected: [],
  };

  for (const requirement of requirements) {
    const status: unknown = requirement.status;
    if (!isRequirementStatus(status)) {
      throw new Error(
        `Cannot group Requirement ${requirement.id}: invalid status ${String(status)}`,
      );
    }
    groups[status].push(requirement);
  }

  return groups;
}
