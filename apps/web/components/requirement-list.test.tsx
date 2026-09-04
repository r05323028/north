import { act } from "react";
import type { ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Requirement } from "@/lib/requirements";

const { useCollection } = vi.hoisted(() => ({
  useCollection: vi.fn(),
}));

vi.mock("@/lib/use-requirement-collection", () => ({
  useRequirementCollection: useCollection,
}));

import {
  RequirementList,
  RequirementListView,
} from "@/components/requirement-list";

type ListProps = ComponentProps<typeof RequirementListView>;

const row: Requirement = {
  id: "r/1",
  title: "Login flow",
  description: "Users sign in.",
  summary: "",
  acceptance_criteria: [],
  assumptions: [],
  open_questions: [],
  status: "ready",
  revision: 2,
  state_version: 3,
  created_by: "user-1",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
};

function props(overrides: Partial<ListProps> = {}): ListProps {
  return {
    requirements: [row],
    loading: false,
    refreshing: false,
    error: null,
    creator: "",
    onCreateAction: vi.fn(),
    onCreatorChange: vi.fn(),
    onSearchChange: vi.fn(),
    onSortChange: vi.fn(),
    onStatusChange: vi.fn(),
    search: "",
    sort: "updated",
    status: "",
    ...overrides,
  };
}

function setValue(element: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(
    Object.getPrototypeOf(element),
    "value",
  )?.set;
  setter?.call(element, value);
  element.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("RequirementListView", () => {
  let root: ReturnType<typeof createRoot> | undefined;
  let container: HTMLDivElement | undefined;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    useCollection.mockReset();
  });

  it("renders loading, empty, error, and refreshing states", () => {
    expect(
      renderToStaticMarkup(
        <RequirementListView
          {...props({ requirements: [], loading: true })}
        />,
      ),
    ).toContain("載入需求中");

    const html = renderToStaticMarkup(
      <RequirementListView
        {...props({
          requirements: [],
          error: "request failed",
          refreshing: true,
        })}
      />,
    );
    expect(html).toContain("request failed");
    expect(html).toContain("重新整理中");
    expect(html).toContain("無符合需求");
  });

  it("renders rows and forwards filters and actions", async () => {
    const onCreateAction = vi.fn();
    const onCreatorChange = vi.fn();
    const onSearchChange = vi.fn();
    const onSortChange = vi.fn();
    const onStatusChange = vi.fn();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    await act(async () => {
      root?.render(
        <RequirementListView
          {...props({
            onCreateAction,
            onCreatorChange,
            onSearchChange,
            onSortChange,
            onStatusChange,
            refreshing: true,
          })}
        />,
      );
    });

    const button = container.querySelector<HTMLButtonElement>(
      '[aria-label="New requirement"]',
    );
    const search = container.querySelector<HTMLInputElement>(
      '[aria-label="Search"]',
    );
    const status = container.querySelector<HTMLSelectElement>(
      '[aria-label="Status"]',
    );
    const sort = container.querySelector<HTMLSelectElement>(
      '[aria-label="Updated"]',
    );
    const creator = container.querySelector<HTMLInputElement>(
      '[aria-label="Creator"]',
    );
    if (!button || !search || !status || !sort || !creator) {
      throw new Error("requirement list controls missing");
    }

    await act(async () => {
      button.click();
      setValue(search, "login");
      setValue(creator, "user-1");
      status.value = "ready";
      status.dispatchEvent(new Event("change", { bubbles: true }));
      sort.value = "updated_asc";
      sort.dispatchEvent(new Event("change", { bubbles: true }));
    });

    expect(onCreateAction).toHaveBeenCalledOnce();
    expect(onSearchChange).toHaveBeenCalledWith("login");
    expect(onCreatorChange).toHaveBeenCalledWith("user-1");
    expect(onStatusChange).toHaveBeenCalledWith("ready");
    expect(onSortChange).toHaveBeenCalledWith("updated_asc");
    expect(container.querySelector('a[href="/requirements/r%2F1"]')).not.toBeNull();
    expect(container.textContent).toContain("Login flow");
  });

  it("passes canonical collection state through wrapper", () => {
    useCollection.mockReturnValue({
      requirements: [row],
      loading: false,
      refreshing: false,
      error: null,
    });

    const html = renderToStaticMarkup(
      <RequirementList onCreateAction={() => undefined} />,
    );

    expect(html).toContain("Login flow");
    expect(useCollection).toHaveBeenCalledWith({ sort: "updated" });
  });
});
