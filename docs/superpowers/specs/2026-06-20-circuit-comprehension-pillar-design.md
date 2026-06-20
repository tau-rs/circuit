# Circuit — Comprehension Pillar Design

**Status:** Draft for review
**Date:** 2026-06-20
**Type:** Product + system design for a new pillar
**Companion to:** `2026-06-14-circuit-vision-design.md`, `2026-06-14-circuit-roadmap.md`
**Mockups:** `assets/2026-06-20-comprehension/` (`app-tour.html`, `feature-surface.html`, `graph-rendering.html`)

---

## 1. One-line

A first-class **Comprehension** pillar: point Circuit at *any* existing repository — including legacy code never built through Circuit — and get *what it does* (a feature catalog mapped to code), *how it is built* (scale-aware architecture), and *where to look* (impact, entry points, glossary) — derived deterministically where possible, **named** semantically by Tau where not, cached locally, and always one click above the real code.

## 2. Problem & position

The existing vision is a **forward loop** — `Intent → Projection → Implement → Verify → Ship` — for *new* work. It half-covers the inverse, equally common need: **understanding code that already exists.** A developer onboarding onto a large/legacy codebase, or returning to one, must rebuild a mental model the tool currently can't give them. M1 derives a structural graph, but a raw module graph is not understanding — it answers neither "what can this app *do*" nor "where is feature X."

Comprehension is the **on-ramp** into the forward loop: understand the system, then reconcile/extend it via projection. It is a distinct *entry point* (comprehend-existing vs design-new) and a large standalone market (onboarding) on its own.

> **Decided (framing):** Comprehension is a **first-class pillar**, alongside the Cockpit (state/health), Flow (delivery), and the Projection loop — not merely an enrichment of M1 derivation. Rationale: different entry point, standalone value, and it is the natural feeder into reconciliation.

## 3. Research grounding

A deep literature sweep (SAR, feature location, reflexion model, program-comprehension theory, code summarization, graph tooling, visualization-at-scale, DDD decomposition, slicing, onboarding DX; 29 primary sources, 24 verified claims) drove three design-shaping facts:

1. **Automated inference is unreliable as ground truth.** Deterministic architecture recovery tops out at **MoJoFM 38–59%** (Garcia et al., ASE 2013, "surprisingly low accuracy"); textual feature location at **30–57% Top-10** (STRICT, SANER 2017). → *Never present derived architecture/features as a green light.* This **validates Circuit's determinism-honesty invariant** and extends it: semantic comprehension is explicitly advisory, confidence-scored, human-correctable.
2. **The field's answer is human-in-the-loop.** The **Software Reflexion Model** (Murphy & Notkin, FSE 1995) — human supplies a model + mapping, the tool computes agreement/divergence, iterate — is the canonical conformance technique and is *exactly* Circuit's reconciliation engine. → *Glass-box, not black-box, is the empirically necessary design, not just an ergonomic preference.* (Directly serves the "don't abstract coding away" requirement.)
3. **Scale is a solved pattern: narrow-then-spend.** Hybrid IR+FCA applies the expensive step to a *small candidate set* (100 of 80,000 methods) and caps what a human inspects at ~7–13 items; HuGMe attraction-function clustering auto-decides **>90%** of mappings. → *Anchor the costly (LLM) pass on a small candidate set, pre-cluster deterministically first, and enforce a navigability budget in the UI.*

Open frontier flagged by the research: whether LLM-era summarization raises those accuracy ceilings is **unproven** — treat Tau feature-labeling accuracy as a risk to validate, not assume.

## 4. The glass-box principle

> Every comprehension artifact is a **lens into the real code, never a replacement for it.** From any one-line summary the developer can drill to the exact function in one hop; there are no dead ends.

```
SYSTEM summary → CONTEXT → FEATURE → function summary → THE ACTUAL CODE
   (steer up here)                              (always reachable below)
```

This is how Circuit "lets the user focus on the right things" (architecture, code choices) **without abstracting coding away**: it removes the need to read *all* the code to find the few files that matter — it never hides the code. Aligns with von Mayrhauser's integrated comprehension model (experts constantly switch top-down ↔ bottom-up).

## 5. Data model — the semantic cache (a third data class)

The vision (§10) has two data classes: **Derived** (never stored, cheap to recompute) and **Authored** (stored in `.circuit/`, committed). Semantic derivation is **expensive and non-deterministic**, so it fits neither — it needs a **third class**.

