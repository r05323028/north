# Design

## Decisions

- repositories table: id (uuid), unique name, url, description, timestamps.
  No credential/token fields — schema-level enforcement of the no-secrets rule.
- Catalog endpoint for daemons ships with the protocol change; here only
  admin CRUD + list.
- Delete = soft disable? Keep boring: hard delete allowed; inspection history
  stores repository_id + SHA strings so it never depends on row lifetime.

## Open Questions

None.
