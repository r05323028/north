import type {
  Conversation,
  ConversationPage,
  Message,
} from "@/lib/api/contracts";

export type ConversationPageSlice = {
  offset: number;
  page: ConversationPage;
};

export type ConversationHistory = {
  conversation: Conversation;
  pages: ConversationPageSlice[];
  messages: Message[];
  next_offset: number | null;
  prior_loaded_end_offset: number;
  reached_end: boolean;
};

function compareMessages(left: Message, right: Message): number {
  if (left.created_at < right.created_at) return -1;
  if (left.created_at > right.created_at) return 1;
  if (left.id < right.id) return -1;
  if (left.id > right.id) return 1;
  return 0;
}

export function mergeMessages(pages: ConversationPageSlice[]): Message[] {
  const byId = new Map<string, Message>();
  for (const { page } of pages) {
    for (const message of page.messages) byId.set(message.id, message);
  }
  return [...byId.values()].sort(compareMessages);
}

function contiguousPages(pages: ConversationPageSlice[]): {
  pages: ConversationPageSlice[];
  endOffset: number;
  nextOffset: number | null;
  reachedEnd: boolean;
} {
  const byOffset = new Map(pages.map((slice) => [slice.offset, slice]));
  const chain: ConversationPageSlice[] = [];
  const visited = new Set<number>();
  let offset = 0;
  let endOffset = 0;
  let nextOffset: number | null = null;
  let reachedEnd = false;

  while (!visited.has(offset)) {
    const slice = byOffset.get(offset);
    if (!slice) break;
    visited.add(offset);
    chain.push(slice);
    endOffset = offset + slice.page.messages.length;
    nextOffset = slice.page.next_offset;
    if (nextOffset === null) {
      reachedEnd = true;
      break;
    }
    if (nextOffset <= offset) break;
    offset = nextOffset;
  }

  return { pages: chain, endOffset, nextOffset, reachedEnd };
}

export function buildConversationHistory(
  pages: ConversationPageSlice[],
): ConversationHistory {
  if (pages.length === 0) {
    throw new Error("Conversation history requires a page at offset 0");
  }
  const chain = contiguousPages(pages);
  const first = chain.pages[0];
  return {
    conversation: {
      id: first.page.id,
      requirement_id: first.page.requirement_id,
      created_at: first.page.created_at,
    },
    pages: chain.pages,
    messages: mergeMessages(chain.pages),
    next_offset: chain.nextOffset,
    prior_loaded_end_offset: chain.endOffset,
    reached_end: chain.reachedEnd,
  };
}

export function appendConversationPage(
  current: ConversationHistory,
  page: ConversationPage,
  offset = current.next_offset,
): ConversationHistory {
  if (offset === null) return current;
  return buildConversationHistory([
    ...current.pages.filter((slice) => slice.offset !== offset),
    { offset, page },
  ]);
}

export async function loadConversationHistory(
  fetchPage: (offset: number, limit: number) => Promise<ConversationPage>,
  limit = 50,
): Promise<ConversationHistory> {
  return buildConversationHistory([
    { offset: 0, page: await fetchPage(0, limit) },
  ]);
}

export async function repairConversationHistory(
  fetchPage: (offset: number, limit: number) => Promise<ConversationPage>,
  previous: ConversationHistory,
  limit = 50,
): Promise<ConversationHistory> {
  const previousIds = new Set(previous.messages.map((message) => message.id));
  const refreshed: ConversationPageSlice[] = [];
  const observedIds = new Set<string>();
  let offset = 0;
  const requestedOffsets = new Set<number>();
  let endOffset = 0;

  while (!requestedOffsets.has(offset)) {
    requestedOffsets.add(offset);
    const page = await fetchPage(offset, limit);
    refreshed.push({ offset, page });
    for (const message of page.messages) observedIds.add(message.id);
    endOffset = offset + page.messages.length;

    const reobserved = [...previousIds].every((id) => observedIds.has(id));
    const rebuiltPriorRange = endOffset >= previous.prior_loaded_end_offset;
    const throughCurrentEnd = previous.reached_end && page.next_offset === null;
    if (
      previous.reached_end ? throughCurrentEnd : rebuiltPriorRange && reobserved
    )
      break;
    if (page.next_offset === null || page.next_offset <= offset) break;
    offset = page.next_offset;
  }

  return buildConversationHistory(refreshed);
}

export const repairConversationPages = repairConversationHistory;
