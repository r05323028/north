# Proposal: Human-facing documentation site

## Why

North's `docs/` directory holds compact product, architecture, and development knowledge; users and contributors need task-oriented documentation with navigation suited to reading. Add a separate Astro site without replacing or mechanically copying the existing knowledge base.

## What Changes

- Add a static Astro/Starlight documentation site at repository root `web/` with user-guide, contributor-guide, and changelog sections.
- Make its homepage a landing page with direct paths into user, contributor, and release content.
- Author human-focused Markdown independently in `web/`; summaries of product facts link to canonical source documents in `docs/`.
- Point the changelog page to the repository's published GitHub Releases rather than inventing release history.
- Document the two audiences and editing rules; include the site build in the existing web validation and CI job.
- Leave `docs/` product/architecture content and `apps/web/` runtime unchanged; update only documentation indexing and workflow guidance. No domain, deployment, or app integration in this change.

## Contract markers

- **Invariant:** `web/` is a static, human-facing manual for users and contributors, distinct from the structured knowledge in `docs/`.
- **Invariant:** the homepage orients visitors and links directly to the user guide, contributor guide, and published release history.
- **Invariant:** human guides may summarize canonical product and architecture facts, but must link to `docs/` rather than establish competing specifications; no automatic import or copy from `docs/`.
- **Invariant:** changelog entries represent published release history and link to the repository's release source.
- **Implementation suggestion:** Astro with Starlight, Markdown content, npm lockfile, and the existing validation/CI path.

## Capabilities

### New Capabilities

- `human-documentation-site`: static, navigable user and contributor documentation with a landing page and real release-history entry point.

### Modified Capabilities

None.

## Dependencies

No earlier OpenSpec change required. Human guides reference existing canonical documentation and published release records.

## Impact

- New `web/` Astro/Starlight package and Markdown content.
- `README.md`, `docs/README.md`, `docs/development/documentation.md`, `docs/development/testing.md`, and `docs/development/ci.md` clarify audience, ownership, and validation.
- `scripts/validate.sh` and `.github/workflows/ci.yml` build the site through the existing web gate.
- Adds Astro/Starlight dependencies under `web/`; no application or backend dependency changes.
