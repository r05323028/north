import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const preferenceApi = vi.hoisted(() => ({
  getRequirementWorkspacePreferences: vi.fn(),
  saveRequirementWorkspacePreferences: vi.fn(),
}));

vi.mock("@/lib/api/requirement-workspace-preferences", () => preferenceApi);

vi.mock("@/components/requirement-board", () => ({
  RequirementBoard: ({ onCreateAction }: { onCreateAction: () => void }) => (
    <button type="button" onClick={onCreateAction}>
      board
    </button>
  ),
}));
vi.mock("@/components/requirement-list", () => ({
  RequirementList: ({
    onCreateAction,
    onPreferencesChange,
    preferences,
  }: {
    onCreateAction: () => void;
    onPreferencesChange: (preferences: { status: string; creator: string; sort: string }) => void;
    preferences: { status: string; creator: string; sort: string };
  }) => (
    <>
      <output data-testid="list-preferences">
        {`${preferences.status}|${preferences.creator}|${preferences.sort}`}
      </output>
      <button aria-label="Edit preferences" type="button" onClick={() => onPreferencesChange({ status: "draft", creator: "Local edit", sort: "updated" })}>edit preferences</button>
      <button type="button" onClick={onCreateAction}>
        list
      </button>
    </>
  ),
}));
vi.mock("@/components/requirement-create", () => ({
  RequirementCreate: ({ onCancelAction }: { onCancelAction: () => void }) => (
    <button type="button" onClick={onCancelAction}>
      cancel
    </button>
  ),
}));

import { RequirementWorkspace } from "@/components/requirement-workspace";

describe("RequirementWorkspace", () => {
  let root: ReturnType<typeof createRoot> | undefined;
  let container: HTMLDivElement | undefined;

  beforeEach(() => {
    preferenceApi.getRequirementWorkspacePreferences.mockResolvedValue({
      default_view: "board",
      list_status: null,
      list_created_by: null,
      list_sort: "updated",
    });
    preferenceApi.saveRequirementWorkspacePreferences.mockImplementation(
      (preferences) => Promise.resolve(preferences),
    );
  });

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    root = undefined;
    container = undefined;
    vi.clearAllMocks();
  });

  const renderWorkspace = async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    await act(async () => root?.render(<RequirementWorkspace />));
  };

  const click = async (selector: string) => {
    const element = container?.querySelector<HTMLButtonElement>(selector);
    if (!element) throw new Error(`missing ${selector}`);
    await act(async () => element.click());
  };

  it("loads default preferences and saves no transient search", async () => {
    await renderWorkspace();
    expect(container?.textContent).toContain("board");

    await click('[aria-label="Save as default"]');
    expect(preferenceApi.saveRequirementWorkspacePreferences).toHaveBeenCalledWith({
      default_view: "board",
      list_status: null,
      list_created_by: null,
      list_sort: "updated",
    });
  });

  it("restores saved List view controls", async () => {
    preferenceApi.getRequirementWorkspacePreferences.mockResolvedValue({
      default_view: "list",
      list_status: "ready",
      list_created_by: "Mina",
      list_sort: "updated_asc",
    });

    await renderWorkspace();
    expect(container?.textContent).toContain("list");
    expect(
      container?.querySelector("[data-testid=\"list-preferences\"]")?.textContent,
    ).toBe("ready|Mina|updated_asc");
  });

  it("does not let a slow load or save response replace newer local edits", async () => {
    let resolveLoad!: (value: { default_view: string; list_status: string; list_created_by: string; list_sort: string }) => void;
    preferenceApi.getRequirementWorkspacePreferences.mockReturnValue(new Promise((resolve) => { resolveLoad = resolve; }));
    await renderWorkspace();
    expect(container?.querySelector('[role="status"]')?.textContent).toContain("載入個人預設中");
    expect(container?.querySelector('[aria-label="Save as default"]')).toBeNull();
    await act(async () => resolveLoad({ default_view: "list", list_status: "ready", list_created_by: "Server", list_sort: "updated" }));

    let resolveSave!: (value: { default_view: "list"; list_status: "ready"; list_created_by: "Server"; list_sort: "updated" }) => void;
    preferenceApi.saveRequirementWorkspacePreferences.mockReturnValue(new Promise((resolve) => { resolveSave = resolve; }));
    await click('[aria-label="Save as default"]');
    await click('[aria-label="Save as default"]');
    expect(preferenceApi.saveRequirementWorkspacePreferences).toHaveBeenCalledTimes(1);
    await click('[aria-label="Edit preferences"]');
    await act(async () => resolveSave({ default_view: "list", list_status: "ready", list_created_by: "Server", list_sort: "updated" }));
    expect(container?.querySelector('[data-testid="list-preferences"]')?.textContent).toBe("draft|Local edit|updated");
    expect(container?.textContent).not.toContain("已儲存個人預設");
  });

  it("shows an error when saving preferences fails", async () => {
    preferenceApi.saveRequirementWorkspacePreferences.mockRejectedValue(
      new Error("network failure"),
    );

    await renderWorkspace();
    await click('[aria-label="List"]');
    await click('[aria-label="Edit preferences"]');
    await click('[aria-label="Save as default"]');
    expect(container?.querySelector('[data-testid="list-preferences"]')?.textContent).toBe("draft|Local edit|updated");
    expect(container?.querySelector('[role="alert"]')?.textContent).toContain(
      "無法儲存個人預設",
    );
    expect(container?.textContent).not.toContain("已儲存個人預設");
  });

  it("switches between board, list, and create views", async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    await act(async () => {
      root?.render(<RequirementWorkspace />);
    });
    expect(container.textContent).toContain("board");

    const click = async (selector: string) => {
      const element = container?.querySelector<HTMLButtonElement>(selector);
      if (!element) throw new Error(`missing ${selector}`);
      await act(async () => element.click());
    };

    await click('[aria-label="List"]');
    expect(container.textContent).toContain("list");

    await click('[aria-label="Board"]');
    expect(container.textContent).toContain("board");

    await click('[aria-label="New requirement"]');
    expect(container.textContent).toContain("cancel");

    await click('[aria-label="Board"]');
    expect(container.textContent).toContain("board");

    const boardCreate = container.querySelector<HTMLButtonElement>(
      'button:not([aria-label])',
    );
    if (!boardCreate) throw new Error("missing board create action");
    await act(async () => boardCreate.click());
    expect(container.textContent).toContain("cancel");

    await click('[aria-label="New requirement"]');
    expect(container.textContent).toContain("cancel");
  });
});
