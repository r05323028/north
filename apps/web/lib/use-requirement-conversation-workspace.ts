"use client";

import { useCallback, useEffect, useRef, useState } from "react";

import { ApiError } from "@/lib/api/client";
import {
  getActivityPage,
  getLatestClarificationRun,
  getReadiness,
} from "@/lib/api/clarification";
import type {
  ActivityItem,
  ClarificationRun,
  CurrentUser,
  ReadinessView,
} from "@/lib/api/contracts";
import { getConversationPage } from "@/lib/api/conversations";
import { getCurrentUser } from "@/lib/api/current-user";
import {
  appendConversationPage,
  buildConversationHistory,
  repairConversationHistory,
  type ConversationHistory,
} from "@/lib/conversation-pagination";
import { getRequirement, type Requirement } from "@/lib/requirements";
import {
  subscribeToRequirementEvents,
  type RequirementEventConnectionState,
} from "@/lib/requirement-events";

export const WORKSPACE_PAGE_LIMIT = 50;

type ActivityPageSlice = {
  offset: number;
  activities: ActivityItem[];
  next_offset: number | null;
};

type ActivityHistory = {
  pages: ActivityPageSlice[];
  activities: ActivityItem[];
  next_offset: number | null;
  prior_loaded_end_offset: number;
  reached_end: boolean;
};

export type WorkspaceResource =
  | "requirement"
  | "conversation"
  | "readiness"
  | "activity"
  | "session"
  | "current_user";

export type WorkspaceResourceErrors = Partial<
  Record<WorkspaceResource, string>
>;

export type RequirementConversationWorkspaceState = {
  requirement: Requirement | null;
  conversation: ConversationHistory | null;
  readiness: ReadinessView | null;
  activityHistory: ActivityHistory | null;
  activities: ActivityItem[];
  activity_next_offset: number | null;
  activity_reached_end: boolean;
  run: ClarificationRun | null;
  currentUser: CurrentUser | null;
  loading: boolean;
  refreshing: boolean;
  loadingConversationMore: boolean;
  loadingActivityMore: boolean;
  initialError: string | null;
  refreshError: string | null;
  resourceErrors: WorkspaceResourceErrors;
  connectionState: RequirementEventConnectionState;
};

const emptyState = (): RequirementConversationWorkspaceState => ({
  requirement: null,
  conversation: null,
  readiness: null,
  activityHistory: null,
  activities: [],
  activity_next_offset: null,
  activity_reached_end: false,
  run: null,
  currentUser: null,
  loading: true,
  refreshing: false,
  loadingConversationMore: false,
  loadingActivityMore: false,
  initialError: null,
  refreshError: null,
  resourceErrors: {},
  connectionState: "connecting",
});

function errorText(cause: unknown, resource: string): string {
  if (cause instanceof ApiError) {
    if (cause.code === "invalid_server_data") {
      return `${resource} returned invalid server data`;
    }
    if (cause.code) return `${resource} failed: ${cause.code}`;
    return `${resource} failed (${cause.status})`;
  }
  if (cause instanceof Error) return cause.message;
  return `${resource} failed`;
}

function compareActivities(left: ActivityItem, right: ActivityItem): number {
  if (left.created_at < right.created_at) return -1;
  if (left.created_at > right.created_at) return 1;
  return left.id - right.id;
}

function buildActivityHistory(pages: ActivityPageSlice[]): ActivityHistory {
  const byOffset = new Map(pages.map((page) => [page.offset, page]));
  const chain: ActivityPageSlice[] = [];
  const visited = new Set<number>();
  let offset = 0;
  let endOffset = 0;
  let nextOffset: number | null = null;
  let reachedEnd = false;

  while (!visited.has(offset)) {
    const page = byOffset.get(offset);
    if (!page) break;
    visited.add(offset);
    chain.push(page);
    endOffset = offset + page.activities.length;
    nextOffset = page.next_offset;
    if (nextOffset === null) {
      reachedEnd = true;
      break;
    }
    if (nextOffset <= offset) break;
    offset = nextOffset;
  }

  const activitiesById = new Map<number, ActivityItem>();
  for (const page of chain) {
    for (const activity of page.activities) {
      activitiesById.set(activity.id, activity);
    }
  }
  return {
    pages: chain,
    activities: [...activitiesById.values()].sort(compareActivities),
    next_offset: nextOffset,
    prior_loaded_end_offset: endOffset,
    reached_end: reachedEnd,
  };
}

async function loadActivityHistory(
  requirementId: string,
): Promise<ActivityHistory> {
  const page = await getActivityPage(requirementId, 0, WORKSPACE_PAGE_LIMIT);
  return buildActivityHistory([{ offset: 0, ...page }]);
}

