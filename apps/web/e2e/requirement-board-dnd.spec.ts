import { expect, test, type Page } from "@playwright/test";

async function waitForDropSettled(page: Page) {
  const indicator = page.locator("[data-drop-settling]");
  await expect(indicator).toHaveCount(1);
  await expect(indicator).toHaveCount(0);
}

type Item = { id: string; title: string; status: string; revision: number; state_version: number; created_by: string; created_at: string; updated_at: string; description: string; summary: string; acceptance_criteria: string[]; assumptions: string[]; open_questions: string[] };
const item = (id: string, status = "draft"): Item => ({ id, title: id, status, revision: 1, state_version: 1, created_by: "user-1", created_at: "2026-01-01T00:00:00Z", updated_at: "2026-01-01T00:00:00Z", description: "Description", summary: "Summary", acceptance_criteria: [], assumptions: [], open_questions: [] });
const headers = { "Content-Type": "application/json" };

async function mockBoardShell(page: Page, role = "Requester") {
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Cache-Control": "no-cache", "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));
}

test("keyboard reorder persists canonical server order after reload", async ({ page }) => {
  let current = [item("r-1"), item("r-2")];
  const reorderBodies: unknown[] = [];
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc) return route.continue();
    if (url.pathname !== "/requirements" && !url.pathname.endsWith("/reorder")) return route.continue();
    if (request.method() === "POST") {
      const body = request.postDataJSON() as { expected_state_version: number; after_id: string | null };
      reorderBodies.push(body);
      current = [current[1], current[0]];
      return route.fulfill({ status: 204, body: "" });
    }
    return route.fulfill({ body: JSON.stringify(current), headers });
  });
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Cache-Control": "no-cache", "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role: "Requester" }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));

  await page.goto("/");
  const first = page.locator('[data-requirement-id="r-1"]');
  await expect(first).toBeVisible();
  await page.getByRole("button", { name: "Move r-1" }).focus();
  await page.keyboard.press("Space");
  for (let index = 0; index < 4; index += 1) {
    await page.keyboard.press("ArrowDown");
    await page.waitForTimeout(50);
  }
  await page.keyboard.press("Space");
  await expect.poll(() => reorderBodies.length).toBe(1);
  expect(reorderBodies[0]).toEqual({ expected_state_version: 1, before_id: "r-2", after_id: null });
  await expect.poll(() => page.locator('[data-requirement-id]:not([data-dnd-placeholder])').evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-requirement-id")))).toEqual(["r-2", "r-1"]);
  await page.reload();
  await expect.poll(() => page.locator('[data-requirement-id]:not([data-dnd-placeholder])').evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-requirement-id")))).toEqual(["r-2", "r-1"]);
});

test("stale reorder reports failure and leaves canonical order unchanged", async ({ page }) => {
  let current = [item("r-1"), item("r-2")];
  let postCount = 0;
  let collectionReads = 0;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc || (url.pathname !== "/requirements" && !url.pathname.endsWith("/reorder"))) return route.continue();
    if (request.method() === "POST") { postCount += 1; current = [item("r-2"), item("r-1")]; return route.fulfill({ status: 409, body: JSON.stringify({ error: "conflict" }), headers }); }
    collectionReads += 1;
    return route.fulfill({ body: JSON.stringify(current), headers });
  });
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Cache-Control": "no-cache", "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role: "Requester" }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));
  await page.goto("/");
  const first = page.locator('[data-requirement-id="r-1"]');
  await expect(first).toBeVisible();
  await page.getByRole("button", { name: "Move r-1" }).focus();
  await page.keyboard.press("Space");
  for (let index = 0; index < 4; index += 1) {
    await page.keyboard.press("ArrowDown");
    await page.waitForTimeout(50);
  }
  await page.keyboard.press("Space");
  await waitForDropSettled(page);
  await expect.poll(() => postCount).toBe(1);
  await expect(page.getByRole("alert").filter({ hasText: "conflict" })).toBeVisible();
  expect(current.map(({ id }) => id)).toEqual(["r-2", "r-1"]);
  await expect.poll(() => page.locator('[data-requirement-id]:not([data-dnd-placeholder])').evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-requirement-id")))).toEqual(["r-2", "r-1"]);
  expect(collectionReads).toBeGreaterThan(1);
  await page.reload();
  await expect.poll(() => page.locator('[data-requirement-id]:not([data-dnd-placeholder])').evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-requirement-id")))).toEqual(["r-2", "r-1"]);
});

