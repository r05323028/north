import { expect, test, type Page, type Route } from "@playwright/test";

type Status = "draft" | "discussing" | "ready" | "accepted" | "rejected";

type Requirement = {
  id: string;
  title: string;
  description: string;
  summary: string;
  acceptance_criteria: string[];
  assumptions: string[];
  open_questions: string[];
  status: Status;
  revision: number;
  state_version: number;
  created_by: string;
  created_at: string;
  updated_at: string;
};

const eventHeaders = {
  "Cache-Control": "no-cache",
  "Content-Type": "text/event-stream",
};

function requirement(
  id: string,
  status: Status,
  title = "Login requirement",
): Requirement {
  return {
    id,
    title,
    description: "Users can sign in.",
    summary: "One account per user.",
    acceptance_criteria: ["A valid account can sign in."],
    assumptions: ["Email is unique."],
    open_questions: [],
    status,
    revision: 1,
    state_version: 1,
    created_by: "user-1",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

function jsonHeaders() {
  return { "Content-Type": "application/json" };
}

function isApiFetch(request: {
  resourceType: () => string;
  headers: () => Record<string, string>;
}) {
  return request.resourceType() === "fetch" && !request.headers().rsc;
}

test("Board repairs missed updates on reconnect, duplicate delayed hints, and refocus", async ({
  page,
}) => {
  let current = [requirement("r-1", "draft")];
  let collectionRequests = 0;
  let eventConnections = 0;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (!isApiFetch(request) || url.pathname !== "/requirements") {
      await route.continue();
      return;
    }
    collectionRequests += 1;
    await route.fulfill({
      body: JSON.stringify(current),
      headers: jsonHeaders(),
    });
  });
  await page.route("**/events", async (route) => {
    eventConnections += 1;
    if (eventConnections === 1) {
      await route.fulfill({
        body: "retry: 10\n\n",
        headers: eventHeaders,
      });
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
    const categories = [
      "requirement.changed",
      "conversation.changed",
      "readiness.changed",
      "activity.changed",
      "session.changed",
    ] as const;
    const hints = categories
      .map(
        (category) =>
          `event: ${category}\ndata: ${JSON.stringify({
            category,
            requirement_id: "r-1",
          })}\n\n`,
      )
      .join("");
    await route.fulfill({
      body: hints + hints,
      headers: eventHeaders,
    });
  });

  await page.goto("/");
  await expect(page.getByTestId("requirement-card-r-1")).toContainText("Draft");

  current = [requirement("r-1", "ready")];
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => collectionRequests).toBeGreaterThan(1);
  await expect(page.getByTestId("requirement-card-r-1")).toContainText("Ready");
  await expect.poll(() => eventConnections).toBeGreaterThan(1);
  await expect(page.getByTestId("requirement-card-r-1")).toHaveCount(1);
});

test("List sends search, status, creator, and updated sort to server", async ({
  page,
}) => {
  const requests: string[] = [];
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (!isApiFetch(request) || url.pathname !== "/requirements") {
      await route.continue();
      return;
    }
    requests.push(request.url());
    await route.fulfill({
      body: JSON.stringify([requirement("r-1", "ready")]),
      headers: jsonHeaders(),
    });
  });
  await page.route("**/events", async (route) => {
    await route.fulfill({ body: ":\n\n", headers: eventHeaders });
  });
  await page.route("**/auth/me", async (route) => {
    await route.fulfill({
      body: JSON.stringify({
        id: "user-1",
        email: "user@example.com",
        role: "Requester",
      }),
      headers: jsonHeaders(),
    });
  });

  await page.goto("/");
  await page.getByRole("button", { name: "List", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Requirement list" }),
  ).toBeVisible();
  await page.getByLabel("Search").fill("login flow");
  await page.getByLabel("Status").selectOption("ready");
  await page.getByLabel("Creator").fill("user-1");
  await page.getByLabel("Updated").selectOption("updated_asc");

  await expect
    .poll(() =>
      requests.some((request) => {
        const query = new URL(request).searchParams;
        return (
          query.get("search") === "login flow" &&
          query.get("status") === "ready" &&
          query.get("created_by") === "user-1" &&
          query.get("sort") === "updated_asc"
        );
      }),
    )
    .toBe(true);
});

