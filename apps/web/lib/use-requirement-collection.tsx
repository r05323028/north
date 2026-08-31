"use client";

import { useCallback, useEffect, useRef, useState } from "react";

import {
  listRequirementsAtPath,
  requirementsUrl,
  type Requirement,
  type RequirementQuery,
} from "@/lib/requirements";
import { subscribeToRequirementEvents } from "@/lib/requirement-events";

export type RequirementCollectionState = {
  requirements: Requirement[];
  loading: boolean;
  refreshing: boolean;
  error: string | null;
};

const initialState: RequirementCollectionState = {
  requirements: [],
  loading: true,
  refreshing: false,
  error: null,
};

export function useRequirementCollection(
  query: RequirementQuery = {},
): RequirementCollectionState {
  const path = requirementsUrl(query);
  const requestNumber = useRef(0);
  const [state, setState] = useState<RequirementCollectionState>(initialState);

  const fetchCollection = useCallback(
    async (clear: boolean) => {
      const currentRequest = ++requestNumber.current;
      setState((current) => ({
        requirements: clear ? [] : current.requirements,
        loading: clear || current.requirements.length === 0,
        refreshing: !clear && current.requirements.length > 0,
        error: null,
      }));

      try {
        const requirements = await listRequirementsAtPath(path);
        if (currentRequest !== requestNumber.current) return;
        setState({
          requirements,
          loading: false,
          refreshing: false,
          error: null,
        });
      } catch (cause) {
        if (currentRequest !== requestNumber.current) return;
        setState((current) => ({
          ...current,
          loading: false,
          refreshing: false,
          error:
            cause instanceof Error
              ? cause.message
              : "Unable to load requirements",
        }));
      }
    },
    [path],
  );

  useEffect(() => {
    void fetchCollection(true);
  }, [fetchCollection]);

  useEffect(() => {
    const refresh = () => {
      void fetchCollection(false);
    };
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") refresh();
    };

    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", onVisibilityChange);
    const unsubscribe = subscribeToRequirementEvents({
      onChange: refresh,
      onReconnect: refresh,
    });

    return () => {
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      unsubscribe();
    };
  }, [fetchCollection]);

  return state;
}
