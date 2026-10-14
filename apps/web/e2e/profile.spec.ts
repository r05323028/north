import { expect, test } from "@playwright/test";

const email = "profile@example.test";

test("profile shows read-only identity and logout invalidates session before redirect", async ({ page }) => {
  let authenticated = true;
  let logoutCalls = 0;
  let deniedAuthChecks = 0;
  let signalLogoutStarted!: () => void;
  const logoutStarted = new Promise<void>((resolve) => { signalLogoutStarted = resolve; });
  let releaseLogout!: () => void;
  const logoutGate = new Promise<void>((resolve) => { releaseLogout = resolve; });
  await page.route("**/auth/me", async (route) => {
    if (authenticated) await route.fulfill({ status: 200, json: { id: "profile-user", email, role: "Requester" } });
    else { deniedAuthChecks += 1; await route.fulfill({ status: 401, json: { message: "unauthorized" } }); }
  });
  await page.route("**/auth/logout", async (route) => {
    logoutCalls += 1;
    expect(route.request().method()).toBe("POST");
    signalLogoutStarted();
    await logoutGate;
    authenticated = false;
    await route.fulfill({ status: 204 });
  });

  await page.goto("/");
  await expect(page.getByRole("link", { name: "個人資料" })).toBeVisible();
  await page.goto("/profile");
  await expect(page.getByTestId("profile-email")).toHaveText(email);
  await expect(page.getByTestId("profile-role")).toHaveText("Requester");
  await expect(page.getByRole("textbox")).toHaveCount(0);
  await expect(page.getByRole("link", { name: "個人資料" })).toBeVisible();
  await page.getByRole("button", { name: "登出" }).click();
  await logoutStarted;
  await expect(page).toHaveURL(/\/profile$/);
  await expect(page.getByTestId("profile-email")).toHaveText(email);
  releaseLogout();
  await expect(page).toHaveURL(/\/login$/);
  expect(logoutCalls).toBe(1);
  await page.goBack();
  await expect.poll(() => deniedAuthChecks).toBeGreaterThan(0);
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByTestId("profile-email")).toHaveCount(0);
});

test("logout failure preserves profile and session", async ({ page }) => {
  await page.route("**/auth/me", (route) => route.fulfill({ status: 200, json: { id: "profile-user", email, role: "Requester" } }));
  await page.route("**/auth/logout", (route) => route.fulfill({ status: 503, json: { message: "unavailable" } }));
  await page.goto("/profile");
  await page.getByRole("button", { name: "登出" }).click();
  await expect(page.getByText("登出失敗。登入狀態未變更，請重試。")).toBeVisible();
  await expect(page.getByTestId("profile-email")).toHaveText(email);
  await expect(page).toHaveURL(/\/profile$/);
});
