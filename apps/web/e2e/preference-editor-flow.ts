import type { Page } from "@playwright/test";

export type PreferenceEditorAdapter<Value> = {
  open(page: Page): Promise<void>;
  edit(page: Page, value: Value): Promise<void>;
  save(page: Page): Promise<void>;
  assertRestored(page: Page, value: Value): Promise<void>;
};

export async function exercisePreferenceEditor<Value>(
  page: Page,
  adapter: PreferenceEditorAdapter<Value>,
  value: Value,
): Promise<void> {
  await adapter.open(page);
  await adapter.edit(page, value);
  await adapter.save(page);
  await page.reload();
  await adapter.assertRestored(page, value);
}
