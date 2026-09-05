import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const workspaceSource = readFileSync(
  resolve(process.cwd(), "components/requirement-conversation-workspace.tsx"),
  "utf8",
);
const workspaceHookSource = readFileSync(
  resolve(process.cwd(), "lib/use-requirement-conversation-workspace.ts"),
  "utf8",
);

describe("workspace privacy and transport boundaries", () => {
  it("keeps product UI free of runtime internals and duplicate canonical entities", () => {
    for (const source of [workspaceSource, workspaceHookSource]) {
      expect(source).not.toMatch(
        /chain[- ]of[- ]thought|raw prompt|tool trace/i,
      );
      expect(source).not.toMatch(
        /daemon_id|provider|credential|checkout|command envelope/i,
      );
      const browserSocketConstructor = ["new", "WebSocket"].join(" ");
      expect(source).not.toMatch(/dangerouslySetInnerHTML|Last-Event-ID/i);
      expect(source).not.toContain(browserSocketConstructor);
    }
    expect(workspaceSource).not.toMatch(/type (Message|Requirement)\s*[={<]/);
    expect(workspaceSource).not.toMatch(/new EventSource/);
    expect(workspaceHookSource).not.toMatch(/new EventSource/);
  });
});
