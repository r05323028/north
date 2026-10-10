import { expect, type Page } from "@playwright/test";

import type { RequirementWorkspacePreferences } from "../lib/api/requirement-workspace-preferences";
import type { PreferenceEditorAdapter } from "./preference-editor-flow";

const preferencesPath = "/auth/me/preferences/requirement-workspace";
const saveButton = (page: Page) =>
  page.getByRole("button", { name: "Save as default" });

export const requirementWorkspacePreferenceAdapter: PreferenceEditorAdapter<RequirementWorkspacePreferences> = {
  async open(page) {
    await page.goto("/");
    await expect(saveButton(page)).toBeVisible();
  },
  async edit(page, preferences) {
    await page.getByRole("button", { name: "List", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Requirement list" }),
    ).toBeVisible();
    await page.getByLabel("Search").fill("transient preference search");
    await page.getByLabel("Status").selectOption(preferences.list_status ?? "");
    await page.getByLabel("Creator").fill(preferences.list_created_by ?? "");
    await page.getByLabel("Updated").selectOption(preferences.list_sort);
    if (preferences.default_view === "board") {
      await page.getByRole("button", { name: "Board", exact: true }).click();
    }
  },
  async save(page) {
    const responsePromise = page.waitForResponse((response) => {
      const request = response.request();
      return (
        request.method() === "PUT" &&
        response.url().includes(preferencesPath)
      );
    });
    await saveButton(page).click();
    const response = await responsePromise;
    expect(response.ok()).toBe(true);
    await expect(page.getByText("已儲存個人預設。", { exact: true })).toBeVisible();
  },
  async assertRestored(page, preferences) {
    if (preferences.default_view === "list") {
      await expect(
        page.getByRole("heading", { name: "Requirement list" }),
      ).toBeVisible();
      await expect(page.getByLabel("Search")).toHaveValue("");
      await expect(page.getByLabel("Status")).toHaveValue(
        preferences.list_status ?? "",
      );
      await expect(page.getByLabel("Creator")).toHaveValue(
        preferences.list_created_by ?? "",
      );
      await expect(page.getByLabel("Updated")).toHaveValue(
        preferences.list_sort,
      );
    } else {
      await expect(page.getByRole("heading", { name: "Board" })).toBeVisible();
    }
  },
};
