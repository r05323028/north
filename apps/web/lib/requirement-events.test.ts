import { afterEach, describe, expect, it, vi } from "vitest";

import {
  REQUIREMENT_CHANGED_EVENT,
  subscribeToRequirementEvents,
} from "@/lib/requirement-events";

type EventListener = (event: Event) => void;

class FakeEventSource {
  static instances: FakeEventSource[] = [];

  readonly url: string;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  closed = false;
  private readonly listeners = new Map<string, Set<EventListener>>();

  constructor(url: string) {
    this.url = url;
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

  emit(type: string, data: string) {
    const event = new MessageEvent(type, { data });
    this.listeners.get(type)?.forEach((listener) => listener(event));
  }

  fail() {
    this.onerror?.();
  }

  open() {
    this.onopen?.();
  }

  close() {
    this.closed = true;
  }
}

describe("requirement SSE subscription", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    FakeEventSource.instances = [];
  });

  it("filters malformed and unrelated hints while exposing connection transitions", () => {
    vi.stubGlobal("EventSource", FakeEventSource);
    const onChange = vi.fn();
    const states: string[] = [];
    const unsubscribe = subscribeToRequirementEvents({
      onChange,
      onStateChange: (state) => states.push(state),
      requirementId: "r-1",
    });
    const source = FakeEventSource.instances[0];

    source.emit(
      REQUIREMENT_CHANGED_EVENT,
      JSON.stringify({
        category: REQUIREMENT_CHANGED_EVENT,
        requirement_id: "r-1",
      }),
    );
    source.emit(
      REQUIREMENT_CHANGED_EVENT,
      JSON.stringify({
        category: REQUIREMENT_CHANGED_EVENT,
        requirement_id: "r-2",
      }),
    );
    source.emit(REQUIREMENT_CHANGED_EVENT, "not-json");
    source.emit(
      REQUIREMENT_CHANGED_EVENT,
      JSON.stringify({ category: "activity.changed", requirement_id: "r-1" }),
    );
    source.fail();
    source.open();

    expect(onChange).toHaveBeenCalledOnce();
    expect(states).toEqual(["connecting", "reconnecting", "connected"]);
    unsubscribe();
    expect(states.at(-1)).toBe("closed_or_error");
  });

  it("uses same-origin /events, refetches named hints, and detects reconnects", () => {
    vi.stubGlobal("EventSource", FakeEventSource);
    const onChange = vi.fn();
    const onReconnect = vi.fn();
    const unsubscribe = subscribeToRequirementEvents({ onChange, onReconnect });
    const source = FakeEventSource.instances[0];

    expect(source.url).toBe("/events");
    source.fail();
    source.open();
    source.open();
    for (const category of [
      REQUIREMENT_CHANGED_EVENT,
      "conversation.changed",
      "readiness.changed",
      "activity.changed",
      "session.changed",
    ]) {
      source.emit(
        category,
        JSON.stringify({ category, requirement_id: "r-1" }),
      );
    }
    source.emit(
      "message",
      JSON.stringify({
        category: REQUIREMENT_CHANGED_EVENT,
        requirement_id: "r-1",
      }),
    );

    expect(onChange).toHaveBeenCalledTimes(6);
    expect(onReconnect).toHaveBeenCalledTimes(2);
    unsubscribe();
    source.emit(
      REQUIREMENT_CHANGED_EVENT,
      JSON.stringify({
        category: REQUIREMENT_CHANGED_EVENT,
        requirement_id: "r-1",
      }),
    );
    expect(source.closed).toBe(true);
    expect(onChange).toHaveBeenCalledTimes(6);
  });
});
