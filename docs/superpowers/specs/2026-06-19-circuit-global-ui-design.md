# Circuit — Global UI / UX Design

**Status:** Draft for review — distilled from an interactive mockup-driven brainstorm
**Date:** 2026-06-19
**Type:** Global UI / interaction architecture (the shell the per-surface specs plug into)
**Companion to:** `2026-06-14-circuit-vision-design.md`, `2026-06-14-circuit-roadmap.md`, `2026-06-16-circuit-flow-track-ux-design.md`
**Prototype basis:** Disposable web/HTML mockups (no Tauri commitment) — the goal was the *interaction model*, not visual polish. This spec is the durable record; a later design/Tauri pass owns final visuals.
**Milestone home:** This is the **M5 Tauri shell** pulled forward as design only. It renders the M1–M3 surfaces; nothing here changes the engine's dependency order.

---

## 1. Purpose & scope

Defines **how the whole app is organized** — the navigation tiers, the surfaces, and the cross-cutting primitives that recur at every altitude. The flow/delivery track has its own spec (flows 1–11); this spec is the container it lives in, plus the surfaces that aren't flow: navigation, generation control, architecture comprehension, orchestration of many concurrent sessions, and theming.

The app's center of gravity is **generating code and controlling that generation** — every other surface is in service of that.

---

## 2. Design principles (held throughout)

1. **Three concerns never conflated** (vision §4.3): *State* (cockpit/health), *Flow* (delivery), *Planning* (flags) are distinct visual channels. Plus a fourth, *Attention* (needs-you), and *human-gate* (purple). No channel borrows another's color.
2. **Color is reserved for state.** Flow is structural/neutral; semantic red/amber/green belong to the cockpit. Health always pairs color with a glyph/shape (WCAG 1.4.1). The one sanctioned exception: CI checks (a delivery fact). Feature/grouping is a *label*, never a color.
3. **Generation foreground, flow/health ambient, decisions interrupt.** During active work the rail is a thin spine and the cockpit is glanceable; gates take center stage only when a human must decide.
4. **Attention-first, not enumeration.** "What needs me" is an Inbox you clear, not a list you monitor.
5. **Progressive disclosure** (Shneiderman: *overview → zoom → details on demand*). Surfaces compact handles; detail opens in a peek/panel on demand.
6. **Adaptive primary/secondary, not fixed split.** Where two lenses share space, one leads per context, the other collapses to a peek.
7. **Comprehension-first.** The product's differentiator is understanding code/architecture before and as it's written; the architecture view is co-equal with the generation transcript.
8. **Buttons are human decisions only.** System consequences of work (open PR, run CI, update-from-base, spawn next) are facts/automatic, never buttons.

---

## 3. Information architecture — four tiers + cross-cutting primitives

```
Tier 0  Attention & navigation   (global, cross-project)   Inbox · ⌘K · sidebar
   │  answer an item / open a project
Tier 1  Project home             (per project)             Mission Control / "Now"
   │  go to the work
Tier 2  Orchestrate              (per project)             Plan · List · Board · Graph
   │  peek → launch / open a session
Tier 3  Session                  (one slice)               Transcript ‖ Architecture
```

**Teleports** break the strict hierarchy: an Inbox item or `⌘K` jumps straight to a Session (0→3); a gate that needs you pushes back up to the Inbox (3→0).

**Cross-cutting primitives** appear at *every* tier rather than being one-off screens: **Flow rail · Peek panel · Cockpit (health) · Feature shipping + recap · Themes/tokens.** Their consistency is what keeps the three concerns and flows 1–11 coherent across altitudes.

---

## 4. Tier 0 — Attention & navigation (global)

