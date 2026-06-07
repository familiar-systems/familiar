# Frontend Testing Strategy

**Decided 2026-06-04.** The missing middle is built: a real-browser **component /
interaction** tier via **Storybook 10 + `@storybook/addon-vitest`**, which runs every
story as a Vitest browser-mode test (Playwright Chromium). Exemplars:
`apps/web/src/components/CampaignCard.stories.tsx` and
`apps/web/src/features/toc/TocTree.stories.tsx`. The living operational contract (how
to write and run a story test) is `apps/web/CLAUDE.md` (Testing); this doc records the
*why*.

## Context

The web client is a collaborative, drag-and-drop, rich-text editor (Notion/Logseq
class). Direct manipulation and CRDT sync *are* the product, which makes them both
the highest-value and the trickiest things to test. Before this work there were two
test tiers (pure-function `vitest`; Playwright `integration/` + `e2e/`) and a missing
middle: nothing covered "given props and a user interaction, does this component
behave." The ToC drag/create work surfaced the gap concretely (a React key-identity
bug and a drag projection that no tier could naturally assert).

This doc records the durable approach and the rationale behind the choice. Scope is
the web client and the client-side collaboration layer. The campaign server (Rust)
has its own `cargo test` story and is out of scope here.

## Principles (the stable core)

1. **Functional core, imperative/effects shell.** Push every decision that can be a
   pure function (where does a drop land, what is the new tree, is this depth legal)
   into pure functions returning *values*; keep the shell thin. `getProjection`
   (`tree-utils.ts`) is the model: the gesture is impure, the placement is pure.
2. **Confidence per unit cost.** Each test type has a price (write, maintain, run,
   flakiness) and a coverage shape. Spend where the marginal confidence is cheapest.
   **Assert edges low, assert wiring and happy-path high.**
3. **Some bugs are designed out, not tested.** Timing/coalescing races (e.g. an
   unflushed pointer-event state) are not deterministically reproducible. The fix is
   to read authoritative inputs, not to write a test that "catches" the race. Sort
   every bug into testable vs eliminate-by-construction first.
4. **Any interaction has three fidelities.** Pure intent (unit) -> component given
   synthetic events (fake or real DOM) -> real-browser gesture. A "drag test" is
   really three different tests; choose the fidelity each risk actually needs.
5. **Test collaboration without a network.** The CRDT's own convergence is the
   library's job, not ours. Our schema, binding, and invariants under concurrency
   are tested with the *two-doc trick*: two `LoroDoc`s in-process, apply concurrent
   ops, sync by hand, assert they converge to a legal tree. No socket, no browser.
6. **For the editor, assert transforms over documents, not pixels.** A command is
   `(docState) -> docState'`; test it with document fixtures and a builder DSL
   (ProseMirror ships this pattern). The contenteditable/selection/IME surface is a
   thin shell with a handful of real-browser tests.

## Tiers (mapped to this repo)

| Tier | Covers | Tool | Status |
| --- | --- | --- | --- |
| **Pure transforms** | projection/tree math, doc read/write, multi-peer convergence, editor commands, serialization compiler | `vitest` (node) | Have: `tree-utils.test.ts`, `toc-doc.test.ts`. Extend with convergence. |
| **Component / interaction** | props -> render + callbacks, key identity, keyboard a11y, focus, real drag | Storybook + `@storybook/addon-vitest` (real Chromium via `@vitest/browser-playwright`) | Have: `CampaignCard.stories.tsx`, `TocTree.stories.tsx` |
| **Orchestration** | `LoroClientManager` lifecycle/timing (StrictMode debounce, released-while-joining), future AI pipeline | Effect `TestClock` + `Layer` DI | Conditional: only if Effect is adopted |
| **Integration** | SPA behaviors: nav does no full reload, auth gate, shell persistence | Playwright, mocked REST | Have: `integration/` |
| **E2E** | real drag + real collab over the wire + SQLite persistence | Playwright, real stack | Have: `e2e/smoke.spec.ts` |

Shape: a wide pure base, a real component middle (the once-missing piece, filled), a
thin integration band, a tiny e2e cap. This is the Testing Trophy with a
CRDT-convergence lane bolted onto the base.

## Where does a test go (heuristic ladder)

Walk down until one fits:

- Expressible as `data -> data`? -> **pure unit** (push hard to keep things here).
- Needs concurrency/merge? -> **two-doc in-process** (not a browser).
- Needs the DOM but not real layout (reconciliation, keyboard, conditional render,
  callback wiring)? -> **component** (fake DOM acceptable).
