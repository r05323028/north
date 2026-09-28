# Tasks

## 1. Create the human-facing site

- [x] 1.1 Add the standalone Astro/Starlight package, lockfile, static configuration, and grouped navigation; verified with `npm ci && npm run build` via `./scripts/validate.sh web`.
- [x] 1.2 Add site-owned user, contributor, and changelog pages with canonical-source and release links; verified generated routes, navigation, source links, release URL, and search index.
- [x] 1.3 Turn the homepage into a Starlight splash landing page with direct user, contributor, and changelog paths; verified the static build and rendered links.

## 2. Integrate repository guidance and validation

- [x] 2.1 Document the distinct audiences, source-of-truth rule, contributor entry point, and site commands; verified referenced repository paths resolve.
- [x] 2.2 Add the site build to `./scripts/validate.sh web` and install its lockfile in the existing CI web job; `./scripts/validate.sh web` and actionlint passed; generated output is ignored by Git.

## 3. Verify the change

- [x] 3.1 `openspec validate --all --strict`, `./scripts/validate.sh fast`, and architecture tests passed (architecture tests run in fast profile).
- [x] 3.2 Run `./scripts/pre-push-validation.sh` because source and CI files changed; disposable PostgreSQL 16 Testcontainers runner, native `ci`, and Act Rust job passed with `NORTH_PRE_PUSH_TIMEOUT=3600`.
