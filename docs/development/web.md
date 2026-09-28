# Web development

## Next.js documentation

For changes under `apps/web/`, read the relevant guide from the installed Next.js
package at `apps/web/node_modules/next/dist/docs/` before writing code. In this
monorepo, resolve the package from `apps/web/`; it may not be visible from the
repository root. APIs, conventions, and file structure can differ from other
Next.js versions. Follow the installed guide and heed deprecation notices.

## Generated agent guidance

`apps/web/AGENTS.md` contains a Next.js-managed rules block. `next dev` may
update or recreate that block when it detects an AI coding agent. Generator:
`apps/web/node_modules/next/dist/server/lib/generate-agent-files.js`.
Keep the managed block in place; North-specific durable guidance belongs in the
root `AGENTS.md` and this document. If both app-local `AGENTS.md` and
`CLAUDE.md` are absent, the generator scaffolds both.
