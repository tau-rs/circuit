# Circuit — Flow / Delivery Track UX Design

**Status:** Draft for review — all 11 flows validated
**Date:** 2026-06-16
**Type:** UX / interaction design for the delivery track (vision §4.3, §7, §8)
**Companion to:** `2026-06-14-circuit-vision-design.md`, `2026-06-14-circuit-roadmap.md`, `2026-06-19-circuit-global-ui-design.md` (the shell this plugs into)
**Prototype mode:** Disposable web/HTML UX prototype (no Tauri commitment) — nail the interaction model, fake/read-only data.

---

## Purpose & scope

This spec defines the **interaction model for the Flow / delivery track** — the "where is this work and what's next?" surface (vision §4.3 concern #2). It is the centerpiece of the UX prototype. It deliberately **excludes** the cockpit/health surface (§6) except where the flow rail carries a *rolled-up* health dot, and treats Planning-integrity flags (§8) only as they appear *on* the flow rail.

**Hard invariant carried from the vision (§4.3, §7):** the flow surface **never wears a health color.** A healthy codebase can have an open PR; a broken one can be merged. Flow uses *structural / neutral* affordances; semantic red/amber/green belong to the cockpit only.

---

## Working method

We walk every delivery-track flow one at a time, validate each as a browser mockup, then append the validated interaction to this spec. Each flow section captures: **trigger → rail state → facts shown → actions available → transitions out.**

## Flow checklist (walk order)

> **Model shift (post flow 1):** the rail is goal-driven auto-advance with human gates, not a button panel. Former "action" flows (Create PR, Merge, Spawn next slice) are no longer user-driven flows — they are auto-advance / gate moments, folded into the flows below. The deviation flows are the substance.

**Remote/git mode — six-stage rail (`Draft › Design › Implement › Review › Merge › Done`)**
- [x] 1. Happy path — auto-advance through to merged & done; establishes vocabulary
- [x] 2. Gate pause — human-approval interaction; Design gate as a code-comprehension review surface (Structure/Contracts/Types/Use-cases/Internal-design/Decisions); per-item Send-back. Absorbs former "Create PR"/"Merge".
- [x] 3. Checks failing — default agent self-corrects (auto-regress Review→Implement); `Re-run checks` is flaky/infra-only escape hatch
- [x] 4. Changes requested — regress Review→Implement via review-comment channel; + two-layer Review model (auto deterministic gate / configurable curated semantic review)
- [x] 5. Behind base — auto update-from-base (not a button), just-in-time before Merge; only conflicts escalate
- [x] 6. Merge blocked — agent-first auto-resolve trivial; genuine conflict → human gate (resolve / manual / send-back-to-Design)
- [x] 7. PR closed without merge — two terminals (Done/Dropped); external close → M4 reconciliation; dependents flagged

**Local mode (no git remote) — checkpoints as synthetic PRs**
- [x] 8. Checkpoint lifecycle — same spine, tail relabelled Self-review→Accepted→Archived; Merge gate always-human

**Planning-integrity flags (on the rail, never a health color)**
- [x] 9. Scope-creep — work mapped to no task → ⚑ flag + `⤳ Split` (fourth semantic channel)
- [x] 10. Traceability — tasks done m/n chip (always-on readout) + ⚑ flag on mismatch

**Spec-level container**
- [x] 11. DAG board → graduated into the **Graph** lens of Orchestrate (per-node stage + rolled-up health dot + planning flag; dependency edges; feature clusters). Full design in `2026-06-19-circuit-global-ui-design.md` §6.

---

## Design vocabulary (locked in flow 1)

**Rail shape.** Horizontal six-stage stepper: `Draft › Design › Implement › Review › Merge › Done`. Stage 2 is **Design** (the projection / design-before-code step, vision §4.1), *not* "Project" — the label teaches the concept and avoids the noun ambiguity.

**Goal-driven state machine (core model).** Each stage owns a **deliverable / definition-of-done**. The **default path is the coding agent advancing itself** as each deliverable validates — the rail marches forward on its own. The **user only acts on the non-default path**, via *direction-changing* overrides. There is no pause/Hold control (a pause adds nothing: gates already wait for the human, and between auto stages there is nothing to hold).

**Stage deliverables (definition-of-done per stage):**
| Stage | Deliverable that advances it | Gate |
|---|---|---|
| Draft | intent captured | auto |
| Design | projection approved | **human** (the steering point — §4.1) |
| Implement | tasks complete, branch pushed, PR opened | auto |
| Review | PR approved + checks green | auto |
| Merge | merged to main | **human, configurable** (gate by default; per-project full-auto toggle) |
| Done | node closed → next slice spawns | auto |

**Gates.** Two gate kinds, marked on each stage: **auto** (agent advances on validation) and **human** (rail pauses for sign-off). Gated stages: **Design** (always) and **Merge** (configurable). All others auto.

**Overrides (non-default path only).** Direction-changing actions, not drivers: **↩ Send back** (regress to an earlier stage to redo) and **✕ Reject slice** (abandon). No Hold/pause.

**Action philosophy.** The surface is *not* a button panel. Buttons exist only for **human decisions** (approve a gate; the direction-change overrides). **System consequences of work are never buttons** — opening a PR, running CI, spawning the next slice all happen automatically and are shown as *facts/status*, not actions. (This is why "Create PR", "Re-run checks", and "Spawn next slice" are not buttons.)

**Palette — flow is color-neutral (vision §4.3 invariant).** Stage progress is conveyed by *structure*, never health color: **done** = filled slate + ✓, **current** = blue ring (◆), **upcoming** = dim outline. Semantic red/amber/green are reserved for the cockpit. The session's rolled-up health appears as a single small dot, clearly labelled "cockpit" and spatially separated from the rail.

**The one sanctioned color exception: CI checks.** The checks chip may use green/red, because "did the pipeline pass" is a **delivery fact**, distinct from "is the code well-architected" (cockpit health). This keeps the no-health-color rule about *architecture health on the rail*, not about delivery facts.

**Facts strip.** Below the rail: `tasks m/n · branch (ahead/behind/up-to-date) · PR #n + state · checks` + the separated cockpit dot. Facts are read-only status, not actions.

---

## Flows

### Flow 1 — Happy path (impl session) ✓ locked

**Trigger:** a spawned implementation session for one vertical slice.
**Rail state:** auto-advances Draft → Design → (human approves projection) → Implement → Review → (human approves merge, if gated) → Merge → Done. Shown in the prototype at the Implement moment.
**Facts shown:** tasks 7/7 · branch up-to-date · PR #14 open · checks running · cockpit healthy.
**Actions available:** none in the pure happy path — the agent drives. Override row offers ↩ Send back / ✕ Reject for the non-default path. At the two human gates (Design, Merge) the rail pauses for sign-off.
**Transitions out:** on Done, the DAG node closes and dependent slices spawn automatically (no button).

This flow established the full design vocabulary above. Every subsequent flow reuses it and only documents what *deviates*.

> **Prototype fidelity note:** all mockups are low-fidelity, capturing the *interaction model* only. Visual design (palette, spacing, typography) is deliberately unpolished and is owned by a later design/Tauri pass, not this spec.

### Flow 2 — Gate pause: the human-approval interaction ✓ locked

**Trigger:** the rail auto-advances into a human gate (**Design** always; **Merge** when gated) and stops.

**Gate-waiting visual language.** A *gate awaiting you* is visually distinct from an *agent working*: agent-busy = blue ring; gate-waiting = **purple ring + ⏸**. Purple is a dedicated "needs a human" channel, deliberately **outside the red/amber/green health palette**, so a deliberate pause never reads as a health problem. When a gate is waiting, a **gate panel** drops in below the rail carrying the decision.

**The Design gate is a code-comprehension review surface** (not a confirm dialog). Its purpose: let the developer understand the code and its abstractions *before any code exists*, then steer (vision §4.1 "the developer steers here"). It presents a **tabbed view set**:

| Tab | What it shows | Why |
|---|---|---|
| **Structure** (primary/landing) | Ports & Adapters map — concentric layers (adapters → ports → domain), dependencies pointing inward | Highest-signal view for hex/DDD: shows the *abstractions (ports)* and *dependency direction* at a glance. Preferred over a plain UML class diagram, which hides both. |
| **Contracts** | Full interface/port signatures | The literal code contracts |
| **Types** | UML-style class diagram of domain types / aggregates | The data shapes (DDD) |
| **Use-cases** | Sequence diagram(s) per key behavior | How the abstractions *collaborate* at runtime |
| **Internal design** | **Authored design patterns** the agent intends (e.g. Adapter, Strategy, Newtype) + rationale | Internal/detailed design — lives at the slice level (§4.2). Authored *intent*, not a derived indicator, so the determinism invariant does not exclude it. |
| **Decisions** | Enumerated, judgeable design choices, each with rationale + rejected alternative | What the developer steers at |
| Error model *(optional)* | Boundary error types | Matters under thiserror-at-boundaries |
| Quality budget *(optional)* | Size/complexity limits the slice commits to | Projection's code-quality budget |

**Verdict & feedback routing.** Two outcomes: **Approve & continue** (whole projection → auto-advance resumes) or **Send back** (regress to redo). **Per-item feedback feeds Send-back**: marking a single decision/contract/pattern "question" attaches it, and the agent reworks *just that item*, not the whole projection.

**Merge gate** reuses the same gate-pause pattern, but its panel is lighter — it shows the final PR/diff summary and merge readiness, not a projection.

**Boundary (flow vs M3).** The flow spec owns *that the gate pauses, presents these views, and routes the verdict*. **M3 (projection engine) owns how each view renders.** The gate's depth may graduate into the M3 projection spec; this section is the flow-side contract for it.

### Flow 3 — Checks failing ✓ locked

**Trigger:** PR open, CI red — the Review deliverable ("PR approved + checks green") is not met, so the rail cannot advance.

**Default — agent self-corrects.** A real failing check is treated as *"the Implement deliverable wasn't actually met."* The rail **regresses Review → Implement on its own**; the agent reads the CI log, pushes a fix, CI re-runs, and it auto-advances back to Review when green. **No human action required** — consistent with the goal-driven model. The rail visibly pulls back to Implement (regress indicator) and returns.

**Manual escape hatch — flaky/infra only.** `Re-run checks` exists *only* here, as the human judgment that a failure is transient (network/runner), not a real defect — re-runs CI with no code change. This is the sole survivor of the old "Re-run checks" button.

**Facts shown:** PR state + a red `checks ✗ n failing` chip (delivery-fact red is sanctioned) + which checks failed. Cockpit health is independent and unaffected.

### Review stage model — two layers (applies to flows 3, 4, 6)

The Review stage is **not** a single "human approves PR" step. It is two layers:

**Layer 1 — deterministic gate. Always automatic, not configurable.** Advances only when **cockpit health green + projection-conformance green + CI checks green**. Any red → the rail self-corrects (regress to Implement, agent fixes — flows 3/4); a human is never asked to approve a machine-verifiable defect.

**Layer 2 — semantic review. Human-in-the-loop, configurable.** Surfaces *only* the principles Circuit deliberately refuses to fake-green (LSP, OCP, business-logic correctness, edge cases — vision §5/§6.5) as a **curated review-prompt screen** ("judge these N things no indicator can verify"), not a full-diff re-read. **Default ON, lightweight.** Per-project / per-low-risk-slice toggle to **full-auto** (approve on Layer-1 green); auto when solo/no-team. A *teammate's* second human review is a separate opt-in (team/compliance), **never auto-pinged**.

Rationale: human attention is reserved for the irreplaceable judgment (design at the Design gate; semantics here), never spent rubber-stamping what the cockpit already verifies — the same principle as the goal-driven rail.

### Flow 4 — Changes requested ✓ locked

**Trigger:** a human reviewer requests changes (slice-B `ReviewState::ChangesRequested`).
**Rail behaviour:** same regress shape as flow 3 (Review → Implement, agent self-corrects) but a **distinct channel** — input is *review comments* (rendered as a thread list), uses the **amber review color** vs flow 3's CI-red, and carries its own rail sub-label ("changes requested").
**Default:** agent resolves each comment thread, pushes, then **auto re-runs Layer 1** and **re-queues the Layer-2 semantic review** per config. **Override:** ↩ Send back to Design if a comment reveals the projection itself was wrong.
**Never:** auto-ping a teammate maintainer for re-review without developer confirmation.

### Flow 5 — Behind base ✓ locked

**Trigger:** `main` advanced while the slice was in flight; the branch is behind base.
**Default — automatic.** Catching up on base is a **system consequence, not a human decision** (same class as running CI). Circuit updates-from-base, CI re-runs, the rail continues. **"Update-from-base" is not a button**; it shows as a transient fact (`branch ↓ n behind` → resolving → up to date).
**Timing:** **just-in-time before the Merge gate** (plus whenever current base is needed to recompute conformance/health) — *not* continuously on every `main` push, which would churn CI for nothing.
**Escalation:** only a **conflict** (update can't apply cleanly) stops being automatic → hands off to flow 6. Auto handles the clean majority; humans see only genuine conflicts.

### Flow 6 — Merge blocked / conflict ✓ locked

**Trigger:** the just-in-time update-from-base (flow 5) can't apply cleanly, or the PR is otherwise not mergeable.
**Behaviour — graduated, agent-first.**
1. **Agent attempts auto-resolution** of trivial/mechanical conflicts (imports, lockfiles, non-overlapping hunks). If it resolves confidently, CI re-runs and the rail continues — no human.
2. **Genuine conflict → human gate** (purple "needs you", reusing the flow-2 gate-pause language). The gate panel presents the **conflicting hunks** and the decision, with three outcomes:
   - **Let the agent resolve** with a chosen strategy (the agent proposes a resolution for approval),
   - **Resolve manually**, or
   - **↩ Send back to Design** — when the conflict reveals a *real design clash* with another slice (two slices touched the same contract), which is a planning/projection problem, not a merge mechanic.
**Rail state:** holds at the Review→Merge boundary; never advances on an unmergeable branch. Carries a neutral/structural "blocked" treatment — **not** a health color (a blocked merge says nothing about code quality).
**Cross-link:** outcome 3 connects to planning integrity (flow 9) — concurrent slices colliding is exactly the silent-killer scope problem the flags exist to catch.

### Flow 7 — PR closed without merge ✓ locked

**Trigger:** a PR is closed without merging — via the **Reject** override (flow 1), or **externally** (someone closes it on GitHub), or **superseded** by another approach.
**Terminal states are two, not one.** A slice ends in either **Done** (merged) or **Dropped** (closed, not merged). Dropped is a first-class terminal stage on the rail — *not* an error, *not* a health color (abandoning a slice says nothing about code quality).
**On Reject (developer-initiated):** rail → Dropped immediately; the DAG node is marked dropped.
**On external close (Circuit didn't do it):** this is **reconciliation territory (M4)** — Circuit detects the out-of-band close and asks *was this abandonment or superseded?*, surfacing a re-absorb proposal. Flow-side, the rail moves to Dropped and the node is flagged for the developer's confirmation.
**DAG effect:** a Dropped node that others **depend on** flags its dependents as blocked / needs-replan (cross-links to the DAG board, flow 11, and planning integrity, flow 9). A leaf Dropped node is silent.

### Flow 8 — Local mode: checkpoint lifecycle ✓ locked

**Trigger:** no git remote — Circuit's local strategy (vision §7).
**Same six-stage spine, relabelled tail.** The rail is **identical**; only the delivery facts and the Review/Merge semantics swap to local checkpoints acting as synthetic PRs:
| Remote stage | Local equivalent |
|---|---|
| Review (PR open) | **Self-review checkpoint** — a snapshot, acting like an opened PR |
| Merge (merged) | **Accepted** — developer marks the slice done |
| Done | **Archived** — snapshot frozen |
**Two-layer Review still applies.** Layer 1 (deterministic: cockpit + conformance + local test run) is still automatic. Layer 2 (semantic) is **self-review** — you are the reviewer; there is no teammate channel, so the semantic gate defaults to a lightweight self-confirmation.
**Merge gate = the Accepted action** — local mode has no auto-merge; "Accepted" is always the human mark-done. (This is the one place the Merge gate is unconditionally human.)
**Facts strip swaps:** `PR #n` → `checkpoint <id>`; `checks (CI)` → `local tests`; `branch ahead/behind` still applies if there's a local base.

### Flows 9 & 10 — Planning-integrity flags ✓ locked

Planning integrity is a **fourth semantic channel**, kept distinct from the other three: it is **neither health** (cockpit R/A/G), **nor a delivery fact** (CI green/red), **nor human-attention** (gate purple). It rides on the rail (per session) and the DAG (per spec) as a **flag (⚑)** with its own reserved hue (distinct from review-amber; exact color owned by the design pass). A creeping session can be perfectly clean code — so this must never borrow a health color (vision §8).

**Flow 9 — Scope-creep.** *Trigger:* the session's changes map to **no task** in its DAG/spec (work outside the slice's defined scope). *Surface:* a ⚑ flag on the rail + DAG node. *Action:* **⤳ Split** — promotes the stray work into its own DAG node, restoring traceability. In a parallel fan-out this is the signal that catches *one session doing another slice's work* — the silent killer (vision §8).

**Flow 10 — Traceability.** *Trigger / readout:* tasks done **m/n** (already shown as a facts-strip chip). The flag fires on a **mismatch** — a task with no corresponding work, or progress that can't be reconciled to the task list. Same ⚑ channel; the m/n chip is its always-on readout, the flag is its alarm.
