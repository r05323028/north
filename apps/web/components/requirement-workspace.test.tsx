import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/components/requirement-board", () => ({
  RequirementBoard: ({ onCreateAction }: { onCreateAction: () => void }) => (
    <button type="button" onClick={onCreateAction}>
      board
    </button>
  ),
}));
vi.mock("@/components/requirement-list", () => ({
  RequirementList: ({ onCreateAction }: { onCreateAction: () => void }) => (
    <button type="button" onClick={onCreateAction}>
      list
    </button>
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

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
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
