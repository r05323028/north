import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "@/lib/api/client";
import type {
  ActivityItem,
  ClarificationRun,
  CurrentUser,
  Message,
  ReadinessView,
  ReviewPacket,
} from "@/lib/api/contracts";
import type { Requirement } from "@/lib/requirements";
import type { RequirementConversationWorkspaceState } from "@/lib/use-requirement-conversation-workspace";

const mocks = vi.hoisted(() => {
  class MockClarificationUnavailableError extends Error {
    requirement: Requirement;
    run: ClarificationRun;

    constructor(requirement: Requirement, run: ClarificationRun) {
      super("clarification unavailable");
      this.name = "ClarificationUnavailableError";
      this.requirement = requirement;
      this.run = run;
    }
  }

  return {
    useWorkspace: vi.fn(),
    postMessage: vi.fn(),
    start: vi.fn(),
    dispatch: vi.fn(),
    cancel: vi.fn(),
    edit: vi.fn(),
    acceptReview: vi.fn(),
    rejectReview: vi.fn(),
    requestChangesReview: vi.fn(),
    reopenReview: vi.fn(),
    MockClarificationUnavailableError,
  };
});

vi.mock("@/lib/use-requirement-conversation-workspace", () => ({
  useRequirementConversationWorkspace: mocks.useWorkspace,
}));
vi.mock("@/lib/api/conversations", () => ({
  postRequesterMessage: mocks.postMessage,
}));
vi.mock("@/lib/api/clarification", () => ({
  ClarificationUnavailableError: mocks.MockClarificationUnavailableError,
  cancelClarification: mocks.cancel,
  dispatchClarificationMessage: mocks.dispatch,
  startClarification: mocks.start,
}));
vi.mock("@/lib/api/review", () => ({
  acceptRequirementReview: mocks.acceptReview,
  rejectRequirementReview: mocks.rejectReview,
  requestRequirementChanges: mocks.requestChangesReview,
  reopenRequirement: mocks.reopenReview,
}));
vi.mock("@/lib/requirements", () => ({
  editRequirement: mocks.edit,
  requirementStatusLabels: {
    draft: "Draft",
    discussing: "Discussing",
    ready: "Ready",
    accepted: "Accepted",
    rejected: "Rejected",
  },
}));

import { RequirementConversationWorkspace } from "@/components/requirement-conversation-workspace";
import { RequirementReviewPanel } from "@/components/requirement-review-panel";

const requirement: Requirement = {
  id: "requirement-1",
  title: "Account recovery",
  description: "Users recover access without support intervention.",
  summary: "Self-service recovery.",
  acceptance_criteria: ["Recovery link expires"],
  assumptions: ["Email delivery exists"],
  open_questions: ["Which identity provider?"],
  status: "ready",
  revision: 4,
  state_version: 9,
  created_by: "owner-1",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
};

const requester: CurrentUser = {
  id: "requester-1",
  email: "requester@example.com",
  role: "Requester",
};

const messages: Message[] = [
  {
    id: "message-1",
    conversation_id: "conversation-1",
    author_user_id: "requester-1",
    kind: "requester",
    body: "Please clarify recovery scope.",
    created_at: "2026-01-03T00:00:00Z",
  },
  {
    id: "message-2",
    conversation_id: "conversation-1",
    author_user_id: null,
    kind: "agent",
    body: "Which recovery channels must be supported?",
    created_at: "2026-01-03T00:01:00Z",
  },
  {
    id: "message-3",
    conversation_id: "conversation-1",
    author_user_id: null,
    kind: "system",
    body: "Clarification started.",
    created_at: "2026-01-03T00:02:00Z",
  },
];

const activities: ActivityItem[] = [
  {
    id: 1,
    event_id: "event-1",
    session_id: "run-a",
    activity: "Runtime started",
    created_at: "2026-01-03T00:02:00Z",
  },
];

const readiness: ReadinessView = {
  id: "assessment-1",
  event_id: "event-2",
  session_id: "run-a",
  daemon_event_seq: 3,
  requirement_revision: 4,
  verdict: "ready",
  blockers: [],
  assumptions: ["Email provider is configured"],
  repositories_reviewed: [
    {
      repository_id: "repository-1",
      commit_sha: "abcdef0123456789abcdef0123456789abcdef01",
    },
  ],
  outcome: "accepted",
  rejection_reason: null,
  assessed_at_ms: 100,
  accepted_state_version: 9,
  created_at: "2026-01-03T00:03:00Z",
  current: false,
};