test("pointer drag reorders requirements within a column", async ({ page }) => {
  let current = [item("r-1"), item("r-2")];
  let reorderBody: unknown;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc || (url.pathname !== "/requirements" && !url.pathname.endsWith("/reorder"))) return route.continue();
    if (request.method() === "POST") {
      reorderBody = request.postDataJSON();
      current = [current[1], current[0]];
      return route.fulfill({ status: 204, body: "" });
    }
    return route.fulfill({ body: JSON.stringify(current), headers });
  });
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Cache-Control": "no-cache", "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role: "Requester" }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));
  await page.goto("/");
  const handle = page.getByRole("button", { name: "Move r-1" });
  const target = page.locator('[data-requirement-id="r-2"]');
  await expect(handle).toBeVisible();
  const from = await handle.boundingBox();
  const to = await target.boundingBox();
  if (!from || !to) throw new Error("Drag targets not visible");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height - 4, { steps: 12 });
  await page.mouse.up();
  await waitForDropSettled(page);
  await expect.poll(() => reorderBody).toEqual({ expected_state_version: 1, before_id: "r-2", after_id: null });
  await expect.poll(() => page.locator('[data-requirement-id]:not([data-dnd-placeholder])').evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-requirement-id")))).toEqual(["r-2", "r-1"]);
});

test("touch drag reorders requirements within a column", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "CDP touch input requires Chromium");
  let current = [item("r-1"), item("r-2")];
  let reorderBody: unknown;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc || (url.pathname !== "/requirements" && !url.pathname.endsWith("/reorder"))) return route.continue();
    if (request.method() === "POST") {
      reorderBody = request.postDataJSON();
      current = [current[1], current[0]];
      return route.fulfill({ status: 204, body: "" });
    }
    return route.fulfill({ body: JSON.stringify(current), headers });
  });
  await mockBoardShell(page);
  await page.goto("/");
  const handle = page.getByRole("button", { name: "Move r-1" });
  const target = page.locator('[data-requirement-id="r-2"]');
  await expect(handle).toBeVisible();
  const from = await handle.boundingBox();
  const to = await target.boundingBox();
  if (!from || !to) throw new Error("Touch drag targets not visible");

  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 1 });
  const start = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
  const end = { x: to.x + to.width / 2, y: to.y + to.height - 4 };
  const dispatch = (type: "touchStart" | "touchMove" | "touchEnd", touchPoints: { id: number; x: number; y: number }[]) =>
    cdp.send("Input.dispatchTouchEvent", { type, touchPoints });
  const pointAt = (progress: number) => ({ id: 0, x: start.x + (end.x - start.x) * progress, y: start.y + (end.y - start.y) * progress });
  await dispatch("touchStart", [pointAt(0)]);
  await page.waitForTimeout(300);
  for (let step = 1; step <= 12; step += 1) await dispatch("touchMove", [pointAt(step / 12)]);
  await dispatch("touchEnd", []);
  await waitForDropSettled(page);
  await cdp.detach();

  await expect.poll(() => reorderBody).toEqual({ expected_state_version: 1, before_id: "r-2", after_id: null });
  await expect.poll(() => page.locator('[data-requirement-id]:not([data-dnd-placeholder])').evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-requirement-id")))).toEqual(["r-2", "r-1"]);
});

