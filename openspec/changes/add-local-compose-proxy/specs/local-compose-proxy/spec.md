# Spec Delta

## Purpose

Provides an opt-in local Compose edge that lets the browser use one loopback origin for Next.js pages and North server APIs without changing release Compose behavior.

## ADDED Requirements

### Requirement: Local proxy routes browser traffic by request semantics
The optional local proxy stack SHALL expose the web application at the configured loopback web port as one browser origin. It SHALL route `/auth`, `/requirements`, `/events`, `/daemon`, `/daemons`, `/users`, and `/repositories` server paths to North server and route other web requests to Next.js. The proxy SHALL preserve session cookies and request origin/host information required by same-origin authorization.

#### Scenario: Browser API request reaches North server
- **WHEN** the browser sends an API request to a documented server path through the local web origin
- **THEN** the request reaches North server rather than Next.js, and server authentication/authorization responses are returned to the browser

#### Scenario: Requirement document request reaches Next.js
- **WHEN** a `GET` or `HEAD` request for exactly `/requirements/{id}` identifies a browser document or Next.js RSC/prefetch request through its documented headers
- **THEN** the request reaches Next.js

#### Scenario: Requirement API requests reach North server
- **WHEN** a request for `/requirements/{id}` is headerless, requests JSON, uses a mutation method, or targets a subpath
- **THEN** the request reaches North server

#### Scenario: SSE and daemon WebSocket remain usable
- **WHEN** a client opens `/events` or upgrades `/daemon/ws` through the local origin
- **THEN** the proxy forwards the server response as a live SSE stream or WebSocket connection without buffering or breaking the upgrade

#### Scenario: Proxy remains opt-in
- **WHEN** an operator starts the standard release Compose file without the local proxy overlay
- **THEN** the release Compose services and host-port behavior remain unchanged, with the operator-managed TLS proxy contract still applying
