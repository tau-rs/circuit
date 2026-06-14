# Circuit — Vision & Architecture Design

**Status:** Draft for review
**Date:** 2026-06-14
**Type:** Product vision + system design (precedes per-subsystem specs)

---

## 1. One-line

Circuit is a local-first, multiplatform, git-driven IDE that lets developers **regain control of generated code** by making a project's *intended* architecture the thing you design against, the thing agents build against, and the thing the tool continuously verifies the code back to — rendered live as UML/mermaid, instrumented by advisory indicators, and orchestrated as a two-tier session model.

## 2. Problem

Agentic coding produces code faster than a developer can keep a mental model of it. Existing tools either orchestrate parallel agents without preserving design intent (Conductor, Cursor `/multitask`), or capture a spec without orchestrating the work as traceable sessions (Kiro, Spec Kit, Tessl). Nobody closes the loop: **design intent → many parallel/sequential implementation sessions → continuous verification of code against that intent → reconciliation when reality drifts.** The result is that developers lose the thread of *what was built and whether it still matches what they meant*.

## 3. The wedge

Circuit is **projection-first**, not visualize-after-the-fact. Before an agent writes code, Circuit projects the *shape* of the solution — architecture diagram, the interfaces/contracts, a UI mockup, and a code-quality budget — and the developer steers at that level. The agent implements against the projection; Circuit verifies reality back against it. An opinionated **Hexagonal + DDD + Clean Architecture + Clean Code** model is the *ground-truth schema* that makes both the projection and the verification meaningful: a raw call-graph is noise; "this adapter is imported by the domain core, violating the dependency rule" is signal.

### Competitive position

| Tool | Spec lifecycle | Parallel sessions | Traceability spec↔work | Architecture enforcement/visualization |
|---|---|---|---|---|
| Conductor | Protocol (skill) | Yes | Weak | No |
| Cursor 3 | Plan Mode (ephemeral) | Yes (`/multitask`) | No | No |
| AWS Kiro | Native (Req→Design→Tasks) | No (single IDE) | Partial | No |
| Spec Kit / Tessl | Spec-as-source | No | Via spec files | No |
| **Circuit** | **Native, git-tracked** | **Yes (two-tier)** | **First-class** | **Core** |

Circuit owns the unclaimed intersection: *one spec session → a task DAG → many parallel/sequential implementation sessions, each traceable to the spec, each instrumented, all rendered as architecture.*

## 4. Core concepts

### 4.1 The projection-first loop

`Intent → Projection → Implement → Verify → Ship`

- **Intent** — one session per feature; tab name + auto-summary so the thread is never lost.
- **Projection** — before code: architecture diagram, port/interface contracts, UI mockup, code-quality budget. **The developer steers here.**
- **Implement** — Claude Code / Codex / Tau code against the approved projection.
- **Verify** — reality vs projection: drift, dependency-rule, quality diff.
- **Ship** — git/PR lifecycle drives session status; a local strategy substitutes when there's no git remote.

### 4.2 Two-tier session model

Spec and implementation are different kinds of work, so they are different kinds of session.

- **Spec session** (one per feature/epic) — **high-level thinking only**: intent, bounded contexts, the system-level projection (architecture, context map, inter-slice contracts), and a decomposed **task DAG**. It writes no application code.
- **Implementation sessions** (N per spec) — each executes one node of the DAG, **parallel and/or sequential**. Each runs its **own mini-loop** (its own brainstorm/detailed projection → code → verify) scoped to its slice. Detailed design lives here, not in the spec session.
- **Fix sessions** — scoped child sessions spawned from a single non-green sub-indicator, pre-loaded with just that violation as their spec.

This is fractal: spec, implementation, and fix sessions are the same object at different scopes. The **unit of an implementation session is a vertical slice / bounded context** (cohesive, ships an increment, one clean branch/PR); Circuit proposes a default DAG decomposition that the developer may edit before fan-out.

> **Recommended (fan-out granularity):** vertical slice by default, editable DAG. Rationale: atomic-task sessions create coordination overhead and noisy PRs; pure auto-decomposition removes developer agency. *Pending explicit confirmation.*

### 4.3 Three separated concerns

The single most important UI principle: never conflate these.

