import { requestJson } from "@/lib/api/client";
import {
  requirementStatuses,
  type RequirementSort,
  type RequirementStatus,
} from "@/lib/requirements";

export type RequirementWorkspaceView = "board" | "list";

export type RequirementWorkspacePreferences = {
  default_view: RequirementWorkspaceView;
  list_status: RequirementStatus | null;
  list_created_by: string | null;
  list_sort: RequirementSort;
};

const preferencesPath = "/auth/me/preferences/requirement-workspace";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isRequirementStatus(value: unknown): value is RequirementStatus {
  return typeof value === "string" && requirementStatuses.some((status) => status === value);
}

function parsePreferences(value: unknown): RequirementWorkspacePreferences {
  if (!isRecord(value)) throw new Error("Invalid requirement workspace preferences");

  const defaultView = value.default_view;
  const listStatus = value.list_status;
  const listCreatedBy = value.list_created_by;
  const listSort = value.list_sort;
  if (defaultView !== "board" && defaultView !== "list") {
    throw new Error("Invalid requirement workspace preferences");
  }
  if (listStatus !== null && !isRequirementStatus(listStatus)) {
    throw new Error("Invalid requirement workspace preferences");
  }
  if (listCreatedBy !== null && typeof listCreatedBy !== "string") {
    throw new Error("Invalid requirement workspace preferences");
  }
  if (listSort !== "updated" && listSort !== "updated_asc") {
    throw new Error("Invalid requirement workspace preferences");
  }

  return {
    default_view: defaultView,
    list_status: listStatus,
    list_created_by: listCreatedBy,
    list_sort: listSort,
  };
}

export async function getRequirementWorkspacePreferences(): Promise<RequirementWorkspacePreferences> {
  return parsePreferences(await requestJson(preferencesPath));
}

export async function saveRequirementWorkspacePreferences(
  preferences: RequirementWorkspacePreferences,
): Promise<RequirementWorkspacePreferences> {
  const response = await requestJson(preferencesPath, {
    method: "PUT",
    body: JSON.stringify(preferences),
  });
  return parsePreferences(response);
}