async function repairActivityHistory(
  requirementId: string,
  previous: ActivityHistory,
): Promise<ActivityHistory> {
  const pages: ActivityPageSlice[] = [];
  let offset = 0;
  const requestedOffsets = new Set<number>();
  let endOffset = 0;

  while (!requestedOffsets.has(offset)) {
    requestedOffsets.add(offset);
    const page = await getActivityPage(
      requirementId,
      offset,
      WORKSPACE_PAGE_LIMIT,
    );
    pages.push({ offset, ...page });
    endOffset = offset + page.activities.length;
    const throughPriorRange = endOffset >= previous.prior_loaded_end_offset;
    const throughCurrentEnd = previous.reached_end && page.next_offset === null;
    if (previous.reached_end ? throughCurrentEnd : throughPriorRange) break;
    if (page.next_offset === null || page.next_offset <= offset) break;
    offset = page.next_offset;
  }

  return buildActivityHistory(pages);
}

function activityFromHistory(
  history: ActivityHistory,
): Pick<
  RequirementConversationWorkspaceState,
  | "activityHistory"
  | "activities"
  | "activity_next_offset"
  | "activity_reached_end"
> {
  return {
    activityHistory: history,
    activities: history.activities,
    activity_next_offset: history.next_offset,
    activity_reached_end: history.reached_end,
  };
}