1. **State** — *"is the code good?"* → the **cockpit** (advisory health indicators). The artifact.
2. **Flow** — *"where is this work and what's next?"* → the **delivery track** (pipeline + git facts + automation buttons). The process. **Never wears a health color** — a healthy codebase can have an open PR; a broken one can be merged.
3. **Planning integrity** — *"does the work match the plan?"* → **flags** on the flow rail / DAG (scope-creep, traceability). Neither health nor delivery.

## 5. The opinionated model & enforcement stance

Circuit enforces Hexagonal + DDD + Clean Architecture + Clean Code — **advisory, never blocking, piloted by indicators**. Enforcement is **not** strict (blocks) and **not** silently adaptive; it is *advisory with instruments*: Circuit never stops the agent, but it makes the state of the code legible, and those indicators drive attention and session status.

- Enforcement rigor is **capability- and tier-aware**: a domain-rich app gets the full model; a CLI/script gets light layering. Tier and capabilities are declared at project init (authored config) and overridable.
- **Determinism honesty is a product invariant:** a principle gets an indicator *only if it can be computed deterministically from the repo*. Semantic principles (LSP, OCP) are surfaced as review prompts, never as a fake green light. This is the anti-nag rule applied to the tool itself.

## 6. State — the cockpit (indicators)

Three lean categories on the surface; rigor in the drill-down. A category light is the rolled-up readout (silent when green). **The sub-indicator is the unit of meaning and action**: each non-green sub-indicator carries its own fix button. **Indicators fire on a violated invariant, never on elaboration within a slice** — additive detail inside a session is expected and stays silent. Rollup is severity-weighted with debounce so mid-edit blips never flash. Each non-trivial sub-indicator carries a **"?"** popover (copy in §11).

### 6.1 Architecture — *Clean Architecture · Hexagonal · DDD boundaries*

*Structure:* Dependency rule (inward only) · Ports & adapters (domain framework-free) · **No cycles (ADP)** · **Stability & abstractness (SDP+SAP → I/A/D metrics)**
*Domain (DDD):* Context boundaries (no cross-context leak) · **Aggregate integrity** (reference by ID, one aggregate per transaction) · **Anti-corruption layer** (external types translated at the boundary)
*Intent:* **Screaming structure** (folders reveal domain, not framework) · Projection conformance (no planned contract broken)

### 6.2 Code — *Clean Code · DDD naming*

Size & complexity (fn length, cyclomatic, file size) · **Function shape** (arg count, command/query separation, side-effects) · Naming (vs ubiquitous-language glossary) · Duplication/DRY · SRP/cohesion · **ISP** (no fat interfaces) · **Law of Demeter** (no train-wreck chains)

### 6.3 Correctness — *tests · contracts*

Tests pass · Coverage Δ on touched units (flags new uncovered public surface) · Contract/API conformance (signatures match projected ports) · **UI match** (rendered vs mockup — gated on `has-ui`)

### 6.4 Plug-in health (opt-in)

Security · Performance budget · Dependency footprint. Indicators are an extension point.

### 6.5 Deliberately excluded from the lights

LSP, OCP (semantic — review prompts) · comment quality, error-handling style (heuristic — review prompts) · formatting (inline quick-fix, owned by a formatter).

### 6.6 Action model

- **Per-sub-indicator fix** → spawns a child impl session pre-loaded with that one violation as its spec, traced to the parent slice.
- **Inline quick-fix** for deterministic cases (formatter, extract-function, update-from-base) — no session spawned.

### 6.7 Two cockpit variants