test("pointer drag to unsupported lifecycle column is rejected without transition", async ({ page }) => {
  const current = [item("r-1", "discussing"), item("r-ready", "ready")];
  let postCount = 0;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc || (url.pathname !== "/requirements" && !url.pathname.endsWith("/reorder"))) return route.continue();
    if (request.method() === "POST") { postCount += 1; return route.fulfill({ status: 204, body: "" }); }
    return route.fulfill({ body: JSON.stringify(current), headers });
  });
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Cache-Control": "no-cache", "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role: "Requester" }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));
  await page.goto("/");
  const handle = page.getByRole("button", { name: "Move r-1" });
  const destination = page.locator('[data-requirement-id="r-ready"]');
  await expect(handle).toBeVisible();
  const from = await handle.boundingBox();
  const to = await destination.boundingBox();
  if (!from || !to) throw new Error("Drag targets not visible");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 15 });
  await page.mouse.up();
  await waitForDropSettled(page);
  await expect(page.getByRole("alert").filter({ hasText: "This lifecycle move is not available from the board." })).toBeVisible();
  expect(postCount).toBe(0);
  await page.reload();
  await expect(page.locator('[data-status="discussing"] [data-requirement-id="r-1"]')).toBeVisible();
  await expect(page.locator('[data-status="ready"] [data-requirement-id="r-1"]')).toHaveCount(0);
});

test("pointer Draft to empty Discussing column begins discussion and refreshes status", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  let current = [item("r-draft")];
  const calls: { path: string; body: unknown }[] = [];
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc) return route.continue();
    if (url.pathname === "/requirements/r-draft/begin-discussion" && request.method() === "POST") {
      calls.push({ path: url.pathname, body: request.postDataJSON() });
      current = [item("r-draft", "discussing")];
      return route.fulfill({ body: JSON.stringify(current[0]), headers });
    }
    if (url.pathname === "/requirements") return route.fulfill({ body: JSON.stringify(current), headers });
    return route.continue();
  });
  await mockBoardShell(page);
  await page.goto("/");
  const handle = page.getByRole("button", { name: "Move r-draft" });
  const destination = page.locator('[data-status="discussing"] .north-column-body');
  const from = await handle.boundingBox();
  const to = await destination.boundingBox();
  if (!from || !to) throw new Error("Drag targets not visible");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 15 });
  await page.mouse.up();
  await waitForDropSettled(page);
  await expect.poll(() => calls).toEqual([{ path: "/requirements/r-draft/begin-discussion", body: { expected_state_version: 1 } }]);
  await expect(page.locator('[data-status="discussing"] [data-requirement-id="r-draft"]:not([data-dnd-placeholder])')).toBeVisible();
});

test("drop to empty Ready column is rejected without transition", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const current = [item("r-draft")];
  let postCount = 0;
  let collectionReads = 0;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc) return route.continue();
    if (request.method() === "POST") {
      postCount += 1;
      return route.fulfill({ status: 204, body: "" });
    }
    if (url.pathname === "/requirements") {
      collectionReads += 1;
      return route.fulfill({ body: JSON.stringify(current), headers });
    }
    return route.continue();
  });
  await mockBoardShell(page);
  await page.goto("/");
  const handle = page.getByRole("button", { name: "Move r-draft" });
  const destination = page.locator('[data-status="ready"] .north-column-body');
  const from = await handle.boundingBox();
  const to = await destination.boundingBox();
  if (!from || !to) throw new Error("Drag targets not visible");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 15 });
  await page.mouse.up();
  await waitForDropSettled(page);
  await expect(page.getByRole("alert").filter({ hasText: "This lifecycle move is not available from the board." })).toBeVisible();
  expect(postCount).toBe(0);
  expect(collectionReads).toBeGreaterThan(1);
  await expect(page.locator('[data-status="draft"] [data-requirement-id="r-draft"]')).toBeVisible();
});

