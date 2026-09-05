import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ConversationPage } from "@/lib/api/contracts";
import { useRequirementConversationWorkspace } from "@/lib/use-requirement-conversation-workspace";

type EventListener = (event: Event) => void;

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  readyState = 0;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  private readonly listeners = new Map<string, Set<EventListener>>();

  constructor(readonly url: string) {
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: EventListener) {
    const listeners = this.listeners.get(type) ?? new Set<EventListener>();
    listeners.add(listener);
    this.listeners.set(type, listeners);
  }

  removeEventListener(type: string, listener: EventListener) {
    this.listeners.get(type)?.delete(listener);
  }

  open() {
    this.readyState = 1;
    this.onopen?.();
  }

  fail() {
    this.readyState = 0;
    this.onerror?.();
  }

  emit(type: string, data: string) {
    const event = new MessageEvent(type, { data });
    this.listeners.get(type)?.forEach((listener) => listener(event));
  }

  close() {
    this.readyState = 2;
  }
}

const requirement = {
  id: "requirement-1",
  title: "Initial requirement",
  description: "Description",
  summary: "Summary",
  acceptance_criteria: [],
  assumptions: [],
  open_questions: [],
  status: "draft",
  revision: 1,
  state_version: 1,
  created_by: "owner-1",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

function message(id: string) {
  const second =
    { X: 0, A: 1, B: 2, C: 3, D: 4, E: 5, F: 6 }[
      id as "X" | "A" | "B" | "C" | "D" | "E" | "F"
    ] ?? 0;
  return {
    id,
    conversation_id: "conversation-1",
    author_user_id: "user-1",
    kind: "requester" as const,
    body: id,
    created_at: `2026-01-01T00:00:${String(second).padStart(2, "0")}Z`,
  };
}

function activity(id: number) {
  return {
    id,
    event_id: `event-${id}`,
    session_id: "run-1",
    activity: `activity-${id}`,
    created_at: `2026-01-01T00:01:${String(id).padStart(2, "0")}Z`,
  };
}

function conversationPage(
  ids: string[],
  next_offset: number | null,
): ConversationPage {
  return {
    id: "conversation-1",
    requirement_id: "requirement-1",
    created_at: "2026-01-01T00:00:00Z",
    messages: ids.map((id) => message(id)),
    next_offset,
  };
}

function response(value: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    text: () => Promise.resolve(JSON.stringify(value)),
  };
}

