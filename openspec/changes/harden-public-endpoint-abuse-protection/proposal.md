# Public Endpoint Abuse Protection

## Why

Unauthenticated `/auth/request-code` and `/daemon/setup/request` remain public resource-creation surfaces. North 0.1.0 must name this abuse boundary without adding a large rate-limiting subsystem to unrelated work.

## What Changes

- Define shared abuse protection for verification-code requests and daemon setup requests.
- Combine coarse IP/subnet limits with resource-specific quotas and safe failure behavior.
- Define trusted reverse-proxy client-IP handling and deployment assumptions.
- Add observable, integration-tested enforcement without weakening legitimate setup or login flows.

## Capabilities

### New Capabilities

- `public-endpoint-abuse-protection`: Shared abuse controls for public authentication and daemon setup request endpoints.

### Modified Capabilities

- `email-auth`: Apply shared abuse controls to public verification-code issuance.

## Impact

Future changes will affect `crates/north-server` public request handlers and middleware, deployment/proxy configuration, persistence for counters or bans if needed, integration tests, and security documentation. This proposal does not change current runtime behavior.
