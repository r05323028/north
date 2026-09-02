import type { Metadata } from "next";
import Script from "next/script";

import { NorthShell } from "@/components/north-shell";
import "./globals.css";

export const metadata: Metadata = {
  title: "North",
  description:
    "Self-hosted requirement management: turn ambiguous requests into structured, reviewable requirements.",
};

const themeScript = `(() => {
  try {
    const key = "north-theme";
    const stored = localStorage.getItem(key) || "system";
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = (value) => {
      const dark = value === "dark" || (value === "system" && media.matches);
      const root = document.documentElement;
      root.dataset.theme = dark ? "dark" : "light";
      root.classList.toggle("dark", dark);
      root.style.colorScheme = dark ? "dark" : "light";
    };
    apply(stored);
    media.addEventListener("change", () => {
      if ((localStorage.getItem(key) || "system") === "system") apply("system");
    });
    window.__setTheme = (value) => {
      localStorage.setItem(key, value);
      apply(value);
      window.dispatchEvent(new CustomEvent("themechange", { detail: value }));
    };
    window.__getTheme = () => localStorage.getItem(key) || "system";
  } catch {}
})();`;

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="zh-TW" suppressHydrationWarning>
      <body className="antialiased">
        <NorthShell>{children}</NorthShell>
        <Script id="north-theme" strategy="beforeInteractive">
          {themeScript}
        </Script>
      </body>
    </html>
  );
}
