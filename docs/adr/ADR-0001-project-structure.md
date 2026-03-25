# ADR-0001: Project Structure and Documentation Organization

## Status

Accepted

## Context

The Twake Desktop NG project has grown multiple documentation files with overlapping content:
- `docs/spec.md` (853 lines) - Technical specification
- `PLAN.md` (545 lines) - 6-week development plan
- `PLAN_HACKATON.md` (322 lines) - 48-hour hackathon plan
- `STREAM_A_CEF.md`, `STREAM_B_SYNC_CORE.md`, `STREAM_C_IPC_NETWORK.md` - Implementation guides
- `INTERFACES.md` (543 lines) - Interface contracts
- `docs/spec-draft.md` - Earlier draft

This creates confusion about:
- Where to find authoritative information
- Which document to update when requirements change
- How to navigate the documentation for different audiences (architects vs developers)

## Decision

Adopt a hierarchical documentation structure with clear separation of concerns:

```
docs/
├── spec.md                    # Architecture overview (200-300 lines)
│   └── "Master document" - links to detailed specs
│
├── superpowers/
│   └── specs/                 # Detailed design specs
│       ├── YYYY-MM-DD-vfs-engine-design.md
│       ├── YYYY-MM-DD-reconciliation-engine-design.md
│       ├── YYYY-MM-DD-ipc-contract-design.md
│       └── YYYY-MM-DD-CEF-shell-design.md
│
└── adr/
    ├── README.md              # ADR index
    ├── ADR-0001-project-structure.md
    ├── ADR-0002-authentication-flow.md
    └── ADR-0003-two-process-architecture.md
```

**Root level files (unchanged):**
- `PLAN.md` - 6-week development roadmap
- `PLAN_HACKATON.md` - 48-hour hackathon plan
- `STREAM_A/B/C.md` - Implementation guides for parallel development

**Document purposes:**

| Document | Audience | Purpose | Update Frequency |
|----------|----------|---------|------------------|
| `docs/spec.md` | Architects, new devs | High-level architecture overview | Rare |
| `docs/superpowers/specs/*.md` | Developers, implementers | Detailed design for specific components | Per feature |
| `docs/adr/*.md` | Architects, reviewers | Documented decisions with rationale | Per major decision |
| `PLAN.md` | Project manager, devs | Development timeline and milestones | Weekly |
| `STREAM_*.md` | Developers | Step-by-step implementation guide | During development |

## Consequences

### Positive

1. **Clear navigation** - Each document has a single purpose
2. **Modular updates** - Change one spec without touching others
3. **Parallel development** - Each stream can reference its own spec
4. **Historical record** - ADRs capture why decisions were made
5. **Scalable** - New features add new specs without bloating existing docs

### Negative

1. **Initial migration cost** - Need to restructure existing content
2. **Cross-references** - Must maintain links between documents
3. **Potential fragmentation** - Risk of specs becoming outdated if not maintained

### Trade-offs

**Chose hierarchical over flat structure:**
- More complex initially but scales better
- Easier to maintain consistency within each spec
- Harder to get "big picture" without reading multiple docs

**Chose to keep STREAM_*.md files:**
- They serve as implementation guides, not specs
- Useful for developers working in parallel
- Can be updated independently of design specs

## Migration Plan

1. Create ADR-0001 (this document)
2. Reduce `docs/spec.md` to overview (200-300 lines)
3. Extract detailed sections into `docs/superpowers/specs/`
4. Update cross-references in all documents
5. Add ADR-0003 for two-process architecture (extracted from spec.md)
6. Update ADR README index

## References

- [opencode brainstorming skill](https://opencode.ai) - Design spec workflow
- Existing `docs/spec.md` - Source material for migration