test("create uses canonical response and opens requirement workspace", async ({
  page,
}) => {
  const created = requirement("created-1", "draft", "Created requirement");
  let createBody: unknown;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (!isApiFetch(request)) {
      await route.continue();
      return;
    }
    if (request.method() === "POST" && url.pathname === "/requirements") {
      createBody = JSON.parse(request.postData() ?? "{}");
      await route.fulfill({
        status: 201,
        body: JSON.stringify(created),
        headers: jsonHeaders(),
      });
      return;
    }
    if (
      request.method() === "GET" &&
      url.pathname === "/requirements/created-1"
    ) {
      await route.fulfill({
        body: JSON.stringify(created),
        headers: jsonHeaders(),
      });
      return;
    }
    if (
      request.method() === "GET" &&
      url.pathname === "/requirements/created-1/conversation"
    ) {
      await route.fulfill({
        body: JSON.stringify({
          id: "conversation-created-1",
          requirement_id: "created-1",
          created_at: created.created_at,
          messages: [],
          next_offset: null,
        }),
        headers: jsonHeaders(),
      });
      return;
    }
    if (
      request.method() === "GET" &&
      url.pathname === "/requirements/created-1/readiness"
    ) {
      await route.fulfill({
        body: JSON.stringify({ assessment: null }),
        headers: jsonHeaders(),
      });
      return;
    }
    if (
      request.method() === "GET" &&
      (url.pathname === "/requirements/created-1/activity" ||
        url.pathname === "/requirements/created-1/session")
    ) {
      await route.fulfill({
        body: JSON.stringify(
          url.pathname.endsWith("/activity")
            ? { activities: [], next_offset: null }
            : { session: null },
        ),
        headers: jsonHeaders(),
      });
      return;
    }
    if (url.pathname === "/requirements") {
      await route.fulfill({ body: "[]", headers: jsonHeaders() });
      return;
    }
    await route.continue();
  });
  await page.route("**/events", async (route) => {
    await route.fulfill({ body: ":\n\n", headers: eventHeaders });
  });
  await page.route("**/auth/me", async (route) => {
    await route.fulfill({
      body: JSON.stringify({
        id: "user-1",
        email: "user@example.com",
        role: "Requester",
      }),
      headers: jsonHeaders(),
    });
  });

  await page.goto("/");
  await page.getByRole("button", { name: "New requirement" }).first().click();
  await page.getByLabel("Title").fill("Created requirement");
  await page.getByLabel("Description").fill("A canonical description.");
  await page.getByRole("button", { name: "Create requirement" }).click();

  await expect(
    page.getByRole("heading", { name: "Created requirement" }),
  ).toBeVisible();
  await expect(
    page
      .getByTestId("live-requirement-panel")
      .getByText("Draft", { exact: true }),
  ).toBeVisible();
  expect(createBody).toEqual({
    title: "Created requirement",
    description: "A canonical description.",
  });
});

type WorkspaceMessage = {
  id: string;
  conversation_id: string;
  author_user_id: string;
  kind: "requester";
  body: string;
  created_at: string;
};

type WorkspaceRunFixture = {
  run_id: string;
  requirement_id: string;
  start_message_id: string;
  phase: "awaiting_assignment" | "active" | "terminal";
  status: "starting" | "running" | "completed" | "unavailable";
  cancel_requested: boolean;
  created_at: string;
  updated_at: string;
  last_activity_at: string;
};

type WorkspaceRouteState = {
  current: () => Requirement;
  messages: WorkspaceMessage[];
  requests: string[];
  session?: () => WorkspaceRunFixture | null;
  extra: (route: Route, url: URL) => Promise<boolean>;
};

