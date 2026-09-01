import { expect, test } from "@playwright/test";

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
    const hint = `event: requirement.changed\ndata: ${JSON.stringify({
      category: "requirement.changed",
      requirement_id: "r-1",
    })}\n\n`;
    await route.fulfill({
      body: hint + hint,
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

test("create uses canonical response and opens read-only detail", async ({
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
    if (url.pathname === "/requirements") {
      await route.fulfill({ body: "[]", headers: jsonHeaders() });
      return;
    }
    await route.continue();
  });
  await page.route("**/events", async (route) => {
    await route.fulfill({ body: ":\n\n", headers: eventHeaders });
  });

  await page.goto("/");
  await page.getByRole("button", { name: "New requirement" }).first().click();
  await page.getByLabel("Title").fill("Created requirement");
  await page.getByLabel("Description").fill("A canonical description.");
  await page.getByRole("button", { name: "Create requirement" }).click();

  await expect(
    page.getByRole("heading", { name: "Created requirement" }),
  ).toBeVisible();
  await expect(page.getByText("Draft", { exact: true })).toBeVisible();
  expect(createBody).toEqual({
    title: "Created requirement",
    description: "A canonical description.",
  });
});