test("pointer Draft to Discussing begins discussion and refreshes status", async ({ page }) => {
  let current = [item("r-draft"), item("r-discussing", "discussing")];
  const calls: { path: string; body: unknown }[] = [];
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc) return route.continue();
    if (url.pathname === "/requirements/r-draft/begin-discussion") {
      calls.push({ path: url.pathname, body: request.postDataJSON() });
      current = [item("r-draft", "discussing"), current[1]];
      return route.fulfill({ body: JSON.stringify(current[0]), headers });
    }
    if (url.pathname === "/requirements") return route.fulfill({ body: JSON.stringify(current), headers });
    return route.continue();
  });
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role: "Requester" }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));
  await page.goto("/");
  const handle = page.getByRole("button", { name: "Move r-draft" });
  const target = page.locator('[data-requirement-id="r-discussing"]');
  const from = await handle.boundingBox();
  const to = await target.boundingBox();
  if (!from || !to) throw new Error("Drag targets not visible");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 15 });
  await page.mouse.up();
  await waitForDropSettled(page);
  await expect.poll(() => calls).toEqual([{ path: "/requirements/r-draft/begin-discussion", body: { expected_state_version: 1 } }]);
  await expect(page.locator('[data-status="discussing"] [data-requirement-id="r-draft"]:not([data-dnd-placeholder])')).toBeVisible();
});

test("reviewer pointer Rejected to Discussing reopens and refreshes status", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  let current = [item("r-rejected", "rejected"), item("r-discussing", "discussing")];
  const calls: { path: string; body: unknown }[] = [];
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc) return route.continue();
    if (url.pathname === "/requirements/r-rejected/reopen") {
      calls.push({ path: url.pathname, body: request.postDataJSON() });
      current = [item("r-rejected", "discussing"), current[1]];
      return route.fulfill({ body: JSON.stringify(current[0]), headers });
    }
    if (url.pathname === "/requirements") return route.fulfill({ body: JSON.stringify(current), headers });
    return route.continue();
  });
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role: "RequirementManager" }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));
  await page.goto("/");
  const handle = page.getByRole("button", { name: "Move r-rejected" });
  const target = page.locator('[data-requirement-id="r-discussing"]');
  const from = await handle.boundingBox();
  const to = await target.boundingBox();
  if (!from || !to) throw new Error("Drag targets not visible");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 15 });
  await page.mouse.up();
  await waitForDropSettled(page);
  await expect.poll(() => calls).toEqual([{ path: "/requirements/r-rejected/reopen", body: { expected_state_version: 1 } }]);
  await expect(page.locator('[data-status="discussing"] [data-requirement-id="r-rejected"]:not([data-dnd-placeholder])')).toBeVisible();
});

test("Accepted and Ready lifecycle drops plus Discussing to Ready send no mutation", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const current = [item("r-accepted", "accepted"), item("r-ready", "ready"), item("r-discussing", "discussing"), item("r-draft")];
  const posts: string[] = [];
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc) return route.continue();
    if (request.method() === "POST") {
      posts.push(url.pathname);
      return route.fulfill({ status: 204, body: "" });
    }
    if (url.pathname === "/requirements") return route.fulfill({ body: JSON.stringify(current), headers });
    return route.continue();
  });
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role: "Requester" }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));
  await page.goto("/");
  const drop = async (handleName: string, targetId: string) => {
    const from = await page.getByRole("button", { name: handleName }).boundingBox();
    const to = await page.locator(`[data-requirement-id="${targetId}"]`).boundingBox();
    if (!from || !to) throw new Error("Drag targets not visible");
    await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
    await page.mouse.down();
    await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 15 });
    await page.mouse.up();
    await waitForDropSettled(page);
  };
  await drop("Move r-accepted", "r-draft");
  await expect(page.getByRole("alert").filter({ hasText: "Accepted requirements cannot be moved." })).toBeVisible();
  await drop("Move r-ready", "r-discussing");
  await expect(page.getByRole("alert").filter({ hasText: "This lifecycle move is not available from the board." })).toBeVisible();
  await drop("Move r-discussing", "r-ready");
  await expect(page.getByRole("alert").filter({ hasText: "This lifecycle move is not available from the board." })).toBeVisible();
  expect(posts).toEqual([]);
});

