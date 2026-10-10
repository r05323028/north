# Compose Image Build Fallback Specification

## Purpose

Provides an opt-in local-source build overlay for North server and web images without changing image-only release Compose behavior.

## Requirements

### Requirement: Compose supports local source builds without release publication
The standard `docker-compose.yaml` SHALL remain image-only and preserve configured release image references. An opt-in `docker-compose.build.yaml` SHALL override server and web image references with local tags, disable app-image registry pulls, and build from the current checkout using dedicated local Dockerfiles. The build SHALL NOT require prepared `dist/` artifacts, a SemVer tag, or GitHub release/publication. The release Dockerfiles SHALL remain package-backed.

#### Scenario: Local source build
- **WHEN** an operator uses both Compose files and local app images do not exist
- **THEN** Compose builds the server and web images from the checked-out Rust and Next.js sources without pulling North app images from a registry

#### Scenario: Explicit rebuild
- **WHEN** an operator runs `docker compose ... up --build` with the local overlay
- **THEN** Compose rebuilds both local images from source before starting services

#### Scenario: Release deployment
- **WHEN** an operator uses only `docker-compose.yaml`
- **THEN** Compose uses configured release image references and does not build application source

#### Scenario: Build context safety
- **WHEN** Compose sends local source build contexts
- **THEN** Docker ignore rules exclude environment files and generated build artifacts from both contexts
