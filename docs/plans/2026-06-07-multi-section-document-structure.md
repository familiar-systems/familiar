# Multi-Section Document Structure (Page CRDT Layout)

**Status:** Draft
**Date:** 2026-06-07
**Related:** [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md) · [Campaign Actor Domain Design](2026-05-04-campaign-actor-domain-design.md) · [Templates as Pages](2026-02-20-templates-as-pages.md) · [Glossary](../glossary.md)

> This is the **storage / CRDT structure** layer for pages. The agent-facing markdown format and retrieval tiers are owned by [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md); this doc describes how a page's LoroDoc is laid out underneath that format.

---

## Context

We are building the first page kinds with **engine-meaningful structure**: a page is no longer one undifferentiated content blob but a set of **sections**, where each section can constrain its block types and carry special rules, and the set of sections is a function of the page's `kind`.

Examples that motivated this:

- **Entity / Template**: a `preamble` (the dense "index card") plus a freeform `body`.
- **Skill**: a `description` (the routing text the agent reads to decide whether to load the skill) plus a freeform `body` of instructions.
- **Session**: GM prep, GM summary, journal, and an audio transcript, all on one page.

The driving question: do sections become multiple Loro **containers in one document**, or multiple separate Loro **documents** ("many CRDTs")? This doc records the answer and the reasoning, so a future agent does not have to re-derive it.

---

## TL;DR (the decisions)

