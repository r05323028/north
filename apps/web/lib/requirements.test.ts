import { afterEach, describe, expect, it, vi } from "vitest";

import {
  createRequirement,
  getRequirement,
  groupRequirements,
  listRequirements,
  requirementsUrl,
  type Requirement,
} from "@/lib/requirements";

function requirement(
  id: string,
  status: Requirement["status"],
  overrides: Partial<Requirement> = {},
): Requirement {
  return {
    id,
    title: `Requirement ${id}`,
    description: "Description",
    summary: "Summary",
    acceptance_criteria: [],
    assumptions: [],
    open_questions: [],
    status,
    revision: 1,
    state_version: 1,
    created_by: "user-1",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function jsonResponse(value: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    text: () => Promise.resolve(JSON.stringify(value)),
  };
}

describe("Requirement collection helpers", () => {
  it("places each mixed-status Requirement in exactly one fixed group", () => {
    const requirements = [
      requirement("draft", "Draft"),
      requirement("discussing", "Discussing"),
      requirement("ready", "Ready"),
      requirement("accepted", "Accepted"),
      requirement("rejected", "Rejected"),
    ];

    const groups = groupRequirements(requirements);
    const grouped = Object.values(groups).flat();

    expect(grouped).toHaveLength(requirements.length);
    expect(new Set(grouped.map(({ id }) => id))).toEqual(
      new Set(requirements.map(({ id }) => id)),
    );
    expect(groups.Ready.map(({ id }) => id)).toEqual(["ready"]);
    expect(groups.Rejected.map(({ id }) => id)).toEqual(["rejected"]);
  });

  it("maps supported list controls to server query names", () => {
    expect(
      requirementsUrl({
        search: "login flow",
        status: "Ready",
        created_by: "user-1",
        sort: "updated_asc",
      }),
    ).toBe(
      "/requirements?search=login+flow&status=ready&created_by=user-1&sort=updated_asc",
    );
    expect(requirementsUrl()).toBe("/requirements");
  });
});

describe("Requirement API", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("sends list filters to the canonical collection endpoint", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse([]));
    vi.stubGlobal("fetch", fetchMock);

    await listRequirements({
      status: "Ready",
      created_by: "user-1",
      sort: "updated",
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/requirements?status=ready&created_by=user-1&sort=updated",
      expect.objectContaining({ credentials: "include" }),
    );
  });

  it("posts only title and description and returns canonical response", async () => {
    const canonical = requirement("created", "Draft", {
      revision: 1,
      state_version: 1,
    });
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(canonical, 201));
    vi.stubGlobal("fetch", fetchMock);

    const result = await createRequirement({
      title: " New requirement ",
      description: " A description ",
    });

    expect(result).toEqual(canonical);
    expect(fetchMock).toHaveBeenCalledWith(
      "/requirements",
      expect.objectContaining({
        credentials: "include",
        method: "POST",
        body: JSON.stringify({
          title: " New requirement ",
          description: " A description ",
        }),
      }),
    );
  });

  it("loads detail through the encoded canonical route", async () => {
    const canonical = requirement("r/1", "Ready");
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(canonical));
    vi.stubGlobal("fetch", fetchMock);

    await expect(getRequirement("r/1")).resolves.toEqual(canonical);
    expect(fetchMock).toHaveBeenCalledWith(
      "/requirements/r%2F1",
      expect.objectContaining({ credentials: "include" }),
    );
  });
});