Replaces a Conductor-style flat "all sessions" enumeration (explicitly rejected: it created a third redundant session list and fought Circuit's project model). Instead, three primitives:

- **Inbox** — cross-project, attention-first, **clearable**: gates awaiting approval, changes-requested, planning flags, failures. Each item is actionable (e.g. *Review design →*) and carries its project + age. Filterable (All / Approvals / Reviews / Flags). This is the canonical "notify me when a session needs me" surface (cf. Linear Inbox, GitHub notifications).
- **`⌘K` command palette** — jump to any session / view / project instantly. The right tool for "swap fast," not a persistent wall of sessions.
- **Slim sidebar** — navigation + light status:
  - **Repo / project switcher** + **`+ New`** action.
  - **Inbox** (with count), **My work**.
  - **Active now, grouped by project** (a project header with health dot + count, then its running sessions) — sessions live *inside* a project, not as equals. Each session row carries **stage · ±diff · time · status**.
  - **Idle/Paused** and **Archived** sections.
  - **Settings / account** footer.

Rationale: Circuit stays project/architecture-centric for *work*; this is only a thin attention+navigation layer. Per-project Orchestrate remains the deliberate "see everything in this project."

---

## 5. Tier 1 — Project home: Mission Control / "Now"

The calm per-project landing. **Not a dashboard** — shows only what's needed now, with affordances to go deeper:

- **Direction band** — global direction at a glance: the M1→M5 roadmap with the current milestone's % and the vision line. Clickable → full roadmap.
- **"Needs you"** — one **hero** item (the single most important decision, e.g. a Design gate) with a clear primary action, plus a few compact rows. Calm empty-state when clear.
- **Recommended next move** — at concurrency scale this may be *"launch N parallel slices"*, not just "approve a gate."
- Depth on demand: status line → roadmap; rows → their session; footer → All sessions / Architecture / Orchestrate.

Earlier dense "everything" dashboards were rejected for being un-affordable; the home is a focused "Now."

---

## 6. Tier 2 — Orchestrate (per project)

Where many concurrent sessions are planned, launched, and tracked. Driven by the authored task DAG.

### 6.1 Plan (author the DAG)
Define tasks, dependencies, and parallel-vs-sequential order. **Circuit proposes a decomposition; the developer edits before fan-out** (vision §4.2). Each task carries a **`feature` tag** (see 6.4).

### 6.2 Three lenses on one task set
- **List (default)** — grouped by status (Needs-you / Running / Ready / Blocked / Done). Density + fast triage; scales to hundreds where a graph can't.
- **Board** — Kanban **grouped by status** (never by flow-stage — that discards dependency structure, vision §7). Spatial/tactile triage.
- **Graph** (renamed from "Waves") — the dependency graph: nodes + **every task-to-task edge** drawn (intra- and inter-cluster), **orthogonal (square) routing**. Structure, parallelism, critical path.

All three carry: per-task **description**, compact **blocking** (`blocks X` / `blocked by Y`), the **suggestions strip**, and **click → Peek** (§9.2).

### 6.3 Suggestions strip (the guidance engine, both directions)
A compact strip surfacing what the harness recommends: **Build** (ready slices → launch), **Review** (e.g. an amber architecture → review it), **Recap** (progress + what shipped). Guidance flows to the user (launch/approve/replan) and to each agent (its slice + scope).

### 6.4 Feature grouping (waves as feature shipping)
- A **feature** = an authored group of slices that ship together; **ships when all its slices are done+merged** (a pure rollup). One slice → one feature.
- In the **Graph**, a feature renders as a **cluster region** enclosing its nodes, with a header (name + ship pill) and a ship-status accent strip; **dependency edges cross regions freely**.
- **group-by toggle: Feature** (default — what you ship) **| Depth** (mechanical topological columns). Feature is a *separate axis* from dependency depth and is a label, not a color.

### 6.5 Scheduling & concurrency (grounded in build/orchestration practice)
- **Waves are a visualization, not an execution barrier.** Validated against LangGraph (Pregel/BSP supersteps = waves, but with a straggler-inducing barrier) vs Bazel `--jobs` / make `-j` / Airflow pools (continuous worker-pool scheduling). Circuit adopts **continuous scheduling up to a concurrency cap** — a node is ready the instant *its own* edges clear, not when its column finishes (per-edge readiness; the Graph draws the satisfied edge that unblocks a later node early).
- **Three launch modes:** **Auto** (fill to cap), **Wave** (launch a ready cohort), **Manual** (per-node, one at a time). Per-node `▶ Launch` always available.
- Coordination risk (parallel agents colliding) is handled by the `.circuit/` DAG + worktrees + the scope-creep flag — the "one session doing another's slice" guard.

---

## 7. Tier 3 — Session (generate / understand)

The work surface. **Adaptive primary/secondary**, not a fixed 50/50:

- **Transcript ‖ Architecture.** Left-center: the **agent activity transcript** (scrollable thinking + tool calls + edits + a steering input — Conductor-style) — *control the generation*. Right-center: a **live view** that tabs **Architecture · Diff · Terminal · Browser · Projection** — *understand the code*.
- **Adaptive default by rail stage:** *Implement* → transcript leads; *Design / Review / Merge gates* → architecture/projection leads. Each collapsible to a peek; manual toggle overrides.
- **Top:** the thin **flow rail** (ambient) + cockpit health chips inline (never coloring the rail).
- **Tabs (per session):** Overview (peek summary) · Session (transcript) · Architecture · Project (→ Orchestrate).

### Gate review (from the flow-track spec)
At a human gate the relevant panel takes center stage: the **Design gate** is a code-comprehension review (Structure / Contracts / Types / Use-cases / Internal-design-patterns / Decisions); the **Merge gate** shows the final diff/PR summary. Verdict routes Approve / Send-back (per-item).

---

## 8. Diagrams — mermaid

The Architecture view renders **mermaid** (vision §13; M1 already emits it): hex layers as subgraphs (Domain → Ports → Adapters), data-flow edges, the node being built highlighted, **derived live and updating as code lands**. UML-style class/sequence views (Types / Use-cases tabs) likewise derive from the graph.

---

## 9. Cross-cutting primitives

### 9.1 Flow rail
Goal-driven six-stage spine `Draft › Design › Implement › Review › Merge › Done`, gated at **Design** (always) and **Merge** (configurable). Appears as: full rail atop a Session, per-node **stage chip** on Board/Graph, a stage line in "Now". Color-neutral. *(Full behavior: flow-track spec, flows 1–11.)*

### 9.2 Peek panel
One component, opens from **any** handle (Inbox item · List row · Board card · Graph node). Two modes:
- **Forward** (ready/active): *what it will do* + **done-when** (the rail's deliverable) + **scope** (in/out → feeds scope-creep) + deps + links + **Launch**.
- **Backward** (done): *what it delivered* + shipped checklist + outcome (merged/approved/green) + numbers + links.
Disclosure: **click → side peek** (Linear pattern, keeps context). Optional hover = a tiny preview (goal + status); the substantial summary is the click panel (you may act on it — hover can't support that).

### 9.3 Cockpit (health)
Rolled-up health dot on nodes; a glanceable inspector/chips in the Session. Severity-weighted, debounced. **Never colors the flow rail** (state ≠ flow). *(Indicator detail: vision §6.)*

### 9.4 Feature shipping + recap
Features ship when their slices merge (§6.4); a **recap** of completed work — roll-up (slices · LOC · health) and per-element done-summary (via the Peek's backward mode) — is reachable at every tier.

### 9.5 Themes & tokens
**Light + dark from one semantic token set** (`--bg / --panel / --tx / --acc / health roles`, two value maps). Light leans Notion/Apple; dark leans IntelliJ New UI / Linear. Health carries glyph+color, never color alone.

---

## 10. Decisions log (notable forks resolved)

- Stage 2 named **Design** (not "Project"); "Waves" lens renamed **Graph**.
- Rail is **goal-driven auto-advance with human gates**, not a button panel; only overrides are **Send-back** / **Reject** (no Hold).
- **Review is two layers**: deterministic auto-gate (cockpit + conformance + checks) always automatic; semantic human review configurable (curated review-prompts, default-on, full-auto per low-risk slice).
- Rejected: cross-project **all-sessions cockpit** as a top tier (redundant) → replaced by **Inbox + ⌘K + slim sidebar**.
- Rejected: **Kanban-by-stage** (loses dependencies) → Board groups by **status**.
- Rejected: **barrier waves** → continuous scheduling; waves kept as view + launch gesture.

---

## 11. Open questions

1. `⌘K` scope — sessions/views/projects only, or also actions ("launch ready wave")?
2. Mission Control "recommended next move" ranking — how the harness prioritizes build vs review vs approve.
3. Merge-gate default (gated vs full-auto) per project tier.
4. Terminal/Browser panels — always available, or capability-gated (`has-ui` → Browser)?
5. Archived sessions — retention / search scope.
6. Cross-project concurrency cap — global vs per-project.

---

## 12. Relationship to the build

This is **M5 (Tauri shell)** as design-only, prototyped on the web. It depends on stable M1–M3 surfaces. Nothing here reorders the milestones; the flow-track spec (M2) and projection surfaces (M3) remain the near-term build targets, and this shell is how they're eventually presented.
