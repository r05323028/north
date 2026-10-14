# Spec Delta

## ADDED Requirements

### Requirement: AI-assisted documentation workflows use locked dependencies

The OpenWiki update workflow MUST install OpenWiki and its Mermaid/jsdom helper tools from committed npm package metadata and lockfile using `npm ci`. It MUST NOT use a global or other unlocked npm install. The workflow MUST invoke the repository-local OpenWiki binary so execution uses the locked installation.

#### Scenario: Workflow installs and runs locked tools

- **WHEN** the workflow installs tools and runs OpenWiki
- **THEN** it uses `npm ci --prefix .github/openwiki-tools` with the committed lockfile, then invokes `.github/openwiki-tools/node_modules/.bin/openwiki` from the repository root