async function installWorkspaceRoutes(
  page: Page,
  state: WorkspaceRouteState,
) {
  await page.route("**/auth/me", async (route) => {
    await route.fulfill({
      body: JSON.stringify({
        id: "user-1",
        email: "user@example.com",
        role: "Requester",
      }),
      headers: jsonHeaders(),
    });
  });
  await page.route("**/events", async (route) => {
    await route.fulfill({ body: ":\n\n", headers: eventHeaders });
  });
  await page.route("**/requirements/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (!isApiFetch(request)) {
      await route.continue();
      return;
    }
    state.requests.push(`${request.method()} ${url.pathname}${url.search}`);
    if (request.method() === "GET" && url.pathname === "/requirements/r-1") {
      await route.fulfill({
        body: JSON.stringify(state.current()),
        headers: jsonHeaders(),
      });
      return;
    }
    if (
      request.method() === "GET" &&
      url.pathname === "/requirements/r-1/conversation"
    ) {
      await route.fulfill({
        body: JSON.stringify({
          id: "conversation-1",
          requirement_id: "r-1",
          created_at: state.current().created_at,
          messages: state.messages,
          next_offset: null,
        }),
        headers: jsonHeaders(),
      });
      return;
    }
    if (
      request.method() === "POST" &&
      url.pathname === "/requirements/r-1/conversation/messages"
    ) {
      const body = JSON.parse(request.postData() ?? "{}") as { body?: string };
      const persisted: WorkspaceMessage = {
        id: `message-${state.messages.length + 1}`,
        conversation_id: "conversation-1",
        author_user_id: "user-1",
        kind: "requester",
        body: body.body ?? "",
        created_at: `2026-01-01T00:00:0${state.messages.length}Z`,
      };
      state.messages.push(persisted);
      await route.fulfill({
        status: 201,
        body: JSON.stringify(persisted),
        headers: jsonHeaders(),
      });
      return;
    }
    if (await state.extra(route, url)) return;
    if (
      request.method() === "GET" &&
      url.pathname === "/requirements/r-1/readiness"
    ) {
      await route.fulfill({
        body: JSON.stringify({ assessment: null }),
        headers: jsonHeaders(),
      });
      return;
    }
    if (
      request.method() === "GET" &&
      url.pathname === "/requirements/r-1/activity"
    ) {
      await route.fulfill({
        body: JSON.stringify({ activities: [], next_offset: null }),
        headers: jsonHeaders(),
      });
      return;
    }
    if (
      request.method() === "GET" &&
      url.pathname === "/requirements/r-1/session"
    ) {
      await route.fulfill({
        body: JSON.stringify({ session: state.session?.() ?? null }),
        headers: jsonHeaders(),
      });
      return;
    }
    await route.continue();
  });
}