export function useRequirementConversationWorkspace(requirementId: string) {
  const [state, setState] =
    useState<RequirementConversationWorkspaceState>(emptyState);
  const stateRef = useRef(state);
  stateRef.current = state;
  const lifecycleGeneration = useRef(0);
  const bundleGeneration = useRef(0);
  const conversationMoreGeneration = useRef(0);
  const activityMoreGeneration = useRef(0);
  const repairTimer = useRef<number | null>(null);
  const invalidateOutstandingWork = useCallback(() => {
    lifecycleGeneration.current += 1;
    bundleGeneration.current += 1;
    conversationMoreGeneration.current += 1;
    activityMoreGeneration.current += 1;
  }, []);

  const loadBundle = useCallback(
    async (initial: boolean): Promise<void> => {
      const lifecycle = lifecycleGeneration.current;
      const generation = ++bundleGeneration.current;
      const previous = stateRef.current;
      const previousConversation = initial ? null : previous.conversation;
      const previousActivity = initial ? null : previous.activityHistory;

      setState((current) => ({
        ...current,
        loading: initial,
        refreshing: !initial,
        initialError: initial ? null : current.initialError,
        refreshError: initial ? current.refreshError : null,
        resourceErrors: initial ? {} : current.resourceErrors,
      }));

      const conversationPromise = previousConversation
        ? repairConversationHistory(
            (offset, limit) =>
              getConversationPage(requirementId, offset, limit),
            previousConversation,
            WORKSPACE_PAGE_LIMIT,
          )
        : getConversationPage(requirementId, 0, WORKSPACE_PAGE_LIMIT).then(
            (page) => buildConversationHistory([{ offset: 0, page }]),
          );
      const activityPromise = previousActivity
        ? repairActivityHistory(requirementId, previousActivity)
        : loadActivityHistory(requirementId);
      const results = await Promise.allSettled([
        getRequirement(requirementId),
        conversationPromise,
        getReadiness(requirementId),
        activityPromise,
        getLatestClarificationRun(requirementId),
        getCurrentUser(),
      ]);

      if (
        lifecycle !== lifecycleGeneration.current ||
        generation !== bundleGeneration.current
      )
        return;
      setState((current) => {
        const resourceErrors: WorkspaceResourceErrors = {};
        let next = { ...current };
        const recordFailure = (
          index: number,
          resource: WorkspaceResource,
          label: string,
        ) => {
          const result = results[index];
          if (result.status === "rejected") {
            resourceErrors[resource] = errorText(result.reason, label);
          }
        };
        if (results[0].status === "fulfilled")
          next.requirement = results[0].value;
        recordFailure(0, "requirement", "Requirement");
        if (results[1].status === "fulfilled")
          next.conversation = results[1].value;
        recordFailure(1, "conversation", "Conversation");
        if (results[2].status === "fulfilled")
          next.readiness = results[2].value;
        recordFailure(2, "readiness", "Readiness");
        if (results[3].status === "fulfilled") {
          next = { ...next, ...activityFromHistory(results[3].value) };
        }
        recordFailure(3, "activity", "Activity");
        if (results[4].status === "fulfilled") next.run = results[4].value;
        recordFailure(4, "session", "Clarification run");
        if (results[5].status === "fulfilled")
          next.currentUser = results[5].value;
        recordFailure(5, "current_user", "Current user");

        const failures = Object.values(resourceErrors);
        return {
          ...next,
          loading: false,
          refreshing: false,
          initialError:
            initial && failures.length > 0
              ? failures.join(" · ")
              : current.initialError,
          refreshError:
            !initial && failures.length > 0 ? failures.join(" · ") : null,
          resourceErrors,
        };
      });
    },
    [requirementId],
  );

  const scheduleRepair = useCallback(() => {
    if (repairTimer.current !== null) return;
    repairTimer.current = window.setTimeout(() => {
      repairTimer.current = null;
      void loadBundle(false);
    }, 0);
  }, [loadBundle]);

  const refresh = useCallback(() => loadBundle(false), [loadBundle]);

  const loadMoreConversation = useCallback(async () => {
    const current = stateRef.current.conversation;
    const offset = current?.next_offset;
    if (!current || offset === null || offset === undefined) return;
    const lifecycle = lifecycleGeneration.current;
    const generation = ++conversationMoreGeneration.current;
    setState((value) => ({ ...value, loadingConversationMore: true }));
    try {
      const page = await getConversationPage(
        requirementId,
        offset,
        WORKSPACE_PAGE_LIMIT,
      );
      if (
        lifecycle !== lifecycleGeneration.current ||
        generation !== conversationMoreGeneration.current
      )
        return;
      setState((value) => ({
        ...value,
        conversation: value.conversation
          ? appendConversationPage(value.conversation, page, offset)
          : value.conversation,
        loadingConversationMore: false,
        resourceErrors: { ...value.resourceErrors, conversation: undefined },
      }));
    } catch (cause) {
      if (
        lifecycle !== lifecycleGeneration.current ||
        generation !== conversationMoreGeneration.current
      )
        return;
      setState((value) => ({
        ...value,
        loadingConversationMore: false,
        resourceErrors: {
          ...value.resourceErrors,
          conversation: errorText(cause, "Conversation"),
        },
      }));
    }
  }, [requirementId]);

  const loadMoreActivity = useCallback(async () => {
    const offset = stateRef.current.activity_next_offset;
    if (offset === null || offset === undefined) return;
    const lifecycle = lifecycleGeneration.current;
    const generation = ++activityMoreGeneration.current;
    setState((value) => ({ ...value, loadingActivityMore: true }));
    try {
      const page = await getActivityPage(
        requirementId,
        offset,
        WORKSPACE_PAGE_LIMIT,
      );
      if (
        lifecycle !== lifecycleGeneration.current ||
        generation !== activityMoreGeneration.current
      )
        return;
      setState((value) => {
        const existing: ActivityPageSlice = {
          offset: 0,
          activities: value.activities,
          next_offset: offset,
        };
        const history = value.activityHistory
          ? buildActivityHistory([
              ...value.activityHistory.pages,
              { offset, ...page },
            ])
          : buildActivityHistory([existing, { offset, ...page }]);
        return {
          ...value,
          ...activityFromHistory(history),
          loadingActivityMore: false,
          resourceErrors: { ...value.resourceErrors, activity: undefined },
        };
      });
    } catch (cause) {
      if (
        lifecycle !== lifecycleGeneration.current ||
        generation !== activityMoreGeneration.current
      )
        return;
      setState((value) => ({
        ...value,
        loadingActivityMore: false,
        resourceErrors: {
          ...value.resourceErrors,
          activity: errorText(cause, "Activity"),
        },
      }));
    }
  }, [requirementId]);

  const applyRequirement = useCallback(
    (requirement: Requirement) => {
      if (requirement.id !== requirementId) return;
      setState((current) => ({
        ...current,
        requirement,
        resourceErrors: {
          ...current.resourceErrors,
          requirement: undefined,
        },
      }));
    },
    [requirementId],
  );

  const applyRun = useCallback(
    (run: ClarificationRun) => {
      if (run.requirement_id !== requirementId) return;
      setState((current) => ({
        ...current,
        run,
        resourceErrors: { ...current.resourceErrors, session: undefined },
      }));
    },
    [requirementId],
  );

  useEffect(() => {
    invalidateOutstandingWork();
    setState(emptyState());
    void loadBundle(true);
    return () => {
      invalidateOutstandingWork();
    };
  }, [invalidateOutstandingWork, loadBundle]);

  useEffect(() => {
    const onFocus = () => scheduleRepair();
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") scheduleRepair();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibilityChange);
    const unsubscribe = subscribeToRequirementEvents({
      requirementId,
      onChange: scheduleRepair,
      onReconnect: scheduleRepair,
      onStateChange: (connectionState) =>
        setState((current) => ({ ...current, connectionState })),
    });
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      unsubscribe();
      if (repairTimer.current !== null) {
        window.clearTimeout(repairTimer.current);
        repairTimer.current = null;
      }
    };
  }, [requirementId, scheduleRepair]);

  return {
    ...state,
    refresh,
    loadMoreConversation,
    loadMoreActivity,
    applyRequirement,
    applyRun,
  };
}
