import { requestJson } from "@/lib/api/client";
import {
  parseConversationPage,
  parseMessage,
  type ConversationPage,
  type Message,
} from "@/lib/api/contracts";

export const CONVERSATION_PAGE_LIMIT = 50;

function requirementConversationPath(requirementId: string): string {
  return `/requirements/${encodeURIComponent(requirementId)}/conversation`;
}

function pagePath(
  requirementId: string,
  offset: number,
  limit: number,
): string {
  if (
    !Number.isSafeInteger(offset) ||
    offset < 0 ||
    !Number.isSafeInteger(limit) ||
    limit < 1 ||
    limit > 100
  ) {
    throw new RangeError(
      "Conversation page must use a bounded offset and limit",
    );
  }
  return `${requirementConversationPath(requirementId)}?offset=${offset}&limit=${limit}`;
}

export async function getConversationPage(
  requirementId: string,
  offset = 0,
  limit = CONVERSATION_PAGE_LIMIT,
): Promise<ConversationPage> {
  return parseConversationPage(
    await requestJson(pagePath(requirementId, offset, limit)),
  );
}

export async function postRequesterMessage(
  requirementId: string,
  body: string,
): Promise<Message> {
  return parseMessage(
    await requestJson(
      `${requirementConversationPath(requirementId)}/messages`,
      {
        method: "POST",
        body: JSON.stringify({ body }),
      },
    ),
  );
}