const reviewPacket = {
  assessment_id: "assessment-1",
  requirement_revision: 4,
  requirement_state_version: 9,
  goal: "Recover access",
  scope: "Email recovery",
  summary: "Self-service recovery.",
  acceptance_criteria: ["Recovery link expires"],
  assumptions: ["Email exists"],
  open_questions: [],
  blockers: [],
  assessment_assumptions: ["Provider is configured"],
  repositories_reviewed: [
    { repository_id: "repository-1", commit_sha: "a".repeat(40) },
  ],
};

type ReviewPanelHarnessProps = {
  requirement: Requirement;
  reviewPacket: ReviewPacket | null;
  currentUser: CurrentUser;
};

function ReviewPanelHarness({
  requirement,
  reviewPacket,
  currentUser,
}: ReviewPanelHarnessProps) {
  const [currentRequirement, setCurrentRequirement] = useState(requirement);
  const [currentPacket, setCurrentPacket] = useState<ReviewPacket | null>(
    reviewPacket,
  );
  const [refreshError, setRefreshError] = useState<string | null>(null);

  function applyRequirement(canonical: Requirement) {
    setCurrentRequirement(canonical);
    setCurrentPacket((packet) =>
      canonical.status === "ready" &&
      packet !== null &&
      packet.requirement_revision === canonical.revision &&
      packet.requirement_state_version === canonical.state_version
        ? packet
        : null,
    );
  }

  async function refresh() {
    setRefreshError("Canonical refresh failed.");
    throw new Error("canonical refresh failed");
  }

  return (
    <>
      <output data-testid="canonical-status">
        {currentRequirement.status}
      </output>
      {refreshError && <p role="status">{refreshError}</p>}
      <RequirementReviewPanel
        currentUser={currentUser}
        onApplyRequirementAction={applyRequirement}
        onRefreshAction={refresh}
        refreshing={false}
        requirement={currentRequirement}
        reviewPacket={currentPacket}
      />
    </>
  );
}

function run(overrides: Partial<ClarificationRun> = {}): ClarificationRun {
  return {
    run_id: "run-a",
    requirement_id: requirement.id,
    start_message_id: "message-1",
    phase: "active",
    status: "running",
    cancel_requested: false,
    attempt_count: 1,
    next_retry_at: null,
    failure_reason: null,
    retrying: false,
    failed: false,
    created_at: "2026-01-03T00:02:00Z",
    updated_at: "2026-01-03T00:02:00Z",
    last_activity_at: "2026-01-03T00:02:00Z",
    ...overrides,
  };
}

function workspace(
  overrides: Partial<RequirementConversationWorkspaceState> = {},
) {
  return {
    requirement,
    conversation: {
      conversation: {
        id: "conversation-1",
        requirement_id: requirement.id,
        created_at: "2026-01-01T00:00:00Z",
      },
      pages: [],
      messages,
      next_offset: null,
      prior_loaded_end_offset: messages.length,
      reached_end: true,
    },
    readiness,
    reviewPacket,
    activities,
    activityHistory: null,
    activity_next_offset: null,
    activity_reached_end: true,
    run: null,
    currentUser: requester,
    loading: false,
    refreshing: false,
    loadingConversationMore: false,
    loadingActivityMore: false,
    initialError: null,
    refreshError: null,
    resourceErrors: {},
    connectionState: "connected" as const,
    refresh: vi.fn().mockResolvedValue(undefined),
    loadMoreConversation: vi.fn().mockResolvedValue(undefined),
    loadMoreActivity: vi.fn().mockResolvedValue(undefined),
    applyRequirement: vi.fn(),
    applyRun: vi.fn(),
    ...overrides,
  };
}

function mount(value = workspace()) {
  mocks.useWorkspace.mockReturnValue(value);
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  return { container, root };
}

async function renderWorkspace(mounted: ReturnType<typeof mount>) {
  await act(async () => {
    mounted.root.render(
      <RequirementConversationWorkspace id={requirement.id} />,
    );
    await settle();
  });
}

async function renderReviewPanel(props: ReviewPanelHarnessProps) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<ReviewPanelHarness {...props} />);
    await settle();
  });
  return { container, root };
}