- Needs real pointer/layout/focus (drag gesture, scroll, selection)? ->
  **component, real browser** (or e2e).
- Needs timing/lifecycle determinism (debounce, async interleave)? -> **Effect
  `TestClock`** if adopted, else leave to e2e/manual.
- Needs the real wire (reconnect, auth, persistence)? -> **e2e smoke, keep it tiny**.
- A non-reproducible race? -> **design it out**, do not test it.

## The component tier (the one real decision)

Because direct manipulation is the product, the component tier is
**real-browser-capable**, not jsdom: only a real browser tells you the drag gesture,
focus handoff, and scroll actually work, and `dnd-kit` fights jsdom (it reads
`getBoundingClientRect`).

**Decision: Storybook 10 + `@storybook/addon-vitest`** -- the Vitest Browser Mode
lean (real Chromium via `@vitest/browser-playwright`), in its Storybook-addon form.
The addon runs every story as a browser-mode Vitest test (a smoke render plus the
`play` function as an interaction test) and reuses the app's `vite.config.ts`, so the
`wasm()` plugin `loro-crdt` needs applies unchanged. Choosing the Storybook form over
bare Vitest Browser Mode buys the component workshop and story-as-test ergonomics for
the same Chromium cost. Config is a separate `vitest.stories.config.ts` (mirroring the
Playwright config split), out of the node-only inner loop but in `mise run check` and
folded into CI's `integration` job (both browser-only tiers share one Vite + Chromium
setup).

**Alternatives rejected:** Playwright Component Testing (real browser, but
experimental and heavier Vite/wasm config); RTL + jsdom (cheapest, but gives up the
drag/focus fidelity that is the whole reason this tier exists).

Key enabler, realized: the seams already existed. `TocTree` takes a plain `tree`
array plus callbacks and `TocRow` is presentational, so interaction tests need **no
WebSocket and no Loro mock** at the component layer (`TocTree.stories.tsx` spies
callbacks with `fn()`). Loro-backed *views* get an **in-process `LoroDoc` fixture**
injected, never a mocked socket -- see `FromLoroDoc` (builds a real doc, reads it back
via `readTocTree`) and `ReordersByKeyboard` (a real `dnd-kit` keyboard drag asserting
the `onMove` wiring). The socket is transport; the doc is the contract.

## The orchestration tier (deferred)

`LoroClientManager`'s async join lifecycle and debounced teardown carry timing
invariants that live only in prose comments, with no deterministic harness.
Whether to bring that spine under Effect (`TestClock` + structured concurrency for
deterministic timing tests) is explored in the sibling
[full-Effect what-if](2026-06-04-testing-with-full-effect-whatif.md); the trigger is a
`loro-websocket` fork, and it is not justified by the current surface. Regardless, the
`RoomSnapshot` state machine and the synchronous `useSyncExternalStore` snapshot stay
imperative at the React seam (Effect cannot *be* a synchronous snapshot).

## Not yet covered

Two small, unblocked testing items remain open:
- **Multi-peer convergence**: extend `toc-doc.test.ts` with a second `LoroDoc` and
  manual sync (the two-doc trick, principle 5); the fixtures already build docs.
- **Railway error seam**: have `useCreatePage` return a `Result` and `submitCreate`
  `match` on it instead of `try/catch` (`openapi-fetch` already returns
  `{ data, error }`, typed via the utoipa/ts-rs codegen) -- a call-site refactor.

## Cautions / non-goals

- Do not put Effect at the component layer (two runtimes fighting React's scheduler).
- Do not re-test Loro's own convergence; test the schema, binding, and invariants.
- Do not chase non-reproducible timing races with tests; design them out.
- Keep edge-case assertions in the pure tiers; e2e stays a tiny smoke or it goes
  flaky and gets ignored.
- The component tier amended the "three tiers, deliberately distinct" contract in
  `apps/web/CLAUDE.md` to "four tiers"; keep that doc and this one in sync.

## Open decisions

- **Effect adoption**: whether and when, scoped to the orchestration spine. Trigger:
  when the suggestion/AI pipeline or conflict handling makes that spine deep enough
  that typed errors + DI + `TestClock` pay for the dependency, or a `loro-websocket`
  fork forces the question. Explored in the sibling
  [full-Effect what-if](2026-06-04-testing-with-full-effect-whatif.md). Not justified
  by the current surface.
