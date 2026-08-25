import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "North",
  description:
    "Self-hosted requirement management: turn ambiguous requests into structured, reviewable requirements.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body className="antialiased">{children}</body>
    </html>
  );
}