function expectCanonicalRefreshFailure(
  container: HTMLElement,
  status: Requirement["status"],
  buttonLabel: string,
) {
  expect(
    container.querySelector('[data-testid="canonical-status"]')?.textContent,
  ).toBe(status);
  expect(
    container.querySelector(`button[aria-label="${buttonLabel}"]`),
  ).toBeNull();
  expect(container.textContent).toContain("Canonical refresh failed.");
  expect(container.textContent).not.toContain("Review action failed");
}

async function acknowledgeRefreshedPacket(
  mounted: ReturnType<typeof mount>,
  value: ReturnType<typeof workspace>,
) {
  mocks.useWorkspace.mockReturnValue({
    ...value,
    reviewPacket: { ...reviewPacket },
  });
  await act(async () => {
    mounted.root.render(
      <RequirementConversationWorkspace id={requirement.id} />,
    );
    await settle();
  });
  const acknowledge = mounted.container.querySelector<HTMLButtonElement>(
    'button[aria-label="Review refreshed packet"]',
  );
  if (!acknowledge) throw new Error("packet acknowledgement missing");
  await act(async () => {
    acknowledge.click();
    await settle();
  });
}

async function renderAndSend(mounted: ReturnType<typeof mount>, body: string) {
  await renderWorkspace(mounted);
  const input = mounted.container.querySelector<HTMLTextAreaElement>(
    "#clarification-message",
  );
  const send = mounted.container.querySelector<HTMLButtonElement>(
    'button[aria-label="Send clarification message"]',
  );
  if (!input || !send) throw new Error("composer controls missing");
  await act(async () => {
    setValue(input, body);
    send.click();
    await settle();
  });
  return input;
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function setValue(
  element: HTMLInputElement | HTMLTextAreaElement,
  value: string,
) {
  const setter = Object.getOwnPropertyDescriptor(
    Object.getPrototypeOf(element),
    "value",
  )?.set;
  setter?.call(element, value);
  element.dispatchEvent(new Event("input", { bubbles: true }));
}

afterEach(() => {
  vi.resetAllMocks();
  document.body.replaceChildren();
});

describe("RequirementConversationWorkspace", () => {
  it("renders canonical conversation roles, activity, readiness, citation, and separate panes", () => {
    mocks.useWorkspace.mockReturnValue(workspace());
    const html = renderToStaticMarkup(
      <RequirementConversationWorkspace id={requirement.id} />,
    );

    expect(html).toContain('data-testid="conversation-pane"');
    expect(html).toContain('data-testid="live-requirement-panel"');
    expect(html).toContain("Please clarify recovery scope.");
    expect(html).toContain("Agent");
    expect(html).toContain("System");
    expect(html).toContain("Runtime started");
    expect(html).toContain("abcdef0123456789abcdef0123456789abcdef01");
    expect(html).toContain("Stale");
    expect(html).toContain("Live updates: connected");
    expect(html).not.toContain("checkout");
    expect(html).not.toContain("tool trace");
  });

  it("keeps readiness rejection canonical and refuses terminal structured edits", () => {
    const rejected = {
      ...readiness,
      outcome: "rejected" as const,
      verdict: "needs_clarification" as const,
      rejection_reason: "blockers_present",
      current: false,
    };
    mocks.useWorkspace.mockReturnValue(workspace({ readiness: rejected }));
    const rejectedHtml = renderToStaticMarkup(
      <RequirementConversationWorkspace id={requirement.id} />,
    );
    expect(rejectedHtml).toContain("blockers_present");
    expect(rejectedHtml).toContain("Requester");

    mocks.useWorkspace.mockReturnValue(
      workspace({ requirement: { ...requirement, status: "accepted" } }),
    );
    const terminalHtml = renderToStaticMarkup(
      <RequirementConversationWorkspace id={requirement.id} />,
    );
    expect(terminalHtml).toContain("Terminal Requirement");
    expect(terminalHtml).not.toContain('aria-label="Edit requirement"');
  });

  it("renders loading and retained-refresh error states without blanking workspace", () => {
    mocks.useWorkspace.mockReturnValue(
      workspace({ requirement: null, conversation: null, loading: true }),
    );
    const loading = renderToStaticMarkup(
      <RequirementConversationWorkspace id={requirement.id} />,
    );
    expect(loading).toContain("Loading conversation…");

    const retained = workspace({
      refreshError: "refresh failed",
      resourceErrors: { readiness: "Readiness failed" },
    });
    mocks.useWorkspace.mockReturnValue(retained);
    const html = renderToStaticMarkup(
      <RequirementConversationWorkspace id={requirement.id} />,
    );
    expect(html).toContain(
      "Refresh failed. Showing last successful canonical data.",
    );
    expect(html).toContain("Account recovery");
    expect(html).toContain("Readiness failed");
  });

  it("persists before starting initial message and never dispatches it", async () => {
    const startResult = run({ phase: "active", status: "starting" });
    const persisted = { ...messages[0], id: "message-new", body: "New scope" };
    mocks.postMessage.mockResolvedValue(persisted);
    mocks.start.mockResolvedValue(startResult);
    const value = workspace();
    const { container, root } = mount(value);

    await renderAndSend({ container, root }, "New scope");

    expect(mocks.postMessage).toHaveBeenCalledWith(requirement.id, "New scope");
    expect(mocks.start).toHaveBeenCalledWith(requirement.id, {
      message_id: "message-new",
      expected_state_version: requirement.state_version,
    });
    expect(mocks.dispatch).not.toHaveBeenCalled();
    expect(mocks.postMessage.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.start.mock.invocationCallOrder[0],
    );
    root.unmount();
    container.remove();
  });

  it("dispatches later messages only to explicit active run and retains draft on persistence failure", async () => {
    const persisted = { ...messages[0], id: "message-new", body: "Follow-up" };
    const active = workspace({ run: run({ run_id: "run-a" }) });
    mocks.postMessage.mockResolvedValueOnce(persisted);
    mocks.dispatch.mockResolvedValueOnce(run({ run_id: "run-a" }));
    const mounted = mount(active);
    const input = await renderAndSend(mounted, "Follow-up");
    expect(mocks.dispatch).toHaveBeenCalledWith(
      requirement.id,
      "run-a",
      "message-new",
    );
    expect(mocks.start).not.toHaveBeenCalled();

    mocks.postMessage.mockRejectedValueOnce(new Error("network"));
    const send = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Send clarification message"]',
    );
    if (!send) throw new Error("composer controls missing");
    await act(async () => {
      setValue(input, "Keep this draft");
      send.click();
      await settle();
    });
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.dispatch).toHaveBeenCalledOnce();
    expect(input.value).toBe("Keep this draft");
    expect(mounted.container.textContent).toContain(
      "Message could not be saved",
    );
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("blocks awaiting assignment, retries recorded start message, and supports cancellation", async () => {
    const awaiting = run({
      phase: "awaiting_assignment",
      status: "unavailable",
      start_message_id: "message-start",
    });
    const value = workspace({ run: awaiting });
    mocks.start.mockResolvedValueOnce(
      run({ phase: "active", status: "starting" }),
    );
    mocks.cancel.mockResolvedValueOnce(
      run({ phase: "terminal", status: "completed", cancel_requested: true }),
    );
    const mounted = mount(value);
    await act(async () => {
      mounted.root.render(
        <RequirementConversationWorkspace id={requirement.id} />,
      );
      await settle();
    });
    const input = mounted.container.querySelector<HTMLTextAreaElement>(
      "#clarification-message",
    );
    const send = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Send clarification message"]',
    );
    const retry = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Retry clarification start"]',
    );
    const cancel = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Cancel clarification"]',
    );
    if (!input || !send || !retry || !cancel)
      throw new Error("awaiting controls missing");
    setValue(input, "Unsent draft");
    expect(send.disabled).toBe(true);
    expect(input.value).toBe("Unsent draft");

    await act(async () => {
      retry.click();
      await settle();
    });
    expect(mocks.start).toHaveBeenCalledWith(requirement.id, {
      message_id: "message-start",
      expected_state_version: requirement.state_version,
    });
    expect(mocks.postMessage).not.toHaveBeenCalled();

    await act(async () => {
      cancel.click();
      await settle();
    });
    expect(mocks.cancel).toHaveBeenCalledWith(requirement.id, "run-a");
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("keeps saved history when dispatch conflicts and does not reroute", async () => {
    const persisted = { ...messages[0], id: "message-new", body: "Follow-up" };
    mocks.postMessage.mockResolvedValueOnce(persisted);
    mocks.dispatch.mockRejectedValueOnce(
      new ApiError(409, "conflict", "conflict"),
    );
    const mounted = mount(workspace({ run: run({ run_id: "run-a" }) }));
    await renderAndSend(mounted, "Follow-up");
    expect(mocks.dispatch).toHaveBeenCalledWith(
      requirement.id,
      "run-a",
      "message-new",
    );
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mounted.container.textContent).toContain(
      "Message saved, but it was not sent",
    );
    await act(async () => {
      mounted.root.unmount();
    });
    mounted.container.remove();
  });

  it("handles unavailable start without retrying or creating another run", async () => {
    const awaiting = run({
      phase: "awaiting_assignment",
      status: "unavailable",
    });
    const persisted = { ...messages[0], id: "message-start", body: "Start" };
    mocks.postMessage.mockResolvedValueOnce(persisted);
    mocks.start.mockRejectedValueOnce(
      new mocks.MockClarificationUnavailableError(requirement, awaiting),
    );
    const value = workspace();
    const mounted = mount(value);
    await renderAndSend(mounted, "Start");
    expect(mocks.postMessage).toHaveBeenCalledOnce();
    expect(mocks.start).toHaveBeenCalledOnce();
    expect(mocks.dispatch).not.toHaveBeenCalled();
    expect(value.applyRequirement).toHaveBeenCalledWith(requirement);
    expect(value.applyRun).toHaveBeenCalledWith(awaiting);
    expect(mounted.container.textContent).toContain(
      "Runtime unavailable before assignment",
    );
    await act(async () => {
      mounted.root.unmount();
    });
    mounted.container.remove();
  });

  it("keeps cancellation-pending run occupied and targets same run", async () => {
    const pending = run({ run_id: "run-a", cancel_requested: true });
    mocks.cancel.mockResolvedValueOnce(
      run({
        run_id: "run-a",
        phase: "terminal",
        status: "completed",
        cancel_requested: true,
      }),
    );
    const mounted = mount(workspace({ run: pending }));
    await renderWorkspace(mounted);
    const send = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Send clarification message"]',
    );
    const cancel = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Repeat cancellation"]',
    );
    if (!send || !cancel) throw new Error("cancellation controls missing");
    expect(send.disabled).toBe(true);
    await act(async () => {
      cancel.click();
      await settle();
    });
    expect(mocks.cancel).toHaveBeenCalledWith(requirement.id, "run-a");
    await act(async () => {
      mounted.root.unmount();
    });
    mounted.container.remove();
  });

  it("starts a new explicit run after terminal completion", async () => {
    const persisted = { ...messages[0], id: "message-next", body: "Next run" };
    mocks.postMessage.mockResolvedValueOnce(persisted);
    mocks.start.mockResolvedValueOnce(
      run({
        run_id: "run-b",
        phase: "active",
        status: "starting",
        start_message_id: "message-next",
      }),
    );
    const mounted = mount(
      workspace({
        run: run({ run_id: "run-a", phase: "terminal", status: "completed" }),
      }),
    );
    await renderAndSend(mounted, "Next run");
    expect(mocks.start).toHaveBeenCalledWith(requirement.id, {
      message_id: "message-next",
      expected_state_version: requirement.state_version,
    });
    expect(mocks.dispatch).not.toHaveBeenCalled();
    await act(async () => {
      mounted.root.unmount();
    });
    mounted.container.remove();
  });

  it("uses displayed state version and requires reconciliation after stale structured edit", async () => {
    const canonical = {
      ...requirement,
      title: "Canonical title",
      state_version: 10,
    };
    const value = workspace();
    mocks.edit.mockRejectedValueOnce(new ApiError(409, "conflict", "conflict"));
    const mounted = mount(value);
    await act(async () => {
      mounted.root.render(
        <RequirementConversationWorkspace id={requirement.id} />,
      );
      await settle();
    });
    const edit = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Edit requirement"]',
    );
    if (!edit) throw new Error("edit control missing");
    await act(async () => {
      edit.click();
      await settle();
    });
    const title = mounted.container.querySelector<HTMLInputElement>(
      "#requirement-edit-title",
    );
    const save = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Save requirement"]',
    );
    if (!title || !save) throw new Error("editor controls missing");
    setValue(title, "Unsaved title");
    await act(async () => {
      save.click();
      await settle();
    });
    expect(mocks.edit).toHaveBeenCalledWith(
      requirement.id,
      expect.objectContaining({
        expected_state_version: requirement.state_version,
        title: "Unsaved title",
      }),
    );
    expect(title.value).toBe("Unsaved title");
    expect(mounted.container.textContent).toContain("Reconcile draft");

    mocks.useWorkspace.mockReturnValue({ ...value, requirement: canonical });
    await act(async () => {
      mounted.root.render(
        <RequirementConversationWorkspace id={requirement.id} />,
      );
      await settle();
    });
    const reconcile = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Use latest canonical requirement"]',
    );
    if (!reconcile) throw new Error("reconcile control missing");
    await act(async () => {
      reconcile.click();
      await settle();
    });
    const reconciledTitle = mounted.container.querySelector<HTMLInputElement>(
      "#requirement-edit-title",
    );
    expect(reconciledTitle?.value).toBe("Canonical title");
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("renders canonical packet evidence without requester review controls", () => {
    mocks.useWorkspace.mockReturnValue(workspace());
    const html = renderToStaticMarkup(
      <RequirementConversationWorkspace id={requirement.id} />,
    );

    expect(html).toContain("Recover access");
    expect(html).toContain("Assessment assumptions");
    expect(html).toContain("Repositories reviewed");
    expect(html).not.toContain('aria-label="Accept Requirement"');
    expect(html).not.toContain('aria-label="Request Changes"');
  });

  it("sends reviewer decisions with packet identity and clears feedback after success", async () => {
    const reviewer = { ...requester, role: "RequirementManager" as const };
    mocks.acceptReview.mockResolvedValueOnce(requirement);
    mocks.requestChangesReview.mockResolvedValueOnce(requirement);
    const value = workspace({ currentUser: reviewer });
    const mounted = mount(value);
    await renderWorkspace(mounted);

    const accept = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Accept Requirement"]',
    );
    if (!accept) throw new Error("accept control missing");
    await act(async () => {
      accept.click();
      await settle();
    });
    expect(mocks.acceptReview).toHaveBeenCalledWith(requirement.id, {
      assessment_id: reviewPacket.assessment_id,
      expected_state_version: reviewPacket.requirement_state_version,
    });
    expect(value.applyRequirement).toHaveBeenCalledWith(requirement);

    const feedback =
      mounted.container.querySelector<HTMLTextAreaElement>("#review-feedback");
    const requestChanges = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Request Changes"]',
    );
    if (!feedback || !requestChanges)
      throw new Error("Request Changes controls missing");
    setValue(feedback, "Clarify recovery expiry.");
    await act(async () => {
      requestChanges.click();
      await settle();
    });
    expect(mocks.requestChangesReview).toHaveBeenCalledWith(requirement.id, {
      assessment_id: reviewPacket.assessment_id,
      expected_state_version: reviewPacket.requirement_state_version,
      feedback: "Clarify recovery expiry.",
    });
    expect(feedback.value).toBe("");
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("keeps Accepted Requirement after refresh failure", async () => {
    const reviewer = { ...requester, role: "RequirementManager" as const };
    const accepted = {
      ...requirement,
      status: "accepted" as const,
      state_version: 10,
    };
    mocks.acceptReview.mockResolvedValueOnce(accepted);
    const mounted = await renderReviewPanel({
      currentUser: reviewer,
      requirement,
      reviewPacket,
    });
    const accept = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Accept Requirement"]',
    );
    if (!accept) throw new Error("accept control missing");
    await act(async () => {
      accept.click();
      await settle();
    });
    expectCanonicalRefreshFailure(
      mounted.container,
      "accepted",
      "Accept Requirement",
    );
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("keeps Discussing and clears submitted feedback after refresh failure", async () => {
    const reviewer = { ...requester, role: "RequirementManager" as const };
    const discussing = {
      ...requirement,
      status: "discussing" as const,
      state_version: 10,
    };
    mocks.requestChangesReview.mockResolvedValueOnce(discussing);
    const mounted = await renderReviewPanel({
      currentUser: reviewer,
      requirement,
      reviewPacket,
    });
    const feedback =
      mounted.container.querySelector<HTMLTextAreaElement>("#review-feedback");
    const requestChanges = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Request Changes"]',
    );
    if (!feedback || !requestChanges)
      throw new Error("Request Changes controls missing");
    await act(async () => {
      setValue(feedback, "Clarify account scope.");
      await settle();
    });
    await act(async () => {
      requestChanges.click();
      await settle();
    });
    expectCanonicalRefreshFailure(
      mounted.container,
      "discussing",
      "Accept Requirement",
    );
    expect(mounted.container.querySelector("#review-feedback")).toBeNull();
    expect(mounted.container.textContent).not.toContain(
      "Clarify account scope.",
    );
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("keeps Discussing after Reopen refresh failure", async () => {
    const reviewer = { ...requester, role: "Admin" as const };
    const rejected = { ...requirement, status: "rejected" as const };
    const discussing = {
      ...rejected,
      status: "discussing" as const,
      state_version: 10,
    };
    mocks.reopenReview.mockResolvedValueOnce(discussing);
    const mounted = await renderReviewPanel({
      currentUser: reviewer,
      requirement: rejected,
      reviewPacket: null,
    });
    const reopen = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Reopen Requirement"]',
    );
    if (!reopen) throw new Error("reopen control missing");
    await act(async () => {
      reopen.click();
      await settle();
    });
    expectCanonicalRefreshFailure(
      mounted.container,
      "discussing",
      "Reopen Requirement",
    );
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("reopens rejected Requirements with state version only", async () => {
    const reviewer = { ...requester, role: "Admin" as const };
    const rejected = { ...requirement, status: "rejected" as const };
    mocks.reopenReview.mockResolvedValueOnce({
      ...rejected,
      status: "discussing" as const,
      state_version: rejected.state_version + 1,
    });
    const mounted = mount(
      workspace({
        currentUser: reviewer,
        requirement: rejected,
        reviewPacket: null,
      }),
    );
    await renderWorkspace(mounted);
    const reopen = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Reopen Requirement"]',
    );
    if (!reopen) throw new Error("reopen control missing");
    await act(async () => {
      reopen.click();
      await settle();
    });
    expect(mocks.reopenReview).toHaveBeenCalledWith(requirement.id, {
      expected_state_version: rejected.state_version,
    });
    expect(mocks.reopenReview.mock.calls[0][1]).not.toHaveProperty(
      "assessment_id",
    );
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("repairs stale review without optimistic transition and requires refreshed acknowledgement", async () => {
    const reviewer = { ...requester, role: "Owner" as const };
    const value = workspace({ currentUser: reviewer });
    mocks.acceptReview
      .mockRejectedValueOnce(new ApiError(409, "conflict", "conflict"))
      .mockResolvedValueOnce(requirement);
    const mounted = mount(value);
    await renderWorkspace(mounted);
    const accept = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Accept Requirement"]',
    );
    if (!accept) throw new Error("accept control missing");
    await act(async () => {
      accept.click();
      await settle();
    });
    expect(mocks.acceptReview).toHaveBeenCalledOnce();
    expect(value.requirement?.status).toBe("ready");
    expect(accept.disabled).toBe(true);

    await acknowledgeRefreshedPacket(mounted, value);
    const refreshedAccept = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Accept Requirement"]',
    );
    if (!refreshedAccept) throw new Error("refreshed accept control missing");
    expect(refreshedAccept.disabled).toBe(false);
    await act(async () => {
      refreshedAccept.click();
      await settle();
    });
    expect(mocks.acceptReview).toHaveBeenCalledTimes(2);
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("preserves Request Changes feedback across stale repair", async () => {
    const reviewer = { ...requester, role: "RequirementManager" as const };
    mocks.requestChangesReview.mockRejectedValueOnce(
      new ApiError(409, "conflict", "conflict"),
    );
    const mounted = mount(workspace({ currentUser: reviewer }));
    await renderWorkspace(mounted);
    const feedback =
      mounted.container.querySelector<HTMLTextAreaElement>("#review-feedback");
    const requestChanges = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Request Changes"]',
    );
    if (!feedback || !requestChanges)
      throw new Error("Request Changes controls missing");
    setValue(feedback, "Keep this unsent review feedback.");
    await act(async () => {
      requestChanges.click();
      await settle();
    });
    expect(feedback.value).toBe("Keep this unsent review feedback.");
    expect(mounted.container.textContent).toContain("Review became stale");
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("keeps acknowledgement for unchanged identity and resets it for state or assessment changes", async () => {
    const reviewer = { ...requester, role: "Admin" as const };
    const value = workspace({ currentUser: reviewer });
    mocks.acceptReview.mockRejectedValueOnce(
      new ApiError(409, "conflict", "conflict"),
    );
    const mounted = mount(value);
    await renderWorkspace(mounted);
    const accept = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Accept Requirement"]',
    );
    if (!accept) throw new Error("accept control missing");
    await act(async () => {
      accept.click();
      await settle();
    });
    await acknowledgeRefreshedPacket(mounted, value);

    mocks.useWorkspace.mockReturnValue({
      ...value,
      reviewPacket: { ...reviewPacket },
    });
    await act(async () => {
      mounted.root.render(
        <RequirementConversationWorkspace id={requirement.id} />,
      );
      await settle();
    });
    expect(
      mounted.container.querySelector(
        'button[aria-label="Review refreshed packet"]',
      ),
    ).toBeNull();

    const changedRequirement = {
      ...requirement,
      revision: 5,
      state_version: 10,
    };
    mocks.useWorkspace.mockReturnValue({
      ...value,
      requirement: changedRequirement,
      reviewPacket: {
        ...reviewPacket,
        requirement_revision: 5,
        requirement_state_version: 10,
      },
    });
    await act(async () => {
      mounted.root.render(
        <RequirementConversationWorkspace id={requirement.id} />,
      );
      await settle();
    });
    expect(
      mounted.container.querySelector(
        'button[aria-label="Review refreshed packet"]',
      ),
    ).not.toBeNull();
    const changedAccept = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Accept Requirement"]',
    );
    expect(changedAccept?.disabled).toBe(true);

    mocks.useWorkspace.mockReturnValue({
      ...value,
      requirement: changedRequirement,
      reviewPacket: {
        ...reviewPacket,
        assessment_id: "assessment-2",
        requirement_revision: 5,
        requirement_state_version: 10,
      },
    });
    await act(async () => {
      mounted.root.render(
        <RequirementConversationWorkspace id={requirement.id} />,
      );
      await settle();
    });
    expect(
      mounted.container.querySelector(
        'button[aria-label="Review refreshed packet"]',
      ),
    ).not.toBeNull();
    mounted.root.unmount();
    mounted.container.remove();
  });

  it("requires refreshed Reopen acknowledgement and uses only state version", async () => {
    const reviewer = { ...requester, role: "Owner" as const };
    const rejected = { ...requirement, status: "rejected" as const };
    const value = workspace({
      currentUser: reviewer,
      requirement: rejected,
      reviewPacket: null,
    });
    mocks.reopenReview
      .mockRejectedValueOnce(new ApiError(409, "conflict", "conflict"))
      .mockResolvedValueOnce({
        ...rejected,
        status: "discussing" as const,
        state_version: rejected.state_version + 1,
      });
    const mounted = mount(value);
    await renderWorkspace(mounted);
    const reopen = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Reopen Requirement"]',
    );
    if (!reopen) throw new Error("reopen control missing");
    await act(async () => {
      reopen.click();
      await settle();
    });
    expect(mocks.reopenReview).toHaveBeenCalledOnce();
    expect(mounted.container.textContent).toContain("Review became stale");

    const refreshedRejected = { ...rejected, state_version: 10 };
    mocks.useWorkspace.mockReturnValue({
      ...value,
      requirement: refreshedRejected,
      reviewPacket: null,
    });
    await act(async () => {
      mounted.root.render(
        <RequirementConversationWorkspace id={requirement.id} />,
      );
      await settle();
    });
    const acknowledge = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Review refreshed Requirement"]',
    );
    if (!acknowledge) throw new Error("Requirement acknowledgement missing");
    await act(async () => {
      acknowledge.click();
      await settle();
    });
    const refreshedReopen = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Reopen Requirement"]',
    );
    if (!refreshedReopen) throw new Error("refreshed Reopen control missing");
    expect(refreshedReopen.disabled).toBe(false);
    await act(async () => {
      refreshedReopen.click();
      await settle();
    });
    expect(mocks.reopenReview).toHaveBeenLastCalledWith(requirement.id, {
      expected_state_version: 10,
    });
    expect(mocks.reopenReview.mock.calls[1][1]).not.toHaveProperty(
      "assessment_id",
    );
    mounted.root.unmount();
    mounted.container.remove();
  });
});
