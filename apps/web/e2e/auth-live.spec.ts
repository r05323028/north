import { readFile } from "node:fs/promises";

import { expect, test, type Page } from "@playwright/test";

const backendUrl = process.env.NORTH_AUTH_TEST_BACKEND_URL;
const deliveryLog = process.env.NORTH_AUTH_TEST_LOG;

test.use({ trace: "off" });

async function waitForCode(email: string): Promise<string> {
  if (!deliveryLog) throw new Error("NORTH_AUTH_TEST_LOG is required");
  const prefix = `north verification code email=${email} code=`;

  for (let attempt = 0; attempt < 100; attempt += 1) {
    let log = "";
    try {
      log = await readFile(deliveryLog, "utf8");
    } catch (error) {
      if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) {
        throw error;
      }
    }
    let code = "";
    for (const line of log.split(/\r?\n/)) {
      if (line.startsWith(prefix)) code = line.slice(prefix.length);
    }
    if (/^\d{6}$/.test(code)) return code;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  throw new Error("Timed out waiting for configured OTP delivery sink");
}

async function proxyAuthToBackend(page: Page) {
  if (!backendUrl) throw new Error("NORTH_AUTH_TEST_BACKEND_URL is required");

  let sessionCookieFlags = {
    cookieName: false,
    httpOnly: false,
    secure: false,
    sameSiteLax: false,
  };
  let currentUserReceivedSessionCookie = false;

  await page.route("**/auth/**", async (route) => {
    const request = route.request();
    const incoming = new URL(request.url());
    const target = new URL(`${incoming.pathname}${incoming.search}`, backendUrl);
    const response = await route.fetch({ url: target.href });

    if (incoming.pathname === "/auth/verify" && response.status() === 204) {
      const directives = (response.headers()["set-cookie"] ?? "")
        .split(";")
        .map((part) => part.trim().toLowerCase());
      sessionCookieFlags = {
        cookieName: directives[0]?.startsWith("north_session=") ?? false,
        httpOnly: directives.includes("httponly"),
        secure: directives.includes("secure"),
        sameSiteLax: directives.includes("samesite=lax"),
      };
    }

    if (incoming.pathname === "/auth/me") {
      const cookie = request.headers().cookie ?? "";
      currentUserReceivedSessionCookie = cookie
        .split(";")
        .some((part) => part.trim().startsWith("north_session="));
    }

    await route.fulfill({ response });
  });

  return {
    sessionCookieFlags: () => sessionCookieFlags,
    currentUserReceivedSessionCookie: () => currentUserReceivedSessionCookie,
  };
}

test("live signup and login establish server sessions", async ({ page }) => {
  test.skip(
    !backendUrl || !deliveryLog,
    "Set NORTH_AUTH_TEST_BACKEND_URL and NORTH_AUTH_TEST_LOG; use a fresh disposable database",
  );
  if (!backendUrl || !deliveryLog) return;

  test.setTimeout(60_000);
  const auth = await proxyAuthToBackend(page);
  const run = `${process.pid}-${Date.now()}`;
  const flows = [
    {
      path: "/signup",
      email: `north-live-signup-${run}@example.test`,
      submit: "完成註冊",
      role: "Owner",
    },
    {
      path: "/login",
      email: `north-live-login-${run}@example.test`,
      submit: "登入",
      role: "Requester",
    },
  ];

  for (const flow of flows) {
    await page.goto(flow.path);
    await page.getByLabel("電子郵件地址").fill(flow.email);
    await page.getByRole("button", { name: "取得驗證碼" }).click();
    await expect(page.getByLabel("6 位數驗證碼")).toBeVisible();
    await page.getByLabel("6 位數驗證碼").fill(await waitForCode(flow.email));
    await page.getByRole("button", { name: flow.submit }).click();

    await expect(page).toHaveURL(/\/$/);
    await expect(
      page.getByRole("status", { name: `Signed in as ${flow.email}` }),
    ).toBeVisible();
    await expect(page.getByText(flow.role, { exact: true })).toBeVisible();
  }

  expect(auth.sessionCookieFlags()).toEqual({
    cookieName: true,
    httpOnly: true,
    secure: true,
    sameSiteLax: true,
  });
  expect(auth.currentUserReceivedSessionCookie()).toBe(true);
});