test("server failure reports error and restores canonical board order", async ({ page }) => {
  const current = [item("r-1"), item("r-2")];
  let postCount = 0;
  let collectionReads = 0;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc || (url.pathname !== "/requirements" && !url.pathname.endsWith("/reorder"))) return route.continue();
    if (request.method() === "POST") {
      postCount += 1;
      return route.fulfill({ status: 503, body: JSON.stringify({ error: "server unavailable" }), headers });
    }
    collectionReads += 1;
    return route.fulfill({ body: JSON.stringify(current), headers });
  });
  await mockBoardShell(page);
  await page.goto("/");
  await expect(page.locator('[data-requirement-id="r-1"]')).toBeVisible();
  await page.getByRole("button", { name: "Move r-1" }).focus();
  await page.keyboard.press("Space");
  for (let index = 0; index < 4; index += 1) {
    await page.keyboard.press("ArrowDown");
    await page.waitForTimeout(50);
  }
  await page.keyboard.press("Space");
  await waitForDropSettled(page);
  await expect.poll(() => postCount).toBe(1);
  await expect(page.getByRole("alert").filter({ hasText: "server unavailable" })).toBeVisible();
  await expect.poll(() => collectionReads).toBeGreaterThan(1);
  await expect.poll(() => page.locator('[data-requirement-id]:not([data-dnd-placeholder])').evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-requirement-id")))).toEqual(["r-1", "r-2"]);
  await page.reload();
  await expect.poll(() => page.locator('[data-requirement-id]:not([data-dnd-placeholder])').evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-requirement-id")))).toEqual(["r-1", "r-2"]);
});

test("requester forbidden reopen reports error and restores rejected column", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const current = [item("r-rejected", "rejected"), item("r-discussing", "discussing")];
  let reopenCount = 0;
  let collectionReads = 0;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc) return route.continue();
    if (url.pathname === "/requirements/r-rejected/reopen" && request.method() === "POST") {
      reopenCount += 1;
      return route.fulfill({ status: 403, body: JSON.stringify({ error: "forbidden" }), headers });
    }
    if (url.pathname === "/requirements" && request.method() === "GET") {
      collectionReads += 1;
      return route.fulfill({ body: JSON.stringify(current), headers });
    }
    return route.continue();
  });
  await mockBoardShell(page, "Requester");
  await page.goto("/");
  const handle = page.getByRole("button", { name: "Move r-rejected" });
  const target = page.locator('[data-requirement-id="r-discussing"]');
  const from = await handle.boundingBox();
  const to = await target.boundingBox();
  if (!from || !to) throw new Error("Drag targets not visible");
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 15 });
  await page.mouse.up();
  await waitForDropSettled(page);
  await expect.poll(() => reopenCount).toBe(1);
  await expect(page.getByRole("alert").filter({ hasText: "forbidden" })).toBeVisible();
  await expect.poll(() => collectionReads).toBeGreaterThan(1);
  await expect(page.locator('[data-status="rejected"] [data-requirement-id="r-rejected"]:not([data-dnd-placeholder])')).toBeVisible();
  await expect(page.locator('[data-status="discussing"] [data-requirement-id="r-rejected"]:not([data-dnd-placeholder])')).toHaveCount(0);
});

test("initial requirements request uses board sort", async ({ page }) => {
  let query: string | null = null;
  await page.route("**/requirements**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.resourceType() !== "fetch" || request.headers().rsc) return route.continue();
    if (url.pathname === "/requirements") {
      query ??= url.search;
      return route.fulfill({ body: JSON.stringify([item("r-1")]), headers });
    }
    return route.continue();
  });
  await page.route("**/events", (route) => route.fulfill({ body: ":\n\n", headers: { ...headers, "Content-Type": "text/event-stream" } }));
  await page.route("**/auth/me", (route) => route.fulfill({ body: JSON.stringify({ id: "user-1", email: "user@example.com", role: "Requester" }), headers }));
  await page.route("**/auth/me/preferences/requirement-workspace", (route) => route.fulfill({ body: JSON.stringify({ default_view: "board", list_status: null, list_created_by: null, list_sort: "updated" }), headers }));
  await page.goto("/");
  await expect.poll(() => query).toBe("?sort=board");
});
