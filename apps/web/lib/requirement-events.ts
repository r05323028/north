export const REQUIREMENT_CHANGED_EVENT = "requirement.changed";

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

  source.addEventListener(REQUIREMENT_CHANGED_EVENT, onChanged);
  source.addEventListener("message", onMessage);
  source.onopen = onOpen;
  source.onerror = onError;

  return () => {
    source.removeEventListener(REQUIREMENT_CHANGED_EVENT, onChanged);
    source.removeEventListener("message", onMessage);
    source.onopen = null;
    source.onerror = null;
    source.close();
  };
}
