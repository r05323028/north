import { expect, test, type Page } from "@playwright/test";

const email = "north-auth@example.test";

async function stubCurrentUser(page: Page, address: string) {
  let verified = false;
  let sessionCookie = "";
  await page.route("**/auth/me", async (route) => {
    const cookie = route.request().headers().cookie ?? "";
    if (verified && cookie.includes("north_session=browser-test")) {
      sessionCookie = cookie;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ id: "user-1", email: address, role: "Owner" }),
      });
      return;
    }
    await route.fulfill({
      status: 401,
      contentType: "application/json",
      body: JSON.stringify({ message: "unauthorized" }),
    });
  });
  return {
    allowSession: () => {
      verified = true;
    },
    sessionCookie: () => sessionCookie,
  };
}

test("renders linked auth pages without workspace shell and adapts to mobile/theme", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  let currentUserRequests = 0;
  await page.route("**/auth/me", async (route) => {
    currentUserRequests += 1;
    await route.fulfill({ status: 401, json: { message: "unauthorized" } });
  });

  await page.goto("/login");
  await expect(page.getByRole("heading", { name: "登入 North" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "主導覽" })).toHaveCount(0);
  const initialTheme = await page.locator("html").getAttribute("data-theme");
  await page.getByRole("button", { name: "切換主題" }).click();
  await expect
    .poll(() => page.locator("html").getAttribute("data-theme"))
    .not.toBe(initialTheme);
  const panel = await page.getByTestId("auth-panel").boundingBox();
  expect(panel).not.toBeNull();
  expect(panel!.x).toBeGreaterThanOrEqual(0);
  expect(panel!.x + panel!.width).toBeLessThanOrEqual(390);

  await page.getByRole("link", { name: "建立帳戶" }).click();
  await expect(page).toHaveURL(/\/signup$/);
  await expect(page.getByRole("heading", { name: "建立 North 帳戶" })).toBeVisible();
  await expect(page.getByText(/第一位建立帳戶.*Owner/)).toBeVisible();
  await page.getByRole("link", { name: "登入" }).click();
  await expect(page).toHaveURL(/\/login$/);
  expect(currentUserRequests).toBe(0);
});

test("login requests, recovers from invalid code, resends, and forwards session cookie", async ({
  page,
}) => {
  const currentUser = await stubCurrentUser(page, email);
  const requestBodies: unknown[] = [];
  const verifyBodies: unknown[] = [];
  let verifyAttempts = 0;

  await page.route("**/auth/request-code", async (route) => {
    requestBodies.push(route.request().postDataJSON());
    await route.fulfill({
      status: 202,
      json: { message: "verification code requested" },
    });
  });
  await page.route("**/auth/verify", async (route) => {
    verifyBodies.push(route.request().postDataJSON());
    verifyAttempts += 1;
    if (verifyAttempts === 1) {
      await route.fulfill({
        status: 401,
        json: { message: "invalid code; private server details" },
      });
      return;
    }
    currentUser.allowSession();
    await route.fulfill({
      status: 204,
      headers: {
        "set-cookie": "north_session=browser-test; Path=/; HttpOnly; SameSite=Lax",
      },
    });
  });

  await page.goto("/login");
  await page.getByLabel("電子郵件地址").fill(email);
  await page.getByRole("button", { name: "取得驗證碼" }).click();
  await expect(page.getByLabel("6 位數驗證碼")).toBeVisible();
  await page.getByLabel("6 位數驗證碼").fill("123456");
  await page.getByRole("button", { name: "登入" }).click();
  await expect(page.locator("#code-error")).toHaveText("驗證碼錯誤或已過期，請重新確認。");
  await expect(page.getByText("private server details")).toHaveCount(0);

  await page.getByRole("button", { name: "重新取得驗證碼" }).click();
  await expect(page.getByRole("status")).toHaveText(
    "若此信箱可使用，請使用最新取得的驗證碼。",
  );
  await page.getByLabel("6 位數驗證碼").fill("654321");
  await page.getByRole("button", { name: "登入" }).click();

  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole("status", { name: `Signed in as ${email}` })).toBeVisible();
  expect(requestBodies).toEqual([{ email }, { email }]);
  expect(verifyBodies).toEqual([
    { email, code: "123456" },
    { email, code: "654321" },
  ]);
  expect(currentUser.sessionCookie()).toContain("north_session=browser-test");
});

test("signup uses same verification API and relies on server-assigned role", async ({
  page,
}) => {
  const firstOwner = "first-owner@example.test";
  const currentUser = await stubCurrentUser(page, firstOwner);
  const requestBodies: unknown[] = [];
  const verifyBodies: unknown[] = [];

  await page.route("**/auth/request-code", async (route) => {
    requestBodies.push(route.request().postDataJSON());
    await route.fulfill({
      status: 202,
      json: { message: "verification code requested" },
    });
  });
  await page.route("**/auth/verify", async (route) => {
    verifyBodies.push(route.request().postDataJSON());
    currentUser.allowSession();
    await route.fulfill({
      status: 204,
      headers: {
        "set-cookie": "north_session=browser-test; Path=/; HttpOnly; SameSite=Lax",
      },
    });
  });

  await page.goto("/signup");
  await page.getByLabel("電子郵件地址").fill(firstOwner);
  await page.getByRole("button", { name: "取得驗證碼" }).click();
  await page.getByLabel("6 位數驗證碼").fill("012345");
  await page.getByRole("button", { name: "完成註冊" }).click();

  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole("status", { name: `Signed in as ${firstOwner}` })).toBeVisible();
  expect(requestBodies).toEqual([{ email: firstOwner }]);
  expect(verifyBodies).toEqual([{ email: firstOwner, code: "012345" }]);
  await expect(page.locator("select")).toHaveCount(0);
});

test("redirects 401 guests but does not misclassify other auth failures", async ({
  page,
}) => {
  await page.route("**/auth/me", async (route) => {
    await route.fulfill({
      status: 401,
      contentType: "application/json",
      body: JSON.stringify({ message: "unauthorized" }),
    });
  });
  await page.goto("/");
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole("heading", { name: "登入 North" })).toBeVisible();
});

test("does not redirect when current-user request fails without 401", async ({
  page,
}) => {
  await page.route("**/auth/me", async (route) => {
    await route.fulfill({
      status: 503,
      contentType: "application/json",
      body: JSON.stringify({ message: "temporarily unavailable" }),
    });
  });
  await page.goto("/");
  await expect(page).not.toHaveURL(/\/login$/);
  await expect(page.getByRole("navigation", { name: "主導覽" })).toBeVisible();
});

test("shows generic cooldown feedback for rate-limited code requests", async ({
  page,
}) => {
  await page.route("**/auth/request-code", async (route) => {
    await route.fulfill({
      status: 429,
      json: { error: "rate_limited", message: "private limiter details" },
    });
  });

  await page.goto("/login");
  await page.getByLabel("電子郵件地址").fill(email);
  await page.getByRole("button", { name: "取得驗證碼" }).click();
  await expect(page.getByRole("status")).toHaveText("操作太頻繁，請稍後再試。");
  await expect(page.getByText("private limiter details")).toHaveCount(0);
  await expect(page.getByLabel("6 位數驗證碼")).toBeHidden();
});
