import { act } from "react";
import type { ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

const { pathname } = vi.hoisted(() => ({ pathname: { value: "/" } }));

vi.mock("next/navigation", () => ({
  usePathname: () => pathname.value,
}));
vi.mock("next/link", () => ({
  default: ({
    children,
    href,
    onClick,
  }: {
    children: ReactNode;
    href: string;
    onClick?: () => void;
  }) => (
    <a href={href} onClick={onClick}>
      {children}
    </a>
  ),
}));

import { NorthShell, PageHeader } from "@/components/north-shell";

type ThemeMode = "system" | "light" | "dark";
type ThemeWindow = Window & {
  __getTheme?: () => ThemeMode;
  __setTheme?: (value: ThemeMode) => void;
};

describe("NorthShell", () => {
  let root: ReturnType<typeof createRoot> | undefined;
  let container: HTMLDivElement | undefined;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    pathname.value = "/";
    document.body.style.overflow = "";
    delete (window as ThemeWindow).__getTheme;
    delete (window as ThemeWindow).__setTheme;
  });

  it("renders navigation states and optional page header content", () => {
    for (const value of [
      "/",
      "/requirements/r-1",
      "/settings/daemons",
      "/settings/repositories",
      "/admin/users",
    ]) {
      pathname.value = value;
      const html = renderToStaticMarkup(
        <NorthShell>
          <p>content</p>
        </NorthShell>,
      );
      expect(html).toContain("主導覽");
      expect(html).toContain("content");
      expect(html).toContain("執行狀態");
      expect(html).toContain("儲存庫");
      expect(html).toContain("成員");
    }

    const header = renderToStaticMarkup(
      <PageHeader
        actions={<button type="button">Action</button>}
        description="Description"
        eyebrow="Eyebrow"
        title="Title"
      />,
    );
    expect(header).toContain("Eyebrow");
    expect(header).toContain("Title");
    expect(header).toContain("Description");
    expect(header).toContain("Action");
  });

  it("opens, closes, and cycles theme through mobile controls", async () => {
    let mode: ThemeMode = "light";
    const themeWindow = window as ThemeWindow;
    themeWindow.__getTheme = () => mode;
    themeWindow.__setTheme = (value) => {
      mode = value;
    };
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    await act(async () => {
      root?.render(
        <NorthShell>
          <p>content</p>
        </NorthShell>,
      );
    });

    const theme = container.querySelector<HTMLButtonElement>(
      '[aria-label="切換主題"]',
    );
    const open = container.querySelector<HTMLButtonElement>(
      '[aria-label="開啟導覽"]',
    );
    const overlay = container.querySelector<HTMLButtonElement>(
      '[aria-label="關閉導覽"]',
    );
    if (!theme || !open || !overlay) throw new Error("shell controls missing");

    expect(theme.title).toBe("主題：淺色");
    await act(async () => theme.click());
    expect(mode).toBe("dark");
    expect(theme.title).toBe("主題：深色");

    await act(async () => open.click());
    expect(open.getAttribute("aria-expanded")).toBe("true");
    expect(document.body.style.overflow).toBe("hidden");

    await act(async () => overlay.click());
    expect(open.getAttribute("aria-expanded")).toBe("false");
    expect(document.body.style.overflow).toBe("");

    await act(async () => open.click());
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(open.getAttribute("aria-expanded")).toBe("false");

    mode = "system";
    await act(async () => window.dispatchEvent(new Event("themechange")));
    expect(theme.title).toBe("主題：跟隨系統");

    mode = "invalid" as ThemeMode;
    await act(async () => window.dispatchEvent(new Event("themechange")));
    expect(theme.title).toBe("主題：跟隨系統");

    await act(async () => open.click());
    const repository = container.querySelector<HTMLAnchorElement>(
      'a[href="/settings/repositories"]',
    );
    if (!repository) throw new Error("repository link missing");
    repository.addEventListener(
      "click",
      (event) => event.preventDefault(),
      { once: true },
    );
    await act(async () => repository.click());
    expect(open.getAttribute("aria-expanded")).toBe("false");
  });
});
