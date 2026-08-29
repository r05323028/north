import { readFileSync } from "node:fs";

const source = readFileSync(
  new URL("./components/repository-settings.tsx", import.meta.url),
  "utf8",
);

const required = [
  ["disabled status", "disabled_at"],
  ["re-enable control", "Re-enable"],
  ["URL immutability", "readOnly={Boolean(editing)}"],
  ["UTF-8 byte validation", "TextEncoder"],
  ["normalized catalog ordering", "compareRepositories"],
  ["server error display", 'role="alert"'],
  ["disabled-name recovery", "re_enable"],
  ["URL replacement guidance", "disable_old_create_new"],
];
for (const [label, needle] of required) {
  if (!source.includes(needle))
    throw new Error(`missing Settings check: ${label}`);
}
if (source.includes("hard-delete") || source.includes("Delete repository")) {
  throw new Error("repository Settings must not expose hard delete");
}
process.stdout.write("repository Settings contract check: OK\n");
