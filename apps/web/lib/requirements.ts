export const requirementStatuses = [
  "Draft",
  "Discussing",
  "Ready",
  "Accepted",
  "Rejected",
] as const;

export type RequirementStatus = (typeof requirementStatuses)[number];
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

export function requirementsUrl(query: RequirementQuery = {}): string {
  const params = new URLSearchParams();
  const search = query.search?.trim();
  const creator = query.created_by?.trim();

  if (search) params.set("search", search);
  if (query.status) params.set("status", query.status.toLowerCase());
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

export function listRequirements(query: RequirementQuery = {}) {
  return request<Requirement[]>(requirementsUrl(query));
}

export function listRequirementsAtPath(path: string) {
  return request<Requirement[]>(path);
}

export function getRequirement(id: string) {
  return request<Requirement>(`/requirements/${encodeURIComponent(id)}`);
}

export function createRequirement(input: CreateRequirementInput) {
  return request<Requirement>("/requirements", {
    method: "POST",
    body: JSON.stringify({
      title: input.title,
      description: input.description,
    }),
  });
}

export type RequirementGroups = {
  [Status in RequirementStatus]: Requirement[];
};

export function groupRequirements(
  requirements: Requirement[],
): RequirementGroups {
  const groups = Object.fromEntries(
    requirementStatuses.map((status) => [status, [] as Requirement[]]),
  ) as RequirementGroups;

  for (const requirement of requirements) {
    groups[requirement.status].push(requirement);
  }

  return groups;
}
