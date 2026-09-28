---
title: Requirement states and review
description: Understand North requirement states and who can make review decisions.
---

## States

| State | Meaning |
| --- | --- |
| Draft | Created; clarification has not meaningfully started. |
| Discussing | Requester and agent are clarifying the requirement. |
| Ready | The agent considers it clear enough for human review. |
| Accepted | A reviewer accepted the requirement. |
| Rejected | A reviewer rejected it; it can be reopened. |

A reviewer can request changes from `Ready`, returning the requirement to `Discussing`. Editing a `Ready` requirement also returns it to `Discussing`; the previous assessment no longer represents its current content.

## Who reviews

All roles can create, view, discuss, and edit non-terminal requirements. Requirement Managers, Admins, and Owners can accept, reject, request changes, and reopen. Requesters cannot make reviewer decisions.

These pages summarize product rules. See the canonical [requirement lifecycle](https://github.com/r05323028/north/blob/main/docs/product/requirement-lifecycle.md), [roles and permissions](https://github.com/r05323028/north/blob/main/docs/product/roles-and-permissions.md), and [readiness](https://github.com/r05323028/north/blob/main/docs/product/readiness.md) documents for exact semantics.
