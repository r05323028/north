# Spec Delta

## MODIFIED Requirements

### Requirement: Hook policy through prek

Git hooks SHALL be managed by prek using `.pre-commit-config.yaml`. Pre-commit SHALL stay fast (hygiene and formatting only). Strict OpenSpec validation SHALL run through the shared validation entrypoint, pre-push gate, CI, and Act parity; it need not run as a file-mutating pre-commit hook. Pre-push SHALL invoke one reusable script running the native gate plus Act-based GitHub Actions parity, with a documented escape hatch. For its local native CI run, pre-push SHALL automatically provision a disposable PostgreSQL 16 database without requiring a user-supplied `NORTH_TEST_DATABASE_URL`, pass the dynamically mapped URL only to the child validation process, use non-persistent database storage, remove the container after that process exits without leaving an anonymous volume, and preserve its exit status. On SIGINT or SIGTERM while the child runs, pre-push SHALL terminate the child, remove the container, and return `128 + signal`. Unless the explicit native-only escape hatch is selected, pre-push SHALL fail when Act reports that the selected workflow job was skipped for an unsupported platform. Failure to provision the database MUST fail closed before native CI and Act run. Direct `validate.sh ci` and `integration` invocations SHALL continue requiring an explicit database URL so hosted CI can use its existing PostgreSQL service. Commit messages SHALL be validated by a shared script usable by both the hook and CI.

#### Scenario: Non-conventional subject is rejected at commit time

- **WHEN** a commit-msg hook receives subject "added a thing"
- **THEN** the shared validator rejects it with usage guidance

#### Scenario: Pre-push runs the same commands CI runs

- **WHEN** the pre-push hook fires
- **THEN** it executes the shared validate.sh profiles and replays a real workflow job via Act rather than a hand-maintained copy

#### Scenario: Local pre-push provisions disposable PostgreSQL

- **WHEN** local pre-push runs with Docker available and no `NORTH_TEST_DATABASE_URL` set by the user
- **THEN** it starts disposable PostgreSQL 16, passes its mapped connection URL to `validate.sh ci`, and removes the container after native validation exits

#### Scenario: Disposable storage leaves no volume

- **WHEN** local pre-push finishes native validation
- **THEN** its PostgreSQL container and any storage created for it are removed

#### Scenario: PostgreSQL startup failure stops pre-push

- **WHEN** disposable PostgreSQL cannot start or its connection endpoint cannot be discovered
- **THEN** pre-push fails before running native CI or Act and reports the provisioning failure

#### Scenario: Failed native validation cleans up and preserves failure

- **WHEN** `validate.sh ci` exits non-zero inside the disposable database scope
- **THEN** the container is removed, the same failure status is returned, and Act does not run

#### Scenario: Unsupported Act skip is not success

- **WHEN** Act reports the selected job skipped for an unsupported platform and the native-only escape hatch was not selected
- **THEN** pre-push exits non-zero and explains that workflow parity did not run

#### Scenario: Interrupt native validation safely

- **WHEN** runner receives SIGINT or SIGTERM while the validation child runs
- **THEN** it terminates the child, removes the PostgreSQL container, and returns 130 or 143 respectively

#### Scenario: Hosted CI keeps its configured database

- **WHEN** hosted CI invokes `validate.sh ci` with its PostgreSQL service URL
- **THEN** the validation profile uses that explicit URL and does not start a local Testcontainers database
