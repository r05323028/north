import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { RequirementBoardView } from "@/components/requirement-board";
import { RequirementDetailView } from "@/components/requirement-detail";
import type { Requirement } from "@/lib/requirements";

const canonical: Requirement = {
  id: "r-1",
  title: "Account login",
  description: "Users can sign in.",
  summary: "One account per user.",
  acceptance_criteria: ["A valid account can sign in."],
  assumptions: ["Email is unique."],
  open_questions: ["Which provider is used?"],
  status: "Ready",
  revision: 3,
  state_version: 4,
  created_by: "user-1",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
};

function state(requirements: Requirement[]) {
  return {
    requirements,
    loading: false,
    refreshing: false,
    error: null,
    onCreateAction: () => undefined,
  };
}

describe("Requirement presentation", () => {
  it("renders all fixed lifecycle columns and detail links", () => {
    const html = renderToStaticMarkup(
      <RequirementBoardView
        {...state([
          { ...canonical, id: "draft", status: "Draft" },
          canonical,
          { ...canonical, id: "accepted", status: "Accepted" },
        ])}
      />,
    );

    for (const status of [
      "Draft",
      "Discussing",
      "Ready",
      "Accepted",
      "Rejected",
    ]) {
      expect(html).toContain(`data-status="${status}"`);
    }
    expect(html).toContain("Account login");
    expect(html).toContain("/requirements/r-1");
  });

  it("renders canonical read-only detail fields without runtime data", () => {
    const html = renderToStaticMarkup(
      <RequirementDetailView requirement={canonical} />,
    );

    for (const value of [
      "Account login",
      "Users can sign in.",
      "Ready",
      "user-1",
      "2026-01-02T00:00:00Z",
      "One account per user.",
      "A valid account can sign in.",
      "Email is unique.",
      "Which provider is used?",
      "3",
      "4",
    ]) {
      expect(html).toContain(value);
    }
  });
});
