import { describe, expect, it } from "vitest";
import { cn } from "./utils";

describe("cn", () => {
  it("merges class names", () => {
    expect(cn("p-2", "p-4")).toBe("p-4");
  });

  it("handles conditional classes", () => {
    expect(cn("text-red-500", false && "hidden", "font-bold")).toBe(
      "text-red-500 font-bold",
    );
  });

  it("deduplicates tailwind conflicts via tailwind-merge", () => {
    expect(cn("px-2 py-1", "px-4")).toBe("py-1 px-4");
  });
});