function deferred<T>() {
  let resolve: (value: T | PromiseLike<T>) => void = () => undefined;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

let currentRequirement = { ...requirement };
let conversationPages = new Map<number, ConversationPage>();
let activityPages = new Map<
  number,
  { activities: ReturnType<typeof activity>[]; next_offset: number | null }
>();

function valueFor(path: string): unknown {
  const url = new URL(path, "http://north.test");
  if (url.pathname === "/requirements/requirement-1") return currentRequirement;
  if (url.pathname === "/requirements/requirement-1/conversation") {
    return conversationPages.get(Number(url.searchParams.get("offset") ?? 0));
  }
  if (url.pathname === "/requirements/requirement-1/readiness")
    return { assessment: null };
  if (url.pathname === "/requirements/requirement-1/activity") {
    return activityPages.get(Number(url.searchParams.get("offset") ?? 0));
  }
  if (url.pathname === "/requirements/requirement-1/session")
    return { session: null };
  if (url.pathname === "/auth/me") {
    return { id: "user-1", email: "user@example.com", role: "Requester" };
  }
  throw new Error(`unexpected path ${path}`);
}

let fetchMock = vi.fn((path: string) =>
  Promise.resolve(response(valueFor(path))),
);

function Probe({ beforeRefresh }: { beforeRefresh?: (count: number) => void }) {
  const state = useRequirementConversationWorkspace("requirement-1");
  const refreshCount = useRef(0);
  return (
    <>
      <output data-testid="bundle">
        {state.loading
          ? "loading"
          : [
              state.requirement?.title ?? "missing-requirement",
              state.conversation?.messages.map(({ id }) => id).join(",") ??
                "missing-conversation",
              state.activities.map(({ id }) => id).join(","),
              state.refreshError ?? "ok",
              state.connectionState,
            ].join("|")}
      </output>
      <button type="button" onClick={() => void state.loadMoreConversation()}>
        load conversation
      </button>
      <button type="button" onClick={() => void state.loadMoreActivity()}>
        load activity
      </button>
      <button
        type="button"
        onClick={() => {
          refreshCount.current += 1;
          beforeRefresh?.(refreshCount.current);
          void state.refresh();
        }}
      >
        refresh
      </button>
    </>
  );
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

function setupPages() {
  currentRequirement = { ...requirement };
  conversationPages = new Map([
    [0, conversationPage(["A", "B"], 2)],
    [2, conversationPage(["C", "D"], null)],
  ]);
  activityPages = new Map([
    [0, { activities: [activity(1)], next_offset: null }],
  ]);
  fetchMock = vi.fn((path: string) =>
    Promise.resolve(response(valueFor(path))),
  );
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("EventSource", FakeEventSource);
}

afterEach(() => {
  vi.unstubAllGlobals();
  FakeEventSource.instances = [];
  document.body.replaceChildren();
});

describe("useRequirementConversationWorkspace", () => {
  it("loads canonical bundle, paginates, and repairs shifted complete history after reconnect and focus", async () => {
    setupPages();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<Probe />);
      await settle();
    });
    expect(
      container.querySelector("[data-testid=bundle]")?.textContent,
    ).toContain("Initial requirement");
    const source = FakeEventSource.instances[0];
    expect(source.url).toBe("/events");
    await act(async () => {
      source.open();
      await settle();
    });
    expect(container.textContent).toContain("connected");

    const loadConversation = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "load conversation",
    );
    if (!loadConversation)
      throw new Error("conversation pagination control missing");
    await act(async () => {
      loadConversation.click();
      await settle();
    });
    expect(container.textContent).toContain("A,B,C,D");

    conversationPages = new Map([
      [0, conversationPage(["X", "A"], 2)],
      [2, conversationPage(["B", "C"], 4)],
      [4, conversationPage(["D", "E"], null)],
    ]);
    await act(async () => {
      source.fail();
      source.open();
      await settle();
    });
    expect(container.textContent).toContain("X,A,B,C,D,E");
    expect((container.textContent ?? "").match(/A/g)?.length).toBe(1);

    conversationPages = new Map([
      [0, conversationPage(["X", "A"], 2)],
      [2, conversationPage(["B", "C"], 4)],
      [4, conversationPage(["D", "E"], 6)],
      [6, conversationPage(["F"], null)],
    ]);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await settle();
    });
    expect(container.textContent).toContain("X,A,B,C,D,E,F");
    expect((container.textContent ?? "").match(/F/g)?.length).toBe(1);
    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  it("suppresses an older repair when a newer repair completes first", async () => {
    setupPages();
    conversationPages = new Map([[0, conversationPage(["A", "B"], null)]]);
    const olderPage = deferred<ReturnType<typeof response>>();
    let repairCalls = 0;
    fetchMock.mockImplementation((path: string) => {
      const url = new URL(path, "http://north.test");
      if (url.pathname === "/requirements/requirement-1/conversation") {
        repairCalls += 1;
        return repairCalls === 1
          ? olderPage.promise
          : Promise.resolve(response(conversationPage(["N", "A", "B"], null)));
      }
      return Promise.resolve(response(valueFor(path)));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<Probe />);
      await settle();
    });
    repairCalls = 0;
    const refresh = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "refresh",
    );
    if (!refresh) throw new Error("refresh control missing");
    await act(async () => {
      refresh.click();
      refresh.click();
      await settle();
    });
    expect(container.textContent).toContain("N,A,B");
    olderPage.resolve(response(conversationPage(["O", "A", "B"], null)));
    await act(async () => {
      await settle();
    });
    expect(container.textContent).toContain("N,A,B");
    expect(container.textContent).not.toContain("O,A,B");
    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  it("retains newest canonical response and stale data after refresh failure", async () => {
    setupPages();
    const oldRequirement = deferred<ReturnType<typeof response>>();
    const newRequirement = deferred<ReturnType<typeof response>>();
    let refreshCount = 0;
    fetchMock.mockImplementation((path: string) => {
      if (path === "/requirements/requirement-1" && refreshCount === 1)
        return oldRequirement.promise;
      if (path === "/requirements/requirement-1" && refreshCount === 2)
        return newRequirement.promise;
      return Promise.resolve(response(valueFor(path)));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <Probe
          beforeRefresh={(count) => {
            refreshCount = count;
          }}
        />,
      );
      await settle();
    });
    const refresh = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "refresh",
    );
    if (!refresh) throw new Error("refresh control missing");

    await act(async () => {
      refresh.click();
      refresh.click();
      await settle();
    });
    newRequirement.resolve(
      response({ ...requirement, title: "Newest requirement" }),
    );
    await act(async () => {
      await settle();
    });
    expect(container.textContent).toContain("Newest requirement");
    oldRequirement.resolve(
      response({ ...requirement, title: "Old requirement" }),
    );
    await act(async () => {
      await settle();
    });
    expect(container.textContent).toContain("Newest requirement");
    expect(container.textContent).not.toContain("Old requirement");

    currentRequirement = { ...requirement, title: "Newest requirement" };
    fetchMock.mockImplementationOnce(() =>
      Promise.reject(new Error("offline")),
    );
    await act(async () => {
      refresh.click();
      await settle();
    });
    expect(container.textContent).toContain("Newest requirement");
    expect(container.textContent).toContain("offline");
    await act(async () => {
      root.unmount();
    });
    container.remove();
  });
});