| Class | Source of truth | Stored? | Example |
|---|---|---|---|
| Derived (existing) | code | no — recompute | dependency graph, cycles, complexity |
| Authored (existing) | git/human | yes — `.circuit/`, committed | spec, glossary, tier config |
| **Semantic cache (new)** | Tau, keyed to code | **yes — local, invalidatable** | feature labels, summaries, confidence |

> **Decided (cache):** semantic cache lives in **`.circuit/cache/`, `.gitignore`d, local-only** — disposable like `target/`, rebuildable from code. Every clone builds its own; no git bloat, no shared-stale-derived-data in PRs.

- **Key = content hash, incremental, symbol-granular.** Each entry is keyed on the **skeleton hash** (tree-sitter structural hash, ignoring formatting/comment-only edits) of the unit it describes. On app open: hash units, diff against index → **0 LLM calls on unchanged units**; only changed units re-run through Tau. First full index is a one-time, backgroundable, per-machine cost; day-2 is instant.
- **Promotion path.** When a human *confirms or edits* a derived label/boundary, it **promotes** out of the local cache into committed `.circuit/` authored knowledge — feeding the glossary and reconciliation. *Guess = local cache; confirmed truth = authored/shared.* (This is the reflexion-model "human refines the model" step.)

Example cache entry:
```json
{ "unit": "app/refund.rs::refund_payment", "skeleton_hash": "a1b2c3…",
  "summary": "Issues a refund against a captured payment via the PaymentPort.",
  "feature": "Refund a payment", "evidence": ["test:it_refunds_a_captured_payment","static-trace"],
  "derived_by": "tau", "confidence": 0.91, "stale": false }
```

## 6. Derivation engine

> **Load-bearing principle:** the **deterministic graph is the skeleton; Tau only labels and summarizes nodes/traces — it never discovers structure.** Keeps LLM work bounded, cacheable, incremental, and keeps the determinism invariant intact (structure = deterministic; names/summaries = semantic).

### 6.1 Feature derivation — three signal classes (Dit taxonomy)

> **Decided (feature signals = "Option C"):** combine all three of Dit et al.'s signal classes, so Tau **names evidence** rather than **guessing**:
> 1. **Static** — deterministically find entry points (HTTP routes, CLI commands, public exports, `main`, event handlers), trace each through the call graph.
> 2. **Deterministic clustering pre-pass** (HuGMe-style attraction) — group tightly-connected code before naming; repairs broken/partial traces (dynamic dispatch, DI) and yields cleaner units → fewer, cheaper LLM calls.
> 3. **Opt-in dynamic (test-trace) seeding** — where a test suite exists, run it once with coverage; each test → the exact code it executed = high-precision feature member-set, and the test's name = a strong label hint. **Falls back to clustering-only on test-less repos.**

Tau then assigns each candidate group a feature name + summary + **confidence**, surfaced with its **evidence type** (test-proven > trace > text-only). Gated: dynamic seeding requires a sandbox and a "don't run untrusted code blindly" guardrail.

### 6.2 Architecture comprehension — scale-aware roll-up

Bottom-up hierarchical summarization over the same graph: `symbol → module → bounded-context → system`. **Hierarchy depth = the scale knob** — the mechanism is identical for a 5-file script and a 2,000-file monorepo; only the depth shown changes.

| | Simple repo | Large repo |
|---|---|---|
| Architecture | **flat** — one summary, single-level; *don't force hex layers* (tier-aware, vision §5) | **hierarchical** — show top 1–2 levels (system→contexts); drill to expand; never summarize the whole at once |
| Features | one per entry point | grouped by context; lazy-derive a context's detail on open |

Boundaries are proposed by clustering, named by Tau, and are **advisory** (the 38–59% honesty) — renamable/splittable/mergeable; confirmed boundaries promote to authored. This is the reflexion model operating on the architecture altitude.

## 7. Surface & interaction model

> **Decided (surface):** **bound feature-catalog ⇄ architecture-map ⇄ drill-to-code** (mockup option 3). Two views of one model: pick a feature → its path lights on the map; pick a map region → it filters the catalog; from either, one hop lands in the real file. This is the literal implementation of top-down ↔ bottom-up switching and serves the feature-list *and* architecture asks from one model.

App views (left nav — see `app-tour.html`):

1. **System overview** *(landing)* — Tau's plain-language summary of what the app is, headline stats, **local-index status** (cached vs re-deriving, 0-LLM-on-unchanged), entry-point list, extracted glossary.
2. **Contexts & architecture** — bounded-context roll-up (the scale knob), with the advisory-boundaries honesty callout.
3. **Features ↔ code** — the bound catalog/map/code surface.
4. **Impact / blast-radius** — the *reverse* lookup: pick a unit → which features break, which tests cover it, the architectural ripple. Serves "decide a change with consequences in view."
5. **At monorepo scale** — same surface, big numbers, collapsed contexts + search/drill, navigability budget enforced.

