export const REQUIREMENT_CHANGED_EVENT = "requirement.changed";

export const REQUIREMENT_EVENT_CATEGORIES = [
  REQUIREMENT_CHANGED_EVENT,
  "conversation.changed",
  "readiness.changed",
  "activity.changed",
  "session.changed",
] as const;

export type RequirementEventCategory =
  (typeof REQUIREMENT_EVENT_CATEGORIES)[number];

export type RequirementEvent = {
  category: RequirementEventCategory;
  requirement_id: string;
};

export type RequirementEventConnectionState =
  | "connecting"
  | "connected"
  | "reconnecting"
  | "closed_or_error";

type RequirementEventSubscription = {
  onChange: (hint?: RequirementEvent) => void;
  onReconnect?: () => void;
  onStateChange?: (state: RequirementEventConnectionState) => void;
  requirementId?: string;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isCategory(value: unknown): value is RequirementEventCategory {
  return (
    typeof value === "string" &&
    (REQUIREMENT_EVENT_CATEGORIES as readonly string[]).includes(value)
  );
}

export function parseRequirementEvent(
  value: unknown,
  expectedCategory?: string,
): RequirementEvent | null {
  if (!isRecord(value) || !isCategory(value.category)) return null;
  if (expectedCategory !== undefined && value.category !== expectedCategory) {
    return null;
  }
  return typeof value.requirement_id === "string" &&
    value.requirement_id.length > 0
    ? {
        category: value.category,
        requirement_id: value.requirement_id,
      }
    : null;
}

export function subscribeToRequirementEvents({
  onChange,
  onReconnect,
  onStateChange,
  requirementId,
}: RequirementEventSubscription): () => void {
  if (typeof window === "undefined" || typeof EventSource === "undefined") {
    onStateChange?.("closed_or_error");
    return () => undefined;
  }

  const source = new EventSource("/events");
  let opened = false;
  let failedBeforeOpen = false;
  let closed = false;
  const emitState = (state: RequirementEventConnectionState) => {
    if (!closed) onStateChange?.(state);
  };
  const dispatch = (event: Event, expectedCategory?: string) => {
    const data = (event as MessageEvent<string>).data;
    if (typeof data !== "string") return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(data) as unknown;
    } catch {
      return;
    }
    const hint = parseRequirementEvent(parsed, expectedCategory);
    if (!hint || (requirementId && hint.requirement_id !== requirementId))
      return;
    onChange(hint);
  };
  const onOpen = () => {
    if (opened || failedBeforeOpen) onReconnect?.();
    opened = true;
    failedBeforeOpen = false;
    emitState("connected");
  };
  const onError = () => {
    if (!opened) failedBeforeOpen = true;
    const readyState = (source as EventSource & { readyState?: number })
      .readyState;
    emitState(readyState === 2 ? "closed_or_error" : "reconnecting");
  };
  const listeners = new Map<string, EventListener>();
  for (const category of REQUIREMENT_EVENT_CATEGORIES) {
    const listener: EventListener = (event) => dispatch(event, category);
    listeners.set(category, listener);
    source.addEventListener(category, listener);
  }
  const messageListener: EventListener = (event) => dispatch(event);
  source.addEventListener("message", messageListener);
  source.onopen = onOpen;
  source.onerror = onError;
  emitState("connecting");

  return () => {
    if (closed) return;
    closed = true;
    for (const [category, listener] of listeners) {
      source.removeEventListener(category, listener);
    }
    source.removeEventListener("message", messageListener);
    source.onopen = null;
    source.onerror = null;
    source.close();
    onStateChange?.("closed_or_error");
  };
}
