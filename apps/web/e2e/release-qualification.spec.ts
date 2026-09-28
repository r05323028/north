import { expect, test, type APIRequestContext, type Page } from "@playwright/test";

const otpCode = process.env.NORTH_RELEASE_OTP_CODE;
const email =
  process.env.NORTH_RELEASE_EMAIL ??
  `north-release-${process.pid}@example.test`;

async function authenticate(request: APIRequestContext) {
  if (process.env.NORTH_RELEASE_STORAGE_STATE) return;
  const existing = await request.get("/auth/me");
  if (existing.ok()) return;
  if (!otpCode) {
    throw new Error(
      "NORTH_RELEASE_OTP_CODE is required; read it from the private server log",
    );
  }
  const requested = await request.post("/auth/request-code", {
    data: { email },
  });
  expect(requested.ok()).toBe(true);
  const verified = await request.post("/auth/verify", {
    data: { email, code: otpCode },
  });
  expect(verified.ok()).toBe(true);
}

async function createRequirement(page: Page, title: string) {
  await page.goto("/");
  await page.getByRole("button", { name: "New requirement" }).first().click();
  await page.getByLabel("Title", { exact: true }).fill(title);
  await page
    .getByLabel("Description", { exact: true })
    .fill("Release qualification requirement description.");
  await page.getByRole("button", { name: "Create requirement" }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  return new URL(page.url()).pathname.split("/").pop()!;
}

async function prepareStructuredRequirement(page: Page) {
  await page.getByRole("button", { name: "Edit requirement" }).click();
  await page.getByLabel("Summary", { exact: true }).fill("Bounded release scope.");
  await page
    .getByLabel("Acceptance criteria (one per line)", { exact: true })
    .fill("The release path produces durable canonical state.");
  await page
    .getByLabel("Assumptions (one per line)", { exact: true })
    .fill("The deterministic agent is available.");
  await page.getByRole("button", { name: "Save requirement" }).click();
  await expect(page.getByText("Bounded release scope.", { exact: true })).toBeVisible();
}

test.describe("assembled release qualification", () => {
  test.beforeEach(async ({ page }) => {
    await authenticate(page.request);
  });

  test("creates, clarifies, reaches exact-revision readiness, and reviews canonically", async ({
    page,
  }) => {
    const title = `Release requirement ${Date.now()}`;
    await createRequirement(page, title);
    await prepareStructuredRequirement(page);

    await page
      .getByRole("textbox", { name: "Message" })
      .fill("Clarify release evidence and durable ownership.");
    await page
      .getByRole("button", { name: "Send clarification message" })
      .click();
    await expect(
      page.getByText("Clarify release evidence and durable ownership.", {
        exact: true,
      }),
    ).toBeVisible();
    await page.reload();
    await expect(
      page.getByText("Clarify release evidence and durable ownership.", {
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByText("Qualification clarification complete.", { exact: true }),
    ).toBeVisible({ timeout: 60_000 });
    await expect(page.getByRole("heading", { name: "Readiness" })).toBeVisible({
      timeout: 60_000,
    });
    await expect(page.getByText("ready", { exact: true })).toBeVisible({
      timeout: 60_000,
    });
    await expect(page.getByRole("definition").filter({ hasText: /^Current$/ })).toBeVisible();
    await expect(
      page.getByTestId("human-review-panel"),
    ).toContainText("Review current Requirement evidence");
    await expect(page.getByTestId("human-review-panel").getByText(title, { exact: true })).toBeVisible();
    const revision = await page
      .getByTestId("live-requirement-panel")
      .locator("dt")
      .filter({ hasText: /^Revision$/ })
      .locator("..")
      .locator("dd")
      .textContent();
    expect(revision).toBeTruthy();

    await page.getByRole("button", { name: "Accept Requirement" }).click();
    await expect(page.getByText("Accepted", { exact: true }).first()).toBeVisible({
      timeout: 30_000,
    });
    await expect(
      page.getByTestId("human-review-panel"),
    ).toContainText("Server-authoritative decision");
    await page.reload();
    await expect(page.getByText("Accepted", { exact: true }).first()).toBeVisible();
    await expect(
      page
        .getByTestId("live-requirement-panel")
        .locator("dt")
        .filter({ hasText: /^Revision$/ })
        .locator("..")
        .locator("dd"),
    ).toHaveText(revision!);
    const body = await page.locator("body").innerText();
    for (const secret of [otpCode, process.env.NORTH_RELEASE_DAEMON_CREDENTIAL]) {
      if (secret) expect(body).not.toContain(secret);
    }
  });

  test("repairs live SSE state without browser WebSocket or stale UI", async ({
    page,
  }) => {
    const title = `SSE release requirement ${Date.now()}`;
    const requests: string[] = [];
    let websocketSeen = false;
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (
        request.method() === "GET" &&
        (url.pathname === "/events" || url.pathname.startsWith("/requirements"))
      ) {
        requests.push(`${request.method()} ${url.pathname}`);
      }
    });
    page.on("websocket", () => {
      websocketSeen = true;
    });

    const requirementId = await createRequirement(page, title);
    await prepareStructuredRequirement(page);
    await expect(
      page.getByRole("status", { name: "Live updates: connected" }),
    ).toBeVisible();
    await expect
      .poll(() => requests.filter((request) => request === "GET /events").length)
      .toBeGreaterThan(0);

    await page.getByRole("button", { name: "Edit requirement" }).click();
    await page.getByLabel("Title", { exact: true }).fill(`${title} canonical`);
    await page.getByRole("button", { name: "Save requirement" }).click();
    await expect(
      page.getByRole("heading", { name: `${title} canonical` }),
    ).toBeVisible();
    await expect
      .poll(
        () =>
          requests.filter(
            (request) => request === `GET /requirements/${requirementId}`,
          ).length,
      )
      .toBeGreaterThan(1);

    const beforeFocus = requests.filter(
      (request) => request === `GET /requirements/${requirementId}`,
    ).length;
    await page.evaluate(() => {
      window.dispatchEvent(new Event("focus"));
      Object.defineProperty(document, "visibilityState", {
        configurable: true,
        value: "visible",
      });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect
      .poll(
        () =>
          requests.filter(
            (request) => request === `GET /requirements/${requirementId}`,
          ).length,
      )
      .toBeGreaterThan(beforeFocus);

    const misleading = await page.request.post(
      new URL("/requirements", page.url()).toString(),
      {
        data: {
          title: `Unrelated ${Date.now()}`,
          description: "This event must not refresh current workspace.",
        },
      },
    );
    expect(misleading.ok()).toBe(true);
    const beforeMisleading = requests.filter(
      (request) => request === `GET /requirements/${requirementId}`,
    ).length;
    await page.waitForTimeout(500);
    expect(
      requests.filter(
        (request) => request === `GET /requirements/${requirementId}`,
      ).length,
    ).toBe(beforeMisleading);

    const requirementUrl = new URL(
      `/requirements/${requirementId}`,
      page.url(),
    ).toString();
    const currentResponse = await page.request.get(requirementUrl);
    expect(currentResponse.ok()).toBe(true);
    const current = (await currentResponse.json()) as { state_version: number };
    const beforeReconnectReads = requests.filter(
      (request) => request === `GET /requirements/${requirementId}`,
    ).length;
    const beforeDisconnectEvents = requests.filter(
      (request) => request === "GET /events",
    ).length;

    // Drop the active stream and hold automatic reconnect while canonical state changes.
    const disconnect = await page.request.post(
      new URL("/__release_test__/sse/disconnect", page.url()).toString(),
    );
    expect(disconnect.status()).toBe(204);
    await expect(
      page.getByRole("status", { name: /Live updates: reconnecting/ }),
    ).toBeVisible({ timeout: 5_000 });

    const nextTitle = `${title} after reconnect`;
    const update = await page.request.patch(requirementUrl, {
      data: { expected_state_version: current.state_version, title: nextTitle },
    });
    expect(update.ok()).toBe(true);
    await expect(
      page.getByRole("heading", { name: `${title} canonical` }),
    ).toBeVisible();
    await expect(page.getByRole("heading", { name: nextTitle })).toHaveCount(0);
    await page.waitForTimeout(500);
    expect(
      requests.filter(
        (request) => request === `GET /requirements/${requirementId}`,
      ).length,
    ).toBe(beforeReconnectReads);

    const reconnect = await page.request.post(
      new URL("/__release_test__/sse/reconnect", page.url()).toString(),
    );
    expect(reconnect.status()).toBe(204);
    await expect(
      page.getByRole("status", { name: "Live updates: connected" }),
    ).toBeVisible({ timeout: 20_000 });
    await expect
      .poll(() => requests.filter((request) => request === "GET /events").length)
      .toBeGreaterThan(beforeDisconnectEvents);
    await expect
      .poll(
        () =>
          requests.filter(
            (request) => request === `GET /requirements/${requirementId}`,
          ).length,
      )
      .toBeGreaterThan(beforeReconnectReads);
    await expect(page.getByRole("heading", { name: nextTitle })).toBeVisible();
    await expect(
      page.getByRole("heading", { name: `${title} canonical` }),
    ).toHaveCount(0);
    expect(websocketSeen).toBe(false);
  });
});