> **Decided (landing):** **System overview** is the default landing screen.

## 8. Artifacts

| Artifact | Answers | Status |
|---|---|---|
| Feature catalog | "what does it *do*?" | core |
| Architecture map (zoom) | "how is it built?" | core |
| Feature trace | "which files implement *this*?" | core |
| Impact / blast-radius | "what breaks if I change X?" | core |
| Entry-point index | "all the ways in?" | core (deterministic, cheap) |
| Glossary (ubiquitous language) | "what do the domain words mean?" | core (promotes to authored) |
| Guided tour / reading order | "where do I start?" | **deferred (post-v1)** |

## 9. Visual language

> **Decided (rendering):** **layered interactive graph** (mockup option B) as the architecture *and* feature-path renderer: columns = hex layers, arrows point inward to the domain core (the picture teaches the dependency rule), with zoom/pan, hover, and click-to-light a feature's path. Built on a dagre/ELK layout engine, styled and animated by Circuit. The **trace ribbon** (option D) is the single-feature view (a subway line through the layers). **Mermaid is kept only as an export format** (for docs/PRs), never the daily surface — it is static, tangles past ~20 nodes, and cannot do live highlighting. Force-directed (option C) is an optional "what-clusters-with-what" lens, not the default.

## 10. Tau integration — a minimal read-only derivation adapter

Comprehension needs a **far thinner** Tau integration than the M5 harness: a **stateless, read-only "label this trace / summarize this unit" call** (assisted by Tau's tree-sitter `get_file_skeleton`/`get_function` tools), **not** a managed coding session. This adapter is decoupled from full session orchestration and can land earlier. Dependency direction is preserved: Tau plugs into Circuit.

## 11. Sequencing

The pillar splits cleanly: almost everything is **deterministic (no LLM)**; only naming/summarizing needs Tau.

> **Decided (sequencing = split across two milestones):**
>
> - **Comprehension-structural** — ships as an **M1.5 extension**, pure deterministic, **zero LLM**, usable read-only on any repo immediately (matches M1's ethos and the determinism invariant). Delivers: entry-point index, call-trace feature *groups* (unnamed/heuristic), layered graph B, impact / blast-radius.
> - **Comprehension-semantic** — its own milestone via the **minimal read-only Tau-for-derivation adapter** (§10), landing **before** the heavy M5 session orchestration. Delivers: Tau names the groups, writes summaries, extracts glossary, computes confidence, fills the local cache; promotion-on-confirm.

**MVP cut:**
- **MVP-A** (deterministic): layered graph B + entry-point index + call-trace groups + impact view. No LLM cost.
- **MVP-B** (semantic): Tau names groups, summaries, glossary, confidence, local cache.

## 12. Non-goals (YAGNI)

- No committed/shared semantic cache in v1 (local-only; revisit only if teammate-shared *raw* guesses prove necessary).
- No green light / health color on any comprehension artifact — it is navigational, not a health signal (would violate the determinism invariant).
- No guided-tour / reading-order generation in v1.
- No code-city / 3D / polymetric visualization in v1 (force-directed is the only optional secondary lens).
- Comprehension does not orchestrate Tau as a coding session — read-only derivation calls only.

## 13. Open questions & risks

1. **Tau labeling accuracy is unproven** (research open frontier). Mitigation: confidence + evidence-type surfaced; test-proven seeds preferred; everything human-correctable. Needs an early accuracy spike on a real repo.
2. **Skeleton-hash invalidation** — exact hash boundary (what counts as a meaning-bearing change) and roll-up cache invalidation up the hierarchy.
3. **Dynamic seeding safety** — sandbox model for running an arbitrary repo's test suite; opt-in/consent UX.
4. **Promotion conflicts** — when a confirmed label and a later re-derivation disagree (ties into reconciliation's conflict handling, vision §15.4).
5. **Boundary stability at scale** — does clustering produce stable bounded-context boundaries across edits, or do they churn?

## 14. Exit criteria

- **M1.5 (structural):** point Circuit at its own repo and any second repo → correct entry-point index, deterministic feature groups, a layered interactive graph (B) with click-to-light feature paths, and a working impact/blast-radius view — all with **zero LLM calls** and unit-tested on fixture repos.
- **Semantic milestone:** Tau names the groups and writes summaries/glossary with surfaced confidence + evidence; re-opening the app makes **0 LLM calls when nothing changed** and re-derives only content-changed units; confirming a label promotes it into committed `.circuit/`.
