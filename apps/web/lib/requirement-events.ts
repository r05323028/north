export const REQUIREMENT_CHANGED_EVENT = "requirement.changed";

const INVALIDATION_EVENTS = [
  REQUIREMENT_CHANGED_EVENT,
  "conversation.changed",
  "readiness.changed",
  "activity.changed",
  "session.changed",
] as const;

type RequirementEventSubscription = {
  onChange: () => void;
  onReconnect?: () => void;
};

export function subscribeToRequirementEvents({
  onChange,
  onReconnect,
}: RequirementEventSubscription): () => void {
  if (typeof window === "undefined" || typeof EventSource === "undefined") {
    return () => undefined;
  }

  const source = new EventSource("/events");
  let opened = false;
  let failedBeforeOpen = false;
  const onOpen = () => {
    if (opened || failedBeforeOpen) onReconnect?.();
    opened = true;
    failedBeforeOpen = false;
  };
  const onError = () => {
    if (!opened) failedBeforeOpen = true;
  };
  const onChanged = () => onChange();
  const onMessage = (event: Event) => {
    const data = (event as MessageEvent<string>).data;
    if (typeof data !== "string") return;

    try {
      const hint: unknown = JSON.parse(data);
      if (
        typeof hint === "object" &&
        hint !== null &&
        "category" in hint &&
        hint.category === REQUIREMENT_CHANGED_EVENT
      ) {
        onChange();
      }
    } catch {
      // Malformed notification cannot replace canonical HTTP data.
    }
  };

  INVALIDATION_EVENTS.forEach((eventName) => {
    source.addEventListener(eventName, onChanged);
  });
  source.addEventListener("message", onMessage);
  source.onopen = onOpen;
  source.onerror = onError;

  return () => {
    INVALIDATION_EVENTS.forEach((eventName) => {
      source.removeEventListener(eventName, onChanged);
    });
    source.removeEventListener("message", onMessage);
    source.onopen = null;
    source.onerror = null;
    source.close();
  };
}
