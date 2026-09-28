---
title: Contributor guide
description: Set up North's repository and validate contributions.
---

North is a Rust workspace with a Next.js product UI, PostgreSQL persistence, and an independent Astro documentation site.

## Start here

1. Read the repository's [`AGENTS.md`](https://github.com/r05323028/north/blob/main/AGENTS.md) and [documentation index](https://github.com/r05323028/north/blob/main/docs/README.md).
2. For behavior changes, create an [OpenSpec change](https://github.com/r05323028/north/tree/main/openspec/changes) before implementation. Keep product and architecture facts in their canonical `docs/` pages; update this site when human guidance changes.
3. Run the fast validation before handing off:

   ```bash
   ./scripts/validate.sh fast
   ```

   Full CI validation also runs PostgreSQL-backed tests and requires `NORTH_TEST_DATABASE_URL`:

   ```bash
   ./scripts/validate.sh ci
   ```

## Work on this site

```bash
cd web
npm ci
npm run dev
```

Build the static site with `npm run build`. Keep site content task-oriented and link to canonical `docs/` sources instead of copying their specifications.