1. **One LoroDoc per page.** Sections are **root containers inside that one doc**, not separate documents. (Loro has no Yjs-style "subdoc"; the real choice is containers-in-one-doc vs separate-docs.)
2. **A section = a named container + a block-type schema + rules.** The page `kind` declares the ordered list of sections. This is the uniform abstraction; the preamble is just "a section whose schema forbids headings," the skill description is "a bounded routing section," the body is "the permissive section."
3. **`kind` is the discriminant** (already stored in `meta.kind`). Sections are modeled as a per-kind sum type, matching the "adding a concept is adding a case" precedent (`PageKind`, `TocEntryKind`).
4. **Skill layout** = `meta` + `description` + `body`. Maps to the [Agent Skills spec](https://agentskills.io/specification); `license` / `compatibility` / `metadata` / `allowed-tools` are dropped for a campaign-internal skill.
5. **Sessions ship as 1 CRDT.** The transcript lives in the session doc as ordinary sectioned blocks for now. Splitting it into its own room later is **lossless** and deferred until session-page load actually hurts (and people complain).
6. **The permission model is unaffected.** Per-block `gm_only` render-filtering on a single doc (the existing decision) still applies; sections-as-containers keeps the single-doc property it depends on.

---

## Roadmap

Coarse sequencing. Everything in **Now / Soon / After Soon** is the *same* machine: one LoroDoc per page, sections as root containers, one shared default undo manager. **Later** is the refinement pass, the work these kinds defer, clustered because sessions motivate most of it.

- **Now: Entity + Template.** Both kinds ship `meta` + `preamble` + `body` (two real section containers). Existing page content moves into `body`; `preamble` starts empty. See *Entity / Template* below.
- **Soon: Session.** Adds the `session` kind as one LoroDoc with prep / summary / transcript / journal sections, minted together with its temporal `sessions` row; default undo, transcript in-doc. See *Session* and *Sessions = 1 CRDT* below.
- **After Soon: Skill.** Adds the `skill` kind (`meta` + `description` + `body`), a purely additive match arm mapping to the [Agent Skills spec](https://agentskills.io/specification). See *Skill* below.
- **Later: cleanup & refinement.** A per-section undo manager replacing the single shared one (see *Undo* below); vendoring the already-present `loro-prosemirror` / `loro-websocket` patches into owned forks; possibly ripping the transcript into its own room or server-authoritative data; and section-schema enforcement. The Loro-fork half is the connection-side sibling of [Testing With Full Effect](../discovery/2026-06-04-testing-with-full-effect-whatif.md).

---

## The Section Model

A **section** is:

- a **named Loro root container** (a `LoroMap` shaped as a ProseMirror document, like today's `content`),
- with a **block-type schema** (which node types it admits), and
- optional **rules** (e.g. "no headings", "length-bounded", "fixed table shape").

A **page kind** declares an ordered list of sections. Illustrative shape (names not final; ground concrete constants in the editor package and `crates/campaign-shared`):

```
match kind {
    Entity | Template => [ (preamble, no-headings), (body, permissive) ]
    Skill             => [ (description, bounded routing prose), (body, permissive) ]
    Session           => [ (prep, ...), (summary, ...), (transcript, ...), (journal, ...) ]
}
```

Three consequences:

- **Structural vs editorial sections collapse into one idea.** There is no separate "structural section" type. Everything is a section; sections differ only by schema + rules. The GM's `## Appearance` / `## History` are **headings inside the permissive `body` section**, not sections themselves.
- **Schemas are editor contract.** The per-section block-type allowlists live in `@familiar-systems/editor` (the TipTap schema both browser and server agree on). Each section binds to its container via `containerId` on `LoroSyncPlugin`; "preamble: no headings" is a PM schema without the heading node. Enforcement is cooperative (editor schema + compiler warning), not a security boundary, consistent with the threat model. (`gm_only` writes are the only thing the server hard-guards.)
- **Storage class is a property of a section, not a break in the abstraction.** Most sections are CRDT containers. A section *may* instead be backed by server-authoritative data (REST), e.g. a large worker-written transcript. Same "section with blocks and rules" interface; different backing store.

### Index-card sections (the always-loaded tier)

Two sections play the same role in different kinds:

| Kind   | Index-card section | Job      |
| ------ | ------------------ | -------- |
| Entity | `preamble`         | retrieval |
| Skill  | `description`      | routing   |

Both are bounded, dense, no-headings prose, **always loaded** as the cheap tier; the `body` loads on demand. This mirrors the [retrieval tiers](2026-03-25-ai-serialization-format-v2.md) and the Agent Skills loading model (name + description at startup, body on activation). Because the index card is its **own section**, "load every skill's routing card" is a single targeted read (`WHERE section = 'description'` across `kind = skill` pages), the same cheap fleet-wide projection shape planned for page names in `CampaignVocabulary` (a future actor, not yet built), with no per-skill doc woken up.

---

## Per-Kind Layouts

### Entity / Template

`meta` + `preamble` + `body`, and this is the **Now** work. Both kinds get the same two section containers: `preamble` is the bounded, no-headings index card; `body` is the permissive freeform section; `preamble` starts empty. This ships **greenfield**: there is no pre-existing `content` data to carry over, so nothing is migrated. (The mechanism that makes a section rename cheap in general still holds — the LoroDoc is rebuilt from the `blocks` rows on actor start, CRDT history is never the source of truth, so a rename is a row-level concern rather than CRDT-history surgery. But no `content`→`body` backfill runs, and a row whose `section` the page's kind does not declare is **dropped** on restore, not auto-renamed — see the orphan guard in `LoroPageDoc::from_blocks`. Were legacy `content` rows ever to exist, they would need an explicit `UPDATE blocks SET section='body'`.)

Template is not a separate concern here. It is already a page kind alongside Entity (`PageKind::Template`, with `template_id` lineage per [Templates as Pages](2026-02-20-templates-as-pages.md)), and it carries the identical `preamble` + `body` layout so that cloning a template yields an entity with both sections.

(The earlier "is the preamble worth its own container for Entity alone?" hedge is resolved by committing now: the two-editor split is the same machinery Session and Skill reuse, so it is paid once, here.)

### Skill

Top-level containers: **`meta`**, **`description`**, **`body`**.

Mapping to the [Agent Skills spec](https://agentskills.io/specification) (a `SKILL.md` is YAML frontmatter + a markdown body):

| Spec field      | Required | Where it goes / disposition                                                            |
| --------------- | -------- | -------------------------------------------------------------------------------------- |
| `name`          | yes      | `meta.title`                                                                            |
| `description`   | yes      | the `description` section (bounded routing prose; ≤1024 chars in the spec)              |
| body (markdown) | yes      | the `body` section (spec: "no format restrictions")                                     |
| `license`       | no       | **dropped** (for portable/shippable skills; a campaign skill lives in one campaign)     |
| `compatibility` | no       | **dropped** (runtime/environment requirements; not relevant in-campaign)                |
| `allowed-tools` | no       | **dropped** (tool access is governed by workflow role: P&R write vs Q&A read, not per-skill) |
| `metadata`      | no       | **not used** for now; if ever needed, LWW scalars in `meta`, not a container (YAGNI)    |

Visibility (`meta.status`) for a GM-authored skill is typically `gm_only` (it is agent instruction, not world content; excluded from `kind == entity` listings). The naming decision: use **`description`** (matching the shipped/global `SKILL.md` skills), not "Trigger", per the one-term-per-concept rule.

> Note: the pre-existing schema comment in `crates/campaign-shared/src/loro/page.rs` floated `"trigger"` / `"core"` as illustrative skill sections. That maps to `description` / `body`; `core` is just `body`.

### Session

Ships as **one LoroDoc** with four sections, ordered by real use: **prep** (written before play), **summary** (the post-play recap), **transcript** (the raw record), **journal** (the polished narrative, written last from the other two). Transcript sits before journal deliberately, since the journal is authored against it. A dedicated `Sources` section (the glossary groups player notes + audio there) is not modeled yet; the transcript is its own section, in-doc for now and persisted as ordinary sectioned blocks - ripping it into its own room or server-authoritative data is **Later** (see "Sessions = 1 CRDT").

Unlike Entity/Skill, a session is **two linked halves**: the `kind = session` page above and a temporal `sessions` row (`SessionId` + GM-curated `ordinal`, the durable identity relationships' `origin` / `invalidated_by` reference). They are minted **together in one genesis transaction** by the supervisor's `CreateSession` workflow (page-first, then the row, so `sessions.page_id -> pages.id` resolves; `ordinal = max + 1`). The page title is a placeholder; the canonical `Session {ordinal}[: name]` display derives from the temporal row. See [Entity Relationship Temporal Model](2026-06-23-entity-relationship-temporal-model.md).

---

## Preamble Maintenance

The preamble is the page's retrieval card (the [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md) Tier-1 index card), and it is **AI-authored by default**. It is kept faithful to the body by the same eventual-consistency pipeline that drives embeddings, not by a human curating it:

- **One pipeline for every derived-from-text artifact.** A body edit (debounced) enqueues an off-peak regeneration pass. Embeddings, the computed ToC, and page size ride this path; so does the preamble.
- **Write-back splits by authorability.** Non-authored projections (ToC, size, embeddings) write back **silently**. Authorable artifacts (the preamble, relationships) write back as **suggestions**: a proposal the GM disposes of, never a silent overwrite. Silent regeneration of an authorable artifact would clobber a GM edit and violate "AI proposes, GM disposes"; the suggestion gate is what makes the preamble covenant-safe.
- **Drift is prevented by harness, not detection.** There is no drift-detector. When the pass runs, the agent reads the **whole body plus the current preamble** and proposes a delta, so its proposal is faithful to the body by construction. Eventual consistency is acceptable: most text is stable most of the time, and changed text is close enough until the next pass lands.

Because the preamble is AI-authored, its section rules ("dense, no headings, bounded length") are **authoring guidance the agent follows**, not a schema wall. A cooperative GM may edit against them; enforcement stays cooperative, consistent with the threat model.

---

## Loro Facts That Constrain This

Verified against the Loro source (`loro`, `loro-prosemirror`, `protocol`) and docs:

- **No subdocument primitive.** Loro is not Yjs. The choice is *multiple root containers in one doc* vs *multiple docs*. Root containers are free to create ("accessing one does not produce history").
- **A room is one whole document.** The sync protocol's room type is `%LOR` = "Loro Document", and it "**does not address collection-level synchronization**" ([protocol.md](https://github.com/loro-dev/protocol/blob/main/protocol.md)). Join/version/update are all document-grained. **There is no per-container subscription.** A client that joins a page room receives the whole doc, all sections.
- **You cannot lazy-load one container of a doc.** Loro loads/syncs documents whole; shallow snapshots trim *history*, not *containers*. Lazy-loading a *part* therefore requires that part to be its own document (its own room) or to not be a CRDT at all (server data).
- **Undo is per-doc, per-peer, and injectable.** `loro-prosemirror`'s `LoroUndoPlugin` does `props.undoManager || new UndoManager(props.doc, {})`. Multiple section editors over one doc can **share one `UndoManager`** → unified page-level undo, no "undo per section" cost. (A multi-view setup wants a thin custom undo plugin because `loro-prosemirror` sets cursor-restore callbacks per plugin instance; bounded, not architectural.)
- **`LoroSyncPlugin({ doc, containerId })`** binds a ProseMirror editor to a specific root container; multiple editors can bind to different containers of the same doc. This is the mechanism that makes "sections as containers" work in the editor.

---

## Decisions & Rationale

### One LoroDoc per page; sections as containers, not separate docs

Containers buy structural separation **and** keep one snapshot, one room, one undo manager, and atomic cross-section `commit()`. Separate docs buy independent sync/persistence/eviction at the cost of: no shared cursors, no atomic cross-section edit, cross-doc peer-id reconciliation, and more rooms to join. The [Campaign Actor Domain Design](2026-05-04-campaign-actor-domain-design.md) already evaluated multi-doc-per-page (for visibility splitting) and rejected it for these reasons. Containers are the default; a separate doc is reserved for a section with an independent writer/lifecycle (see Set Aside → transcript).

### Per-kind section schema as a sum type

`kind` already lives in `meta` and is read first on restore. The section set is derived from it; restore loads `blocks WHERE section = <name>` into each container and snapshot walks the other way. The `blocks.section` column already exists, so this is mostly wiring. Adding a kind = adding a match arm; the compiler points at every site that must handle it.

### Sessions = 1 CRDT (deferral)

Following "0, 1, many": going from 1 doc to 2 is an infinitely larger step than the section split itself, so default to 1. **The split is lossless** because the durable truth is the relational `blocks` table (the LoroDoc is rebuilt from rows on every actor start, with a fresh oplog; CRDT history is never a source of truth). A future split is therefore "stop loading `section = 'transcript'` rows into the session doc, start loading them into their own room", not a data migration: same blocks, same IDs, different actor.

**Precondition for the lossless split:** keep sections distinct *now*, even inside the one doc (separate containers, distinct `section` values at rest). 1 CRDT is **not** 1 section. Blurring sections into one blob is the only thing that would forfeit the easy reversibility.

**Trigger to actually split:** session-page materialization starts to hurt (large transcripts slowing page open / inflating actor memory), and users complain. The cost scales with transcript **block granularity** (per-word diarization blocks would be the painful case; per-utterance is mild). Until then, 1.

### Undo: default manager now, per-section undo Later

**Now / Soon / After Soon** keep loro-prosemirror's default: page-level undo via one shared `UndoManager` across a kind's section editors, which is the behavior you want for Entity/Template/Skill (one stack for the whole page). The one known rough edge: with two editors on a shared manager, cursor restore is registered per editor (`LoroUndoPlugin`'s `setOnPush` / `setOnPop`), so the caret can land in the wrong section after a cross-section undo. Undo itself is correct; only caret placement is rough. This is accepted now and fixed by the **Later** per-section undo work (per-section commit origins plus a cursor router), which Session motivates.

The permission model is untouched: `gm_only` stays client-render-filtered on a single doc per the [existing decision](2026-05-04-campaign-actor-domain-design.md), and sections-as-containers preserves the single-doc property it relies on.

---

## Considered and Set Aside

What looked relevant but is not the path (kept here so it is not re-litigated):

- **Yjs-style subdocuments.** Not a Loro concept. The user's initial "break it out as a subdoc" framing imported a Yjs idea; in Loro it resolves to containers-in-one-doc.
- **"Many CRDTs", one doc per section/subpage.** Rejected as a default: room = doc, so every section-doc is a separate sync/snapshot/undo unit with no shared cursors and no atomic cross-section commit; the actor ADR already turned this down. Reconsidered only for an independent-writer section, and even then a non-CRDT (REST) backing may beat a second CRDT room.
- **"Preamble as Implicit Position" (serialization doc) looks like a conflict, but is not.** That decision is about the **agent markdown** (no explicit `<preamble>` wrapper; position defines it). It is a serialization-layer rule, compatible with a storage-layer `preamble` container: the compiler reads the container and still emits leading prose with no wrapper. Storage container ≠ markdown wrapper.
- **Lazy-loading a single container.** Investigated to keep a big section in-doc but load it on demand. Impossible in Loro (whole-doc load/sync; shallow snapshots trim history, not containers). This is *why* a future transcript split, not an in-doc lazy load, is the lever.
- **Transcript as its own CRDT room (option B) or server-authoritative REST data (option C).** Both give lazy loading; both are deferred. Shipping option A (in-doc sectioned blocks) now, with option C noted as the likely landing if/when we split, since a transcript is bulk, worker-generated, append-mostly, and essentially never co-edited live.

---

## Current Code Grounding

(References by symbol, not line number, since lines drift.)

- **Containers & schema constants:** `CONTAINER_META` (`"meta"`), `CONTAINER_PREAMBLE` (`"preamble"`), `CONTAINER_BODY` (`"body"`), `KEY_TITLE` / `KEY_STATUS` / `KEY_KIND`, the `SECTION_PREAMBLE` / `SECTION_BODY` aliases (each equal to its container name), and `PageHandle`, all in `crates/campaign-shared/src/loro/page.rs`.
- **`PageKind`** enum (`Entity`, `Template`; future `Session` / `Skill` / `Memory`), `as_loro_str`, and **`sections()`** (the ordered per-kind section list — the one place section layout is declared), in `crates/campaign-shared/src/page_kind.rs`.
- **`LoroPageDoc`** in `apps/campaign/src/loro/page.rs`: the single constructor `from_blocks` buckets `(section, blob)` rows into the kind's declared containers and seeds each empty section with one paragraph (block ids injected via a minter); plus `extract_sections` and the immutable `kind()`. **`CrdtDoc`** trait in `apps/campaign/src/domain/crdt/doc.rs`; **`Room<D>`** in `apps/campaign/src/domain/crdt/room.rs`.
- **`PageActor`** / `PageInit` and the room id `page:<PageId>` routing in `apps/campaign/src/actors/page.rs` and `apps/campaign/src/actors/supervisor.rs` (one room per page = one room per LoroDoc).
- **Block codec:** `extract_blocks` / `restore_content` in `apps/campaign/src/loro/block_codec.rs`.
- **Schema at rest:** `blocks` table with the `section TEXT NOT NULL` column (`preamble` / `body`, seeded per the kind's sections at genesis) in `apps/campaign/src/migrations/m20260428_000002_create_blocks.rs`; `pages` table (`kind`, `template_id`) in `m20260428_000001_create_pages.rs`.
- **Existing Loro patches (files of interest):** `patches/loro-prosemirror@0.4.3.patch` (editor-init ordering; a suppressed cursor-mapping error) and `patches/loro-websocket@0.6.2.patch` (React StrictMode connect/close race; reconnect-after-teardown guard), wired in `pnpm-workspace.yaml` under `patchedDependencies`. Evidence the forks are already ours in practice; vendoring them is **Later**.

---

## Implied Documentation Updates

To keep terminology single-valued (surface, do not silently apply):

- **[AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md):**
  - "the GM can add, remove, or rename sections" should read as "add/rename **headings within the freeform body section**"; the *section list* is kind-declared.
  - `suggest_replace` gains a `reason` argument, **requires a prior full read** (the drift harness), and returns the registered *proposal* (the page is unchanged until the GM accepts; the conversation's own later reads show its pending suggestion inline).
  - The preamble is **AI-authored and maintained** via the pipeline in *Preamble Maintenance* above, alongside the `create-or-edit-preamble` skill; the markdown stays positional/no-wrapper ("Preamble as Implicit Position" is unaffected).
  - **Visibility:** the stored default is fail closed (`gm_only`), per CLAUDE.md; the GM-facing markdown keeps marking the *hidden* (`[gm_only]` stays loud, since the serialization is a projection of computed visibility). The full model (one uniform axis at page/section/block, co-authored, per-role RAG) is owned by and now documented in that doc.
- **[Glossary](../glossary.md):** when `session` / `skill` land in code, "section" should be defined as "a kind-declared, schema-bearing container", distinct from a `##` heading.

---

## Open Questions

- **Transcript final disposition** (**Later**) if/when split: separate CRDT room (B) vs server-authoritative data (C). Leaning C.
- **At-rest representation of non-prose sections** (e.g. a future structured/fixed-table section): blocks vs dedicated columns. Sessions do not need this; flag it before the first structured section ships.
- **Section-schema enforcement** specifics: server-side structural validation, if any, beyond the editor schema. (The cooperative baseline is settled; see *Preamble Maintenance*: section rules are authoring guidance, not a hard wall.)
- **Multi-view undo cursor routing**: the caret-placement rough edge when section editors share one `UndoManager` (see *Undo* above), resolved by the **Later** per-section undo work.

---

## References

**External**
- Loro docs — concepts, containers, undo: https://loro.dev/docs
- Loro sync protocol (room = document; "does not address collection-level synchronization"): https://github.com/loro-dev/protocol/blob/main/protocol.md
- `loro-prosemirror` (`LoroSyncPlugin` `containerId`, `LoroUndoPlugin`): https://github.com/loro-dev/loro-prosemirror
- Agent Skills specification (`SKILL.md` format, startup vs activation loading): https://agentskills.io/specification

**Internal**
- [AI Serialization Format v2](2026-03-25-ai-serialization-format-v2.md) — agent markdown, retrieval tiers, preamble, suggestion model
- [Campaign Actor Domain Design](2026-05-04-campaign-actor-domain-design.md) — single-doc-per-page, permission model, multi-doc rejection
- [Templates as Pages](2026-02-20-templates-as-pages.md) — templates are pages; section structure cloning
- [Glossary](../glossary.md) — PageKind, Skill, Session sub-entities
- [Testing With Full Effect (what-if)](../discovery/2026-06-04-testing-with-full-effect-whatif.md): client async-lifecycle and the Loro-fork posture, sibling of the Later cleanup
