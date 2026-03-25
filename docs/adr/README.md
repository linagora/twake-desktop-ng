# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for the Twake Desktop NG project.

## What is an ADR?

An ADR documents a significant architectural decision made for this project. Each ADR captures:

- The context and problem
- The decision taken
- The consequences (positive and negative)

## Template

Use this template for new ADRs:

```markdown
# [ADR-0001]: [Title]

## Status

[Proposed | Accepted | Deprecated | Superseded]

## Context

[What is the issue that we're seeing that is motivating this decision?]

## Decision

[What is the change that we're proposing and/or doing?]

## Consequences

[What becomes easier or more difficult to do because of this change?]
```

## ADR Index

| ID   | Title                           | Status   | Date       |
| ---- | ------------------------------- | -------- | ---------- |
| 0001 | Initial ADR setup               | Accepted | 2026-03-25 |
| 0002 | Authentication flow (OIDC PKCE) | Proposed | 2026-03-25 |

## How to Use

1. Create a new file: `docs/adr/ADR-XXXX-[title].md`
2. Number sequentially (0001, 0002, ...)
3. Update this index when adding new ADRs
4. Keep ADRs concise and focused on a single decision
