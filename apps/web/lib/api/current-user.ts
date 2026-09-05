import { requestJson } from "@/lib/api/client";
import { parseCurrentUser, type CurrentUser } from "@/lib/api/contracts";

export async function getCurrentUser(): Promise<CurrentUser> {
  return parseCurrentUser(await requestJson("/auth/me"));
}
