import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("next/navigation", () => ({ useRouter: () => ({ replace: vi.fn() }) }));
vi.mock("@/lib/api/current-user", () => ({ getCurrentUser: vi.fn() }));

import { UserProfile } from "@/components/user-profile";

describe("UserProfile", () => {
  it("shows loading state without editable account controls", () => {
    const html = renderToStaticMarkup(<UserProfile />);
    expect(html).toContain("個人資料");
    expect(html).toContain("正在載入個人資料");
    expect(html).not.toContain("<input");
    expect(html).not.toContain("<select");
  });
});
