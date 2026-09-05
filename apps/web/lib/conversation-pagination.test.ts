import { describe, expect, it } from "vitest";

import type { ConversationPage } from "@/lib/api/contracts";
import {
  appendConversationPage,
  buildConversationHistory,
  repairConversationHistory,
} from "@/lib/conversation-pagination";

function page(
  offset: number,
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

describe("conversation pagination repair", () => {
  it("follows shifted next offsets and re-observes every prior message", async () => {
    const previous = buildConversationHistory([
      { offset: 0, page: page(0, ["A", "B"], 2) },
      { offset: 2, page: page(2, ["C", "D"], null) },
    ]);
    const calls: number[] = [];
    const repaired = await repairConversationHistory(
      async (offset) => {
        calls.push(offset);
        if (offset === 0) return page(offset, ["X", "A"], 2);
        if (offset === 2) return page(offset, ["B", "C"], 4);
        return page(offset, ["D"], null);
      },
      previous,
      2,
    );

    expect(calls).toEqual([0, 2, 4]);
    expect(repaired.messages.map((message) => message.id)).toEqual([
      "A",
      "B",
      "C",
      "D",
      "X",
    ]);
    expect(new Set(repaired.messages.map((message) => message.id)).size).toBe(
      5,
    );
  });

  it("continues to the new end when previously loaded history was complete", async () => {
    const previous = buildConversationHistory([
      { offset: 0, page: page(0, ["A", "B"], null) },
    ]);
    const calls: number[] = [];
    const repaired = await repairConversationHistory(
      async (offset) => {
        calls.push(offset);
        if (offset === 0) return page(offset, ["X", "A"], 2);
        if (offset === 2) return page(offset, ["B", "C"], 4);
        return page(offset, ["D"], null);
      },
      previous,
      2,
    );

    expect(calls).toEqual([0, 2, 4]);
    expect(repaired.messages.map((message) => message.id)).toEqual([
      "A",
      "B",
      "C",
      "D",
      "X",
    ]);
  });

  it("does not omit prior IDs when new pages shift before an old numeric end", async () => {
    const previous = buildConversationHistory([
      { offset: 0, page: page(0, ["A", "B"], 2) },
      { offset: 2, page: page(2, ["C", "D"], 4) },
    ]);
    const calls: number[] = [];
    const repaired = await repairConversationHistory(
      async (offset) => {
        calls.push(offset);
        if (offset === 0) return page(offset, ["X", "A"], 2);
        if (offset === 2) return page(offset, ["B", "C"], 4);
        return page(offset, ["D", "E"], 6);
      },
      previous,
      2,
    );

    expect(calls).toEqual([0, 2, 4]);
    expect(repaired.messages.map((message) => message.id)).toEqual([
      "A",
      "B",
      "C",
      "D",
      "E",
      "X",
    ]);
  });

  it("deduplicates overlapping pages and follows only canonical next offsets", () => {
    const history = buildConversationHistory([
      { offset: 0, page: page(0, ["A", "B"], 2) },
    ]);
    const expanded = appendConversationPage(history, page(2, ["B", "C"], null));
    expect(expanded.messages.map((message) => message.id)).toEqual([
      "A",
      "B",
      "C",
    ]);
    expect(expanded.prior_loaded_end_offset).toBe(4);
  });
});
