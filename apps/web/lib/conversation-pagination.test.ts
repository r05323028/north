import { describe, expect, it } from "vitest";

import type { ConversationPage } from "@/lib/api/contracts";
import {
  appendConversationPage,
  buildConversationHistory,
  repairConversationHistory,
} from "@/lib/conversation-pagination";

function page(
  ids: string[],
  next_offset: number | null,
): ConversationPage {
  return {
    id: "conversation-1",
    requirement_id: "requirement-1",
    created_at: "2026-01-01T00:00:00Z",
    messages: ids.map((id) => ({
      id,
      conversation_id: "conversation-1",
      author_user_id: "user-1",
      kind: "requester" as const,
      body: id,
      created_at: `2026-01-01T00:00:0${Number(id.slice(1))}Z`,
    })),
    next_offset,
  };
}

async function repairShiftedPages(
  previous: ReturnType<typeof buildConversationHistory>,
  finalIds: string[],
  finalNextOffset: number | null,
) {
  const calls: number[] = [];
  const repaired = await repairConversationHistory(
    async (offset) => {
      calls.push(offset);
      if (offset === 0) return page(["X", "A"], 2);
      if (offset === 2) return page(["B", "C"], 4);
      return page(finalIds, finalNextOffset);
    },
    previous,
    2,
  );
  return { calls, messages: repaired.messages };
}

function expectShiftedRepair(
  result: Awaited<ReturnType<typeof repairShiftedPages>>,
  expectedIds: string[],
) {
  expect(result.calls).toEqual([0, 2, 4]);
  expect(result.messages.map((message) => message.id)).toEqual(expectedIds);
  expect(new Set(result.messages.map((message) => message.id)).size).toBe(
    expectedIds.length,
  );
}

describe("conversation pagination repair", () => {
  it("follows shifted next offsets and re-observes every prior message", async () => {
    const result = await repairShiftedPages(
      buildConversationHistory([
        { offset: 0, page: page(["A", "B"], 2) },
        { offset: 2, page: page(["C", "D"], null) },
      ]),
      ["D"],
      null,
    );
    expectShiftedRepair(result, ["A", "B", "C", "D", "X"]);
  });

  it("continues to the new end when previously loaded history was complete", async () => {
    const result = await repairShiftedPages(
      buildConversationHistory([
        { offset: 0, page: page(["A", "B"], null) },
      ]),
      ["D"],
      null,
    );
    expectShiftedRepair(result, ["A", "B", "C", "D", "X"]);
  });

  it("does not omit prior IDs when new pages shift before an old numeric end", async () => {
    const result = await repairShiftedPages(
      buildConversationHistory([
        { offset: 0, page: page(["A", "B"], 2) },
        { offset: 2, page: page(["C", "D"], 4) },
      ]),
      ["D", "E"],
      6,
    );
    expectShiftedRepair(result, ["A", "B", "C", "D", "E", "X"]);
  });

  it("deduplicates overlapping pages and follows only canonical next offsets", () => {
    const history = buildConversationHistory([
      { offset: 0, page: page(["A", "B"], 2) },
    ]);
    const expanded = appendConversationPage(history, page(["B", "C"], null));
    expect(expanded.messages.map((message) => message.id)).toEqual([
      "A",
      "B",
      "C",
    ]);
    expect(expanded.prior_loaded_end_offset).toBe(4);
  });
});
