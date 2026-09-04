import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useRequirementCollection } from "@/lib/use-requirement-collection";
import type { Requirement, RequirementQuery } from "@/lib/requirements";

type EventListener = (event: Event) => void;

class FakeEventSource {
  static instances: FakeEventSource[] = [];

  onopen: (() => void) | null = null;
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

  emit(type: string, data = "") {
    const event = new MessageEvent(type, { data });
    this.listeners.get(type)?.forEach((listener) => listener(event));
  }

  open() {
    this.onopen?.();
  }

  close() {}
}

const first: Requirement = {
  id: "r-1",
  title: "First",
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
};
const second: Requirement = { ...first, title: "Second", status: "ready" };

function response(value: Requirement[]) {
  return {
    ok: true,
    status: 200,
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

function Probe({ query }: { query?: RequirementQuery }) {
  const { requirements, loading } = useRequirementCollection(query);
  return (
    <output>
      {loading
        ? "loading"
        : requirements.map(({ id, status }) => `${id}:${status}`).join("|")}
    </output>
  );
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
}

describe("useRequirementCollection invalidation", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    FakeEventSource.instances = [];
  });

  it("replaces canonical rows after reconnect, duplicate hints, and refocus", async () => {
    const fetchMock = vi
      .fn()
      .mockImplementation(() => Promise.resolve(response([second])));
    fetchMock.mockImplementationOnce(() => Promise.resolve(response([first])));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("EventSource", FakeEventSource);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<Probe />);
      await settle();
    });
    expect(container.textContent).toBe("r-1:draft");

    const source = FakeEventSource.instances[0];
    await act(async () => {
      source.open();
      source.open();
      await settle();
    });
    expect(container.textContent).toBe("r-1:ready");

    await act(async () => {
      source.emit(
        "requirement.changed",
        JSON.stringify({
          category: "requirement.changed",
          requirement_id: "r-1",
        }),
      );
      source.emit(
        "requirement.changed",
        JSON.stringify({
          category: "requirement.changed",
          requirement_id: "r-1",
        }),
      );
      await settle();
    });
    expect(container.textContent).toBe("r-1:ready");
    expect((container.textContent ?? "").match(/r-1:ready/g)).toHaveLength(1);

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await settle();
    });
    expect(container.textContent).toBe("r-1:ready");
    expect(fetchMock).toHaveBeenCalledTimes(5);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  it("keeps one EventSource when query changes", async () => {
    const fetchMock = vi.fn().mockResolvedValue(response([first]));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("EventSource", FakeEventSource);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<Probe query={{ search: "first" }} />);
      await settle();
    });
    fetchMock.mockResolvedValue(response([second]));

    await act(async () => {
      root.render(<Probe query={{ search: "second" }} />);
      await settle();
    });

    expect(FakeEventSource.instances).toHaveLength(1);
    expect(fetchMock.mock.calls.map(([path]) => path)).toEqual([
      "/requirements?search=first",
      "/requirements?search=second",
    ]);

    await act(async () => {
      FakeEventSource.instances[0].emit(
        "requirement.changed",
        JSON.stringify({
          category: "requirement.changed",
          requirement_id: "r-1",
        }),
      );
      await settle();
    });
    expect(fetchMock.mock.calls.at(-1)?.[0]).toBe(
      "/requirements?search=second",
    );

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  it("keeps newest canonical response when duplicate hint requests finish out of order", async () => {
    const initial = deferred<ReturnType<typeof response>>();
    const older = deferred<ReturnType<typeof response>>();
    const newer = deferred<ReturnType<typeof response>>();
    const fetchMock = vi
      .fn()
      .mockImplementationOnce(() => initial.promise)
      .mockImplementationOnce(() => older.promise)
      .mockImplementationOnce(() => newer.promise);
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("EventSource", FakeEventSource);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(<Probe />);
      initial.resolve(response([first]));
      await settle();
    });
    expect(container.textContent).toBe("r-1:draft");

    const source = FakeEventSource.instances[0];
    await act(async () => {
      source.emit(
        "requirement.changed",
        JSON.stringify({
          category: "requirement.changed",
          requirement_id: "r-1",
        }),
      );
      source.emit(
        "requirement.changed",
        JSON.stringify({
          category: "requirement.changed",
          requirement_id: "r-1",
        }),
      );
      await settle();
    });
    expect(fetchMock).toHaveBeenCalledTimes(3);

    await act(async () => {
      newer.resolve(response([second]));
      await settle();
    });
    expect(container.textContent).toBe("r-1:ready");

    await act(async () => {
      older.resolve(response([first]));
      await settle();
    });
    expect(container.textContent).toBe("r-1:ready");

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });
});
