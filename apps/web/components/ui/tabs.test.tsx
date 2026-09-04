import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Tabs } from "@/components/ui/tabs";

describe("Tabs", () => {
  let root: ReturnType<typeof createRoot> | undefined;
  let container: HTMLDivElement | undefined;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
  });

  it("marks selected and disabled tabs and reports activation", async () => {
    const onValueChangeAction = vi.fn();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    await act(async () => {
      root?.render(
        <Tabs
          ariaLabel="Views"
          items={[
            { accessibleName: "Board", label: "Board", value: "board" },
            {
              accessibleName: "List",
              disabled: true,
              label: "List",
              value: "list",
            },
          ]}
          onValueChangeAction={onValueChangeAction}
          value="board"
        />,
      );
    });

    const board = container.querySelector<HTMLButtonElement>(
      '[aria-label="Board"]',
    );
    const list = container.querySelector<HTMLButtonElement>(
      '[aria-label="List"]',
    );
    if (!board || !list) throw new Error("tab controls missing");

    expect(board.getAttribute("aria-selected")).toBe("true");
    expect(list.getAttribute("aria-selected")).toBe("false");
    expect(list.disabled).toBe(true);
    await act(async () => board.click());
    expect(onValueChangeAction).toHaveBeenCalledWith("board");
  });
});