- **Impl-session cockpit** — the above (one slice's code health).
- **Spec-session cockpit** — same frame, rolled up: children progress, worst-of-children health, blocked/critical-path nodes, and **spec-completeness** (are all inter-slice contracts + tasks defined before fan-out?).

## 7. Flow — the delivery track

No health colors. A pipeline + facts + actions.

- **Per-session rail** — lifecycle spine: `Draft › Project › Implement › Review › Merge › Done`, carrying task progress, branch state, PR/checks.
- **Spec-level DAG board** — every implementation session is a node showing its flow stage (where) + a small rolled-up health dot (the cockpit). This is also where the parallel/sequential fan-out lives.

> **Recommended (flow rendering):** per-session rail + spec-level DAG board. A linear pipeline fits one session; the dependency graph fits the epic and already carries the fan-out structure. Kanban-by-stage is rejected — it discards the dependency structure. *Pending explicit confirmation.*

- **Automation action bar** (repetitive ceremony → one click): Update-from-base · Create PR · Merge · Re-run checks · Spawn next slice. *The cockpit/flow surface is a control surface, not just a readout.*
- **Local strategy (no git remote):** identical six-stage spine; **local checkpoints act as synthetic PRs** (`Self-review` checkpoint snapshots, `Accepted` = developer marks done, `Archived` = snapshot frozen).

## 8. Planning integrity

Scope-creep (changes mapped to no task) and traceability (tasks done m/n) are the fit between work and plan. They surface as a **flag on the flow rail (per session) and on the DAG (per spec)** with a **⤳ Split** action that promotes stray work into its own DAG node — never as a health light, because a creeping session can be perfectly clean code. In a parallel fan-out, the silent killer is one session doing another slice's work; this is the signal that catches it.

## 9. Reconciliation engine

Detects work that **bypassed Circuit** and re-absorbs it into the authored vision:

- **Detects:** manual commits, edits from another tool, a teammate's push, a branch merged without closing its spec node, `main` drifting ahead of the spec, authored projection no longer matching code.
- **Re-absorbs:** proposes updates to the authored model — *"this commit added a `RefundPayment` use-case not in the spec; absorb as a new slice? update the projection? extend the glossary?"* — developer approves, the git-tracked spec/projection updates.

This bidirectional sync is only *possible* because of the data model (§10): state is recomputable, so reality can always be diffed against intent.

## 10. Data model — *the git repo is Circuit's database*

Two data classes:

- **Derived** — architecture graph, indicator values, UI render, drift/conformance, call/dep graph. **Never stored**; recomputed from the repo on demand. The code is the source of truth for *state*.
- **Authored** — spec (intent + slices), projection (arch · contracts · UI), task DAG, ubiquitous-language glossary, tier & capability config. **Stored as files in `.circuit/`**, committed to git, reviewable in PRs, travelling with the repo. Git is the source of truth for *intent*.

Consequence: local-first and team-shareable for free — same repo → same Circuit, no server, no separate sync database. Spec artifacts should import/export to common formats (Spec Kit / OpenSpec / Kiro) rather than invent a private one.

## 11. Indicator "?" reference copy

Principle for all popovers: **lead with the consequence the developer feels; name the theory only in the drill-down.** Format: *what it checks · why it matters · what trips it.*

- **Dependency rule** — Code should only depend inward: UI → use-cases → domain, never the reverse. Business logic must not know about the DB or web framework. *Trips when an inner layer imports an outer one.*
- **Ports & adapters** — The domain talks to the outside world only through interfaces (ports); concrete tech (Stripe, Postgres) lives in adapters. Swap tech without touching business logic. *Trips when the domain imports a concrete tool directly.*
- **No cycles (ADP)** — No two components should depend on each other in a loop (A→B→A). Cycles make code impossible to change, test, or reuse in isolation. *Trips on any dependency cycle.*
- **Stability & abstractness** — When lots of code depends on one component (a shared `PaymentPort`), every change to it ripples everywhere — so it should be an interface that almost never needs editing. Components nothing depends on can be concrete. *Warns when it's backwards: a widely-used component full of concrete logic, or an interface nobody uses.*
- **Context boundaries** — Different parts of the app (Billing vs Shipping) are separate bounded contexts with their own models; they shouldn't reach into each other's internals. *Trips when one context imports another's domain types.*
- **Aggregate integrity** — An aggregate is a cluster of objects changed together as one unit (Order + its lines). Reference other aggregates by ID, not by object, and change one per transaction. *Trips on a direct cross-aggregate object reference.*
- **Anti-corruption layer** — When you call an external system (Stripe) or another team's module, convert their data into your own types at one boundary spot. If Stripe changes, you fix one file, not fifty. *Trips when third-party types show up deep inside your domain.*
- **Screaming structure** — Your folder layout should reveal what the app *does* (checkout, billing), not which framework it uses (controllers, models). *Trips when structure is organized by tech layer instead of domain.*
- **Projection conformance** — Does the code still match the design you approved for this slice? Adding detail inside the plan is fine; breaking a planned contract is not. *Trips when a planned port/boundary is violated.*
- **Size & complexity** — Functions and files stay small and simple; complexity counts the independent paths through a function. *Trips when a unit exceeds the tier limit.*
- **Function shape** — Few arguments; a function either changes something (returns nothing) or answers something (returns a value, changes nothing) — not both. A `getUser()` that also creates a missing user surprises every caller. *Trips on too many args, or a "get" that secretly writes.*
- **Naming** — Identifiers match the project's agreed vocabulary (the glossary) and say what they mean. *Trips when names drift from the domain language.*
- **SRP / cohesion** — Each class/module has one reason to change and its parts belong together. *Trips on god-classes.*
- **ISP** — Interfaces stay small and focused so implementers don't depend on methods they never use. *Trips on a fat interface with unused methods.*
- **Law of Demeter** — Talk to your immediate collaborators, not their internals — avoid chains like `a.getB().getC().doD()`. *Trips on train-wreck call chains.*
- **Coverage Δ** — How much of the code you changed is exercised by tests; flags new public surface with no tests.
- **Contract/API** — Implemented signatures/endpoints match the interface (port) you projected. *Trips on signature mismatch.*
- **UI match** — The rendered screen matches the mockup you approved. *Trips on material visual divergence.*

*(Trivial, no "?": Duplication, Tests pass.)*

## 12. Adapter layer

Circuit hosts existing ecosystems rather than reinventing them. Four swappable adapter kinds:

- **Language/framework** — tree-sitter (multi-language parse) + ast-grep (structural rules) + LSP/SCIP (type-resolved call graphs); plus a per-framework map of how hex/DDD layers express (Spring vs NestJS vs Axum vs Django).
- **Harness** — Claude Code · Codex · **Tau** (Rust agent; integrate via its JSON-RPC serve mode; its tree-sitter `get_file_skeleton`/`get_function` tools assist derivation).
- **Process** — Superpowers skills run *inside* sessions (brainstorming, TDD, code-review).
- **Bus & interop** — MCP host (consume tools) **and** MCP server (expose projection + indicators so any agent stays aligned); Spec Kit / OpenSpec / Kiro import-export; AGENTS.md / CLAUDE.md write-through (Circuit writes its tier + glossary so harnesses inherit them); git/forge (GitHub, GitLab).

## 13. Tech stack

- **Shell:** Tauri (multiplatform, local-first, small footprint).
- **Core:** Rust (`thiserror` at boundaries, `anyhow` internally; `forbid(unsafe_code)`). Hexagonal — the derivation/indicator/reconciliation engines are the domain; tree-sitter/LSP/git/forge/harness are adapters. Circuit eats its own dogfood.
- **Diagrams:** mermaid + a structured UML model derived from the architecture graph.

## 14. Non-goals (YAGNI)

- Not a general editor replacement — Circuit orchestrates harnesses, it is not where you hand-type most code.
- No private spec format — interop with existing SDD formats.
- No server/cloud backend in v1 — git is the database.
- LSP/OCP/comment-quality detectors — excluded by the determinism invariant.
- Deployment/service-level decoupling analysis — post-v1 tier config.

## 15. Open questions

1. Architecture indicator carries 8 sub-indicators across 3 clusters — confirm the cluster grouping (Structure/Domain/Intent) reads well in the drill-down at real scale.
2. Derivation performance on large repos (LSP batch-resolution is slow) — incremental recompute strategy and cache invalidation keyed on git state.
3. Exact `.circuit/` file schema and its diff-friendliness in PR review.
4. Conflict handling when reconciliation and an active session both propose model changes.
5. Tau-rs repository: scope and name of the new repo (integration crate vs full Circuit home). **Deferred — not created until approved.**

## 16. Roadmap (summary — detailed plan follows separately)

1. **M1 — Derivation + visualization core.** Rust engine: tree-sitter/ast-grep/LSP adapters → architecture graph → live mermaid/UML. Indicator engine for Architecture + Code (deterministic subset). *Proves the hardest, most differentiating tech first; usable read-only on any repo.*
2. **M2 — Session model + flow + git.** Two-tier sessions, lifecycle rail, DAG board, git/PR adapter, automation buttons, local strategy. `.circuit/` authored data model.
3. **M3 — Projection engine.** Design-before-code surface at both altitudes; projection conformance indicator; UI mockup + match (gated).
4. **M4 — Reconciliation engine.** Bypass detection + re-absorb proposals.
5. **M5 — Harness + ecosystem breadth.** Claude Code, Codex, Tau adapters via headless/serve + MCP; Superpowers process integration; Spec Kit/OpenSpec interop.

Sequencing rationale: derivation+visualization is the riskiest and most defensible capability and everything else depends on it, so it goes first; reconciliation needs the data model and sessions to exist; multi-harness breadth is integration work best done once the core surfaces are stable.
