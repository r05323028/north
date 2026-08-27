## Purpose

Protects public authentication and daemon setup request surfaces from automated abuse while preserving legitimate self-hosted login and browser-assisted setup.

## ADDED Requirements

### Requirement: Public request surfaces have bounded abuse protection

The public endpoints that issue authentication codes or create daemon setup requests SHALL apply bounded abuse controls. Controls SHALL limit both broad client activity and repeated requests for one resource, SHALL fail safely without exposing secrets, and SHALL define how trusted reverse-proxy client identity is determined when deployed behind a proxy.

#### Scenario: Repeated code requests are bounded

- **WHEN** one client repeatedly requests verification codes
- **THEN** the endpoint eventually rejects requests within its configured resource and client limits without returning a verification code

#### Scenario: Repeated daemon setup requests are bounded

- **WHEN** one client repeatedly creates daemon setup requests
- **THEN** the endpoint eventually rejects requests within its configured resource and client limits without returning daemon credentials

#### Scenario: Proxy client identity is explicit

- **WHEN** North runs behind a trusted reverse proxy that supplies client identity
- **THEN** abuse controls use the documented trusted proxy value and do not treat arbitrary end-user headers as authoritative
