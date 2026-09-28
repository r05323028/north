# Design

## Context

See `proposal.md` for motivation. `apps/web/` is the Next.js product UI; root `docs/` contains canonical product, architecture, and development knowledge. The existing CI web job installs `apps/web` dependencies and invokes `./scripts/validate.sh web`.

## Goals / Non-Goals

**Goals:**

- Isolate human documentation dependencies and content under root `web/`.
- Use built-in documentation navigation and search; make the static build part of the existing web validation gate.
- Keep canonical product and architecture facts in `docs/` and release history linked to GitHub Releases.
- Give first-time visitors clear paths into user, contributor, and release content.

**Non-Goals:**

- Move or auto-import files from `docs/`.
- Change the Next.js app, backend, release process, or deployment topology.
- Choose a public domain, hosting provider, or base path before deployment is requested.

## Decisions

1. **Standalone package at `web/`.** Use a separate npm package and lockfile rather than adding Astro to the root or reusing `apps/web/`. This respects the requested path and prevents documentation dependencies from entering the product app. A root workspace is unnecessary for one independent site.
2. **Astro Starlight for docs UI.** Use Starlight's built-in accessible docs shell, sidebar, and search instead of building custom Astro components or wiring a separate search service. Keep content in `web/src/content/docs/`, grouped into user guide, contributing, and changelog pages.
3. **Separate authoring, shared authority.** Write reader-oriented Markdown in `web/`; do not load `../docs` into the site. Summaries link to canonical source documents on GitHub. The changelog links to the repository Releases page; no synthetic release notes are created.
4. **Reuse existing validation route.** Extend `./scripts/validate.sh web` with the site build and install the new lockfile in the existing CI `web` job. This avoids a second CI workflow while ensuring the generated pages are checked.
5. **Static output at site root.** Build static files and leave `site`/base-path deployment settings unset until a host is selected. Hosting can later set the public URL without changing page organization.
6. **Use Starlight's splash homepage.** Put a short value statement and direct actions in frontmatter, then use ordinary Markdown for the guide choices and canonical source link. Avoid MDX components, custom CSS, and imagery on the initial landing page.

## Risks / Trade-offs

- Human summaries can drift from canonical facts → Keep normative detail in `docs/`, link from guides, and update affected guides when behavior changes.
- The Releases page may initially have no entries → Link to the real release source and do not infer published history from OpenSpec proposals.
- Hosting may later require a base path → Keep URLs relative within Starlight and set deployment configuration only when a host is chosen.

## Migration Plan

No existing content moves. Install `web/` dependencies with its lockfile and run the site build through `./scripts/validate.sh web`. Rollback removes the standalone `web/` package and its validation/CI/docs references; product runtime remains unaffected.