test("direct workspace loads canonical bundle and uses explicit start, dispatch, and cancel URLs", async ({
  page,
}) => {
  const current = requirement("r-1", "draft", "Conversation requirement");
  const messages: WorkspaceMessage[] = [];
  let currentRun: WorkspaceRunFixture | null = null;
  const requests: string[] = [];

  await installWorkspaceRoutes(page, {
    current: () => current,
    messages,
    requests,
    session: () => currentRun,
    extra: async (route, url) => {
      const request = route.request();
      if (
        request.method() === "POST" &&
        url.pathname === "/requirements/r-1/clarification/start"
      ) {
        const body = JSON.parse(request.postData() ?? "{}") as {
          message_id: string;
        };
        currentRun = {
          run_id: "run-a",
          requirement_id: "r-1",
          start_message_id: body.message_id,
          phase: "active",
          status: "starting",
          cancel_requested: false,
          created_at: "2026-01-01T00:01:00Z",
          updated_at: "2026-01-01T00:01:00Z",
          last_activity_at: "2026-01-01T00:01:00Z",
        };
        await route.fulfill({
          status: 202,
          body: JSON.stringify({ session: currentRun }),
          headers: jsonHeaders(),
        });
        return true;
      }
      if (request.method() === "POST" && url.pathname.endsWith("/dispatch")) {
        if (currentRun) currentRun = { ...currentRun, status: "running" };
        await route.fulfill({
          status: 202,
          body: JSON.stringify({ session: currentRun }),
          headers: jsonHeaders(),
        });
        return true;
      }
      if (request.method() === "POST" && url.pathname.endsWith("/cancel")) {
        if (currentRun)
          currentRun = {
            ...currentRun,
            phase: "terminal",
            status: "completed",
            cancel_requested: true,
          };
        await route.fulfill({
          status: 202,
          body: JSON.stringify({ session: currentRun }),
          headers: jsonHeaders(),
        });
        return true;
      }
      return false;
    },
  });

  await page.goto("/requirements/r-1");
  await expect(
    page
      .getByTestId("conversation-pane")
      .getByRole("heading", { name: "Conversation" }),
  ).toBeVisible();
  await expect(
    page
      .getByTestId("live-requirement-panel")
      .getByRole("heading", { name: "Live Requirement" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Conversation requirement" }),
  ).toBeVisible();

  await page
    .getByRole("textbox", { name: "Message" })
    .fill("Initial clarification");
  await page
    .getByRole("button", { name: "Send clarification message" })
    .click();
  await expect
    .poll(() =>
      requests.some(
        (request) => request === "POST /requirements/r-1/clarification/start",
      ),
    )
    .toBe(true);
  expect(requests.some((request) => request.includes("/dispatch"))).toBe(false);

  await page
    .getByRole("textbox", { name: "Message" })
    .fill("Follow-up clarification");
  await page
    .getByRole("button", { name: "Send clarification message" })
    .click();
  await expect
    .poll(() =>
      requests.some(
        (request) =>
          request ===
          "POST /requirements/r-1/clarification/runs/run-a/messages/message-2/dispatch",
      ),
    )
    .toBe(true);

  await page.getByRole("button", { name: "Cancel clarification" }).click();
  await expect
    .poll(() =>
      requests.some(
        (request) =>
          request === "POST /requirements/r-1/clarification/runs/run-a/cancel",
      ),
    )
    .toBe(true);
  await expect(
    page.getByText("Cancellation completed", { exact: true }),
  ).toBeVisible();
});

test("workspace browser states support reload retry, cancellation pending, terminal starts, and conflicts", async ({
  page,
}) => {
  let current = requirement("r-1", "draft", "Browser state requirement");
  let currentRun: WorkspaceRunFixture = {
    run_id: "run-awaiting",
    requirement_id: "r-1",
    start_message_id: "message-start",
    phase: "awaiting_assignment",
    status: "unavailable",
    cancel_requested: false,
    created_at: "2026-01-01T00:01:00Z",
    updated_at: "2026-01-01T00:01:00Z",
    last_activity_at: "2026-01-01T00:01:00Z",
  };
  const messages: WorkspaceMessage[] = [
    {
      id: "message-start",
      conversation_id: "conversation-1",
      author_user_id: "user-1",
      kind: "requester",
      body: "Initial clarification",
      created_at: "2026-01-01T00:00:00Z",
    },
  ];
  const requests: string[] = [];
  const startBodies: Array<Record<string, unknown>> = [];
  let promoteToCancellationPending = false;
  let patchCalls = 0;
  let cancelCalls = 0;
  let dispatchCalls = 0;

  await installWorkspaceRoutes(page, {
    current: () => current,
    messages,
    requests,
    extra: async (route, url) => {
      const request = route.request();
      if (
        request.method() === "GET" &&
        url.pathname === "/requirements/r-1/session"
      ) {
        if (
          promoteToCancellationPending &&
          currentRun.phase === "active" &&
          !currentRun.cancel_requested
        ) {
          currentRun = { ...currentRun, cancel_requested: true };
          promoteToCancellationPending = false;
        }
        await route.fulfill({
          body: JSON.stringify({ session: currentRun }),
          headers: jsonHeaders(),
        });
        return true;
      }
      if (request.method() === "PATCH" && url.pathname === "/requirements/r-1") {
        patchCalls += 1;
        current = {
          ...current,
          title: "Canonical title after conflict",
          state_version: 2,
          updated_at: "2026-01-01T00:02:00Z",
        };
        await route.fulfill({
          status: 409,
          body: JSON.stringify({ error: "state_version_conflict" }),
          headers: jsonHeaders(),
        });
        return true;
      }
      if (
        request.method() === "POST" &&
        url.pathname === "/requirements/r-1/clarification/start"
      ) {
        const body = JSON.parse(request.postData() ?? "{}") as Record<
          string,
          unknown
        >;
        startBodies.push(body);
        if (startBodies.length === 1) {
          currentRun = {
            ...currentRun,
            run_id: "run-active",
            phase: "active",
            status: "running",
            cancel_requested: false,
            start_message_id: String(body.message_id ?? ""),
          };
          promoteToCancellationPending = true;
          await route.fulfill({
            status: 202,
            body: JSON.stringify({ session: currentRun }),
            headers: jsonHeaders(),
          });
        } else {
          currentRun = {
            run_id: "run-unavailable",
            requirement_id: "r-1",
            start_message_id: String(body.message_id ?? ""),
            phase: "awaiting_assignment",
            status: "unavailable",
            cancel_requested: false,
            created_at: "2026-01-01T00:03:00Z",
            updated_at: "2026-01-01T00:03:00Z",
            last_activity_at: "2026-01-01T00:03:00Z",
          };
          await route.fulfill({
            status: 503,
            body: JSON.stringify({
              error: "clarification_unavailable",
              requirement: current,
              session: currentRun,
            }),
            headers: jsonHeaders(),
          });
        }
        return true;
      }
      if (request.method() === "POST" && url.pathname.endsWith("/dispatch")) {
        dispatchCalls += 1;
        await route.fulfill({
          status: 409,
          body: JSON.stringify({ error: "unexpected_dispatch" }),
          headers: jsonHeaders(),
        });
        return true;
      }
      if (request.method() === "POST" && url.pathname.endsWith("/cancel")) {
        cancelCalls += 1;
        currentRun = {
          ...currentRun,
          phase: "terminal",
          status: "completed",
          cancel_requested: true,
        };
        await route.fulfill({
          status: 202,
          body: JSON.stringify({ session: currentRun }),
          headers: jsonHeaders(),
        });
        return true;
      }
      return false;
    },
  });

  await page.goto("/requirements/r-1");
  await expect(
    page.getByText(
      "Runtime assignment unavailable. Retry same clarification or cancel.",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Message" })).toBeDisabled();
  await expect(
    page.getByRole("button", { name: "Retry clarification start" }),
  ).toBeVisible();

  await page.reload();
  await expect(
    page.getByRole("button", { name: "Retry clarification start" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Retry clarification start" }).click();
  await expect
    .poll(() =>
      startBodies.some(
        (body) =>
          body.message_id === "message-start" &&
          body.expected_state_version === 1,
      ),
    )
    .toBe(true);
  await expect(
    page.getByText(
      "Cancellation pending. Wait for canonical runtime completion.",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Message" })).toBeDisabled();
  await page.getByRole("button", { name: "Repeat cancellation" }).click();
  await expect.poll(() => cancelCalls).toBe(1);
  expect(
    requests.some(
      (request) =>
        request ===
        "POST /requirements/r-1/clarification/runs/run-active/cancel",
    ),
  ).toBe(true);
  await expect(
    page.getByText("Cancellation completed", { exact: true }),
  ).toBeVisible();

  await page.getByRole("button", { name: "Edit requirement" }).click();
  await page.getByLabel("Title", { exact: true }).fill("Draft title");
  await page.getByRole("button", { name: "Save requirement" }).click();
  await expect.poll(() => patchCalls).toBe(1);
  await expect(
    page.getByText("Requirement changed. Draft retained for reconciliation.", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.getByLabel("Title", { exact: true })).toHaveValue(
    "Draft title",
  );
  await expect(
    page
      .getByTestId("live-requirement-panel")
      .getByText("Canonical title after conflict", { exact: true })
      .first(),
  ).toBeVisible();

  await page
    .getByRole("textbox", { name: "Message" })
    .fill("Retry after conflict");
  await page
    .getByRole("button", { name: "Send clarification message" })
    .click();
  await expect
    .poll(() =>
      startBodies.some(
        (body) =>
          body.message_id === "message-2" && body.expected_state_version === 2,
      ),
    )
    .toBe(true);
  await expect(
    page.getByText(
      "Runtime unavailable before assignment. Retry the same clarification or cancel it.",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Retry clarification start" }),
  ).toBeVisible();
  expect(dispatchCalls).toBe(0);
  const lastMessagePost = requests.lastIndexOf(
    "POST /requirements/r-1/conversation/messages",
  );
  const lastStartPost = requests.lastIndexOf(
    "POST /requirements/r-1/clarification/start",
  );
  expect(lastMessagePost).toBeGreaterThanOrEqual(0);
  expect(lastStartPost).toBeGreaterThan(lastMessagePost);
});
