#!/usr/bin/env node

const args = process.argv.slice(2);

const fail = (message) => {
  console.error(`qualification fake agent: ${message}`);
  process.exit(2);
};

const assessment = {
  message: "Qualification clarification complete.",
  verdict: "ready",
  blockers: [],
  assumptions: [],
};

if (args.length === 1 && args[0] === "--self-check") {
  process.stdout.write(`${JSON.stringify(assessment)}\n`);
  process.exit(0);
}

const valueFlags = new Set([
  "--mode",
  "--system-prompt",
  "--session-id",
  "--session-dir",
]);
const booleanFlags = new Set([
  "--print",
  "--no-tools",
  "--no-extensions",
  "--no-skills",
  "--no-context-files",
]);
const values = new Map();
let prompt;
for (let index = 0; index < args.length; index += 1) {
  const arg = args[index];
  if (!arg.startsWith("--")) {
    if (index !== args.length - 1) fail(`unexpected positional argument: ${arg}`);
    prompt = arg;
    continue;
  }
  if (booleanFlags.has(arg)) continue;
  if (!valueFlags.has(arg)) fail(`unexpected option: ${arg}`);
  const value = args[index + 1];
  if (!value || value.startsWith("--")) fail(`missing value for ${arg}`);
  values.set(arg, value);
  index += 1;
}

if (values.get("--mode") !== "json" || !args.includes("--print")) {
  fail("runtime must request JSON print mode");
}
for (const flag of ["--no-tools", "--no-extensions", "--no-skills", "--no-context-files"]) {
  if (!args.includes(flag)) fail(`runtime must disable ${flag.slice(2)}`);
}
if (!values.get("--session-id") || !values.get("--session-dir") || !values.get("--system-prompt")) {
  fail("runtime context flags are incomplete");
}
if (!prompt || prompt.length === 0) fail("runtime prompt is empty");

const event = {
  type: "message_update",
  assistantMessageEvent: {
    type: "text_delta",
    delta: JSON.stringify(assessment),
  },
};
process.stdout.write(`${JSON.stringify(event)}\n`);
