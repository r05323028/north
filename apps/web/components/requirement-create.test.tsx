import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

const { push } = vi.hoisted(() => ({ push: vi.fn() }));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
}));

import { RequirementCreate } from "@/components/requirement-create";

function response() {
  return {
    ok: true,
    status: 201,
    text: () =>
      Promise.resolve(
        JSON.stringify({
          id: "created/requirement",
          title: "Title",
          description: "Description",
          summary: "",
          acceptance_criteria: [],
          assumptions: [],
          open_questions: [],
          status: "draft",
          revision: 1,
          state_version: 1,
          created_by: "user-1",
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
        }),
      ),
  };
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
}

describe("Requirement creation flow", () => {
  let root: ReturnType<typeof createRoot> | undefined;
  let container: HTMLDivElement | undefined;

  afterEach(() => {
    act(() => {
      root?.unmount();
    });
    container?.remove();
    vi.unstubAllGlobals();
    push.mockReset();
  });

  it("posts two fields and navigates using returned Requirement id", async () => {
    const fetchMock = vi.fn().mockResolvedValue(response());
    vi.stubGlobal("fetch", fetchMock);
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    await act(async () => {
      root?.render(<RequirementCreate onCancelAction={() => undefined} />);
      await settle();
    });

    const title =
      container.querySelector<HTMLInputElement>("#requirement-title");
    const description = container.querySelector<HTMLTextAreaElement>(
      "#requirement-description",
    );
    const form = container.querySelector<HTMLFormElement>("form");
    if (!title || !description || !form) throw new Error("create form missing");

    const setValue = (
      element: HTMLInputElement | HTMLTextAreaElement,
      value: string,
    ) => {
      const setter = Object.getOwnPropertyDescriptor(
        Object.getPrototypeOf(element),
        "value",
      )?.set;
      setter?.call(element, value);
      element.dispatchEvent(new Event("input", { bubbles: true }));
    };

    await act(async () => {
      setValue(title, "Title");
      setValue(description, "Description");
      form.dispatchEvent(
        new Event("submit", { bubbles: true, cancelable: true }),
      );
      await settle();
      await vi.waitFor(() =>
        expect(push).toHaveBeenCalledWith(
          "/requirements/created%2Frequirement",
        ),
      );
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/requirements",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ title: "Title", description: "Description" }),
      }),
    );
    expect(push).toHaveBeenCalledWith("/requirements/created%2Frequirement");
  });
});
