# Circuit — Implementation Roadmap

**Status:** Draft for review
**Date:** 2026-06-14
**Companion to:** `2026-06-14-circuit-vision-design.md`

This roadmap sequences the build into five milestones. Each milestone produces **working, testable software on its own** and gets its own task-by-task implementation plan (in `docs/superpowers/plans/`) when it's reached. This document is milestone-level: goal, deliverable, components, dependencies, parallelization, and exit criteria.

## Parallelization reality

The milestones are a **dependency chain**, not five independent tracks — they cannot all run in parallel:

- M2 needs M1's architecture graph + the `.circuit/` data model.
- M3 (projection) needs M2's session model to attach projections to sessions.
- M4 (reconciliation) needs both the data model (M1) and sessions (M2/M3) to diff reality against intent.
- M5 (harness/ecosystem breadth) needs stable core surfaces from M1–M3.

**Where parallelism is real:** *within* a milestone. Independent analyzers, adapters, and renderers have no shared state and are ideal parallel-session work. Each milestone plan is therefore authored as a **task DAG** — Circuit dogfooding its own fan-out model — with parallel-eligible nodes marked. The guidance "implement all in parallel" applies at the task level inside a milestone, not across milestones.

---

## M1 — Derivation + visualization core

**Goal:** Prove the hardest, most differentiating capability — deterministic architecture derivation rendered as a live diagram — on a real repo, with no UI shell required.

**Walking skeleton (first plan):** Parse **Rust** (Circuit analyzes its own repo → instant dogfood) with tree-sitter → build the module/import graph → compute the **Dependency rule** and **No-cycles (ADP)** indicators → render a **mermaid** architecture diagram. CLI-first.

**Components:**
- `lang` adapter trait + Rust implementation (tree-sitter parse → symbols + imports).
- Architecture graph model (modules, layers, edges) — *derived, never stored*.
- Indicator engine (deterministic subset): Architecture → Dependency rule, No-cycles, Ports & adapters; Code → Size & complexity, Duplication.
- mermaid/UML renderer from the graph.
- CLI entry: `circuit analyze [path]` → prints indicators + writes a mermaid diagram.

**Parallel-eligible:** each analyzer (dependency rule vs cycles vs complexity) is an independent node once the graph model exists; the renderer parallels the analyzers.

**Exit criteria:** `circuit analyze` on Circuit's own repo emits correct dependency-rule + cycle findings and a valid mermaid diagram; every indicator has unit tests with fixture repos; adding a second language is a matter of implementing the `lang` trait (proven by a stub second adapter test).

## M2 — Session model + flow + git

**Goal:** The workflow shell — two-tier sessions, the flow track, git/PR integration, and the authored data model on disk.

**Components:**
- `.circuit/` authored data model: spec, task DAG, glossary, tier/capability config (serde schemas, diff-friendly).
- Session domain: spec session, implementation session, fix session; lifecycle spine.
- Flow surfaces: per-session rail + spec-level DAG board.
- git/forge adapter: branch/PR state, automation actions (create PR, merge, update-from-base).
- Local strategy: checkpoint snapshots as synthetic PRs when no remote.

**Depends on:** M1 (graph + indicators feed the session cockpit roll-up).

**Parallel-eligible:** `.circuit/` schema, git adapter, and the DAG-board renderer are independent once session domain types exist.

**Exit criteria:** create a spec session, decompose into a DAG, spawn implementation sessions, see lifecycle + git status drive each session's flow, with all authored state committed under `.circuit/` and round-tripped.

## M3 — Projection engine

**Goal:** Design-before-code — the steering surface at both altitudes plus the conformance check that closes the loop.

**Components:**
- System-level projection (spec session): architecture + context map + inter-slice contracts.
- Slice-level projection (impl session): internal design.
- Projection conformance indicator (derived: code vs approved projection — violation, not elaboration).
- UI mockup + UI-match indicator (gated on `has-ui`).

**Depends on:** M2 (projections attach to sessions), M1 (conformance diffs against the graph).

**Exit criteria:** a developer approves a projection, an agent implements against it, and projection-conformance correctly distinguishes additive detail (silent) from a broken contract (red).

## M4 — Reconciliation engine

**Goal:** Keep reality and intent in sync even when changes bypass Circuit.

**Components:**
- Bypass detection: manual commits, external-tool edits, teammate pushes, merged-without-closing-node, main-ahead-of-spec, projection-no-longer-matches-code.
- Re-absorb proposals: surface model updates (new slice / projection edit / glossary extension) for developer approval.

**Depends on:** M1 (recompute reality), M2/M3 (the authored model to reconcile against).

**Exit criteria:** an out-of-band commit adding an un-specced use-case is detected and produces an approvable proposal that updates the git-tracked spec.

## M5 — Harness + ecosystem breadth

**Goal:** Plug Circuit into the agent and tooling ecosystem.

**Components:**
- Harness adapters: Claude Code, Codex, **Tau** (via its JSON-RPC serve mode).
- MCP: host (consume tools) + server (expose projection + indicators).
- Process: Superpowers skills run inside sessions.
- Spec interop: Spec Kit / OpenSpec / Kiro import-export; AGENTS.md / CLAUDE.md write-through.
- Tauri shell: the multiplatform desktop surface over the M1–M3 views.

**Depends on:** stable core surfaces (M1–M3).

**Note on the tau repo:** Circuit lives at `github.com/tau-rs/circuit`. The Tau *integration* is nonetheless an M5 adapter — sharing the org is a positioning choice, not a coupling of the core to Tau. The dependency direction stays: Tau plugs into Circuit, not the reverse.

**Exit criteria:** the same spec drives a session on each of the three harnesses; Circuit's model is visible to any agent over MCP; the Tauri shell renders the cockpit, flow, and architecture views.

---

## Sequencing rationale

Derivation+visualization (M1) is the riskiest and most defensible capability and everything downstream consumes it, so it leads and ships usable read-only immediately. Sessions+data model (M2) turn it into a workflow. Projection (M3) closes the design loop. Reconciliation (M4) hardens it against real-world drift. Ecosystem breadth (M5) is integration work best done once the core surfaces are stable, and it's where the Tauri UI and the three harness adapters land.
