# Testing With Full Effect: A What-If

## Context

Sibling to [2026-06-04-frontend-test-tiers-decision.md](2026-06-04-frontend-test-tiers-decision.md),
which scopes machinery minimally (pure-function-first, `effect/Micro` for the error
seam, full Effect only as a conditional top lane). This doc explores the opposite
bet: **commit the client's async spine to full Effect**, on the thesis that the
up-front machinery cost is repaid by classes of bug that then never happen, timing
and lifecycle bugs above all.

It is a deliberate "what if we commit," not a reversal of the conservative doc. The
two converge on the same first moves and diverge only at the end; that convergence
is the punchline.

Motivating pressures (real, and the reason this is worth writing down):

1. **Timing/lifecycle bugs are expensive and recurring.** Multi-day debugging of
   races that structured concurrency makes unrepresentable and `TestClock` makes
   deterministically testable.
2. **Incoming feature load is event-heavy async.** The more fallible-async-resourceful
   composition lands, the earlier Effect breaks even.
3. **`loro-websocket` is buggy and poorly maintained.** A fork is plausible. A fork
   is also the natural moment to rebuild the transport with type-safe, exhaustively
   matched state instead of endless `if/elif/else`.

Grounded in the `effect-ts` v4 skill (`joelhooks/effectts-skills` @ `0a7a0d9`) and
its `references/testing.md` (TestClock, `@effect/vitest`, test layers) and
`references/processes.md` (fork types, `Scope`, `acquireRelease`).

## What full Effect buys, mapped to our actual code

(APIs below use v4 names; the v3 equivalents are the same capabilities under prior
names, e.g. `Effect.Service` for `ServiceMap.Service`, `Schema.TaggedError` for
`Schema.TaggedErrorClass`. Nothing here depends on the version.)

**1. `LoroClientManager` as a `Scope`-managed resource graph.** Today the manager
hand-rolls a resource tree: a lazily-constructed socket, a ref-counted room map, a
debounced teardown via `setTimeout` + `closeTimer` null-checks, and a join lifecycle
that re-reads `this.room(roomId)` and re-checks `refCount <= 0` after *every* await
to clean up if the room was released mid-flight. In Effect that becomes:

- the socket is a long-lived resource in a manually-controlled `Scope` (`Scope.make`
  / `Scope.extend` / `Scope.close`, the killable-resource pattern from `processes.md`);
- each room is acquired with `acquireRelease` so "destroy the room" is a finalizer
  that runs on release, on scope close, or on interruption, by construction;
- the debounced teardown is a forked fiber that sleeps then closes the scope, and a
  reconnect simply interrupts it (no `closeTimer` bookkeeping);
- the join is structured concurrency: if the consumer releases mid-join, the join
  fiber is interrupted and its finalizer destroys the half-joined room. **The manual
  "re-check refCount after each await" pattern disappears entirely**, because
  interruption + finalizers express exactly what those checks reconstruct by hand.

**2. The timing invariants become deterministic tests.** This is the direct answer
to motivation 1. `@effect/vitest`'s `it.effect` provides a `TestContext` with a
`TestClock` starting at 0; `TestClock.adjust("100 millis")` drives the debounce with
zero real time and zero flake. The invariants currently asserted only in prose
comments (the StrictMode connect/close/connect collapse, the released-while-joining
race, the `joined` gate against premature status) become `it.effect` tests that fork
the lifecycle, adjust the clock across the debounce window, and assert exactly one
socket survived / the room was torn down / the join was interrupted. The bugs that
cost days stop being reachable, and the few that are get a deterministic harness.

**3. Typed errors across the Rust/TS seam.** Each server error variant becomes a
`Schema.TaggedErrorClass`; recovery is `catchTag`/`catchTags` (exhaustive, the
compiler checks you handled each); errors ride the `E` channel instead of being
thrown. This mirrors the Rust `Result<X, Y>` directly and is parse-don't-validate at
the boundary via `Schema`.

**4. A forked `loro-websocket` rebuilt as an Effect service (the keystone).** If the
fork happens, the transport becomes a `ServiceMap.Service` with a typed interface;
protocol messages become `Schema.TaggedClass` variants decoded/encoded by `Schema`;
the connection state becomes a `Schema.Union` matched with `Match.valueTags`, which
is a *total* function the compiler rejects if you miss a case. The "endless
if/elif/else" status soup that motivation 3 names is exactly the bug class this
erases. The fork decision and the Effect decision are, for the transport, the same
decision.

## Worked example: the join-identity bug

`joinRoom` (in `apps/web/src/features/editor/loro-manager.ts`) is the thesis in
miniature, in code that already exists. It builds a `LoroAdaptor` bound to
`wanted.doc`, awaits `waitConnected()` and `waitForReachingServerVersion()`, then
re-fetches `current = this.room(roomId)` and binds `current.room = room`. After the
awaits it re-checks `current.refCount <= 0` but not `current === wanted`. If a socket
teardown cleared the map and a re-acquire built a fresh handle (new `doc`) during
those awaits, the late-resolving join binds a room glued to the *old* doc onto the
*new* handle, and that handle renders its empty doc forever. (Narrow: it needs a real
`teardown()` overlapping an in-flight initial join, since a same-handle re-acquire
only bumps `refCount`. Latent, but a real gap.)

The tell is that the `refCount` re-check is already there. Someone correctly saw that
an await is a re-entrancy point and wrote a guard, then wrote the wrong one. That
asymmetry, right instinct and incomplete guard, is exactly the failure mode machinery
removes.

**The imperative fix is the functional move in miniature:** bind to the identity you
started with, discard if it changed.

```ts
const current = this.room<T>(roomId);
if (current !== wanted || wanted.refCount <= 0) {
  void room.destroy().catch(() => {});                       // our room is stale
  if (current === wanted && wanted.refCount <= 0) this.rooms.delete(roomId);
  return;                                                     // never touch a newer handle
}
wanted.room = room;                                          // safe: still the live owner
// ... bind wanted.doc, subscribe, snapshot from wanted ...
```

The `catch` block needs the same guard, or a late rejection from a stale join writes
`error` onto the handle that replaced it.

**The structured-concurrency version deletes the guard instead of correcting it.**
Make the room an `acquireRelease` resource scoped to the handle's lifetime, and run
the join in a fiber tied to that scope:

```ts
// sketch; verify exact APIs against the Effect source per the source-first rule
const room = yield* Effect.acquireRelease(
  joinRoom(roomId, new LoroAdaptor(doc)),   // acquire
  (room) => room.destroy(),                  // finalizer: always runs on scope close
)
yield* room.waitForReachingServerVersion()
// bind into this handle's state
```

A teardown or release now *closes the handle's scope*, which interrupts the in-flight
join fiber and runs the finalizer. A late completion binding a stale doc into a new
handle cannot occur: the fiber that would have bound it was interrupted, and the room
it held was already destroyed. There is no `current !== wanted` to forget, because
identity is enforced by the scope rather than by a comparison rewritten at every
await. "Prevent the bug" replaces "remember to check for the bug," which is the whole
case for the machinery, demonstrated by this file's own source.

## Worked example: the same fix, already shipped server-side (`persist.rs`)

The discipline this doc argues for on the client is already in the repo on the
server, in `apps/campaign/src/actors/persist.rs`, and its own comments record the bug
it killed. The naive encoding of the persistence lifecycle was the textbook flag
product: `dirty: bool` plus `persist_timer: Option<JoinHandle>`, two independent
fields whose four-state product contains two illegal states. The module doc
does not hypothesize the bad one, it reports it happening:
"dirty but nothing will ever flush it. This was a real bug: the ToC marked itself
dirty on a client edit but forgot to arm the timer." That is data loss from a single
forgotten cross-field invariant.

The shipped fix is one sum type:

```rust
enum Persist {
    Clean,
    Pending { timer: JoinHandle<()>, attempts: u32 },  // the handle lives in the variant
}
```

The `JoinHandle` lives *inside* `Pending`, so "dirty without a timer" is
unrepresentable, not guarded. `Occupancy::Vacating(JoinHandle)` in `page.rs` does
the identical move for idle eviction. Maintenance, not just safety, is the win: a flag
product forces you to keep its `2^N` implicit combinations consistent at every
mutation site, while the enum enumerates the states that actually exist and
exhaustiveness checking points at every `match` when one is added.

**This is also the key to scoping the Effect decision.** `persist.rs` gets the
illegal-states win with a *plain Rust enum and methods*, no framework. So the sum-type
half needs no Effect, and the same holds on the client: `loro-manager`'s `RoomHandle`
is this exact anti-pattern un-refactored (`joined: boolean`, `room: Room | null`,
`docUnsub: (() => void) | null`, `leaveTimer: Timer | null`, a product whose illegal
combinations include `joined === true` with `room === null`). Folding those into one
discriminated union is the `persist.rs` move in TypeScript, available today with no
dependency.

What `persist.rs` *also* has, and what separates it from the join-identity example, is
lifecycle management: `arm()` aborts the prior timer when it re-arms. That is easy
because the state machine is synchronous. The `loro-manager` join is asynchronous, and
managing a resource's lifetime across `await`s by hand is exactly where the
join-identity bug came from. So the split is clean:

- **Illegal-states-unrepresentable** (sum type): free in Rust and TS, no framework.
  Fixes `RoomHandle`'s field product. `persist.rs` is the proof.
- **Async lifecycle** (scope, interruption, finalizers): what Effect adds on top, for
  the join `await`s a plain sum type cannot manage. The join-identity example is the
  case for it.

Effect is justified by the second half, not the first. The first half is worth doing
regardless, because you have already shown server-side that it is less code and fewer
bugs.

## What it does NOT change (the honest seams)

- **The React boundary stays imperative.** `useSyncExternalStore` demands a
  *synchronous* `getSnapshot`, and an Effect is not a synchronous value. A long-lived
  manager runtime fiber materializes state into a ref that `getSnapshot` reads; Effect
  lives behind the boundary. The SPA never becomes "pure Effect" end to end. (The
  current Effect-for-React integration approach for v4 is an open item to verify at
  impl time; do not assume a specific binding library.)
- **The pure tiers remain.** Projection math and two-doc convergence are still plain
  `vitest`. Effect adds the orchestration tier; it does not replace pure-function
  testing.
- **The component/interaction tier is orthogonal.** Real-browser drag/focus testing is
  still needed and untouched by Effect.

## Costs that survive even for a believer

- **Version is a naming detail, not a strategic one.** Every capability this doc
  leans on exists in both v3 and v4; only the API names differ. Use whichever is
  stable at impl time and rename per the skill's v3/v4 table. The skill targets v4
  and marks some modules `unstable`/`@effect/vitest@beta`, so if you adopt before v4
  stabilizes, use v3 today and port the names later. Either way follow the skill's
  **source-first rule** (mirror the Effect source, verify APIs against it).
- **Bundle.** The full runtime is materially larger than Micro's ~5kb. For a client
  SPA, measure it.
- **Migration risk on the most delicate file.** Rebuilding `LoroClientManager` (already
  correct, hard to test) in a new paradigm risks reintroducing the very races its
  comments document. Mitigation is in the phasing below: write the `TestClock` tests
  against current behavior *first*, then port under green.
- **Whole-program virality + contributor ramp.** Effect is a dialect; everyone who
  touches the spine must speak it, and every non-Effect dependency needs an adapter.
- **React-seam friction.** Two schedulers meet at the boundary (above).

## How you'd actually get there (incremental, no big bang)

- **Phase 0.** Pure tiers + real-browser component runner, exactly as in the sibling
  doc. Independent of Effect; do it regardless.
- **Phase 1.** Error seam in Effect (`TaggedErrorClass` + `catchTags`), wrapping the
  `openapi-fetch` `{ data, error }`. Small, high-value, teaches the model. (If full
  Effect is the destination, do this in Effect rather than Micro to avoid a later
  Micro-to-Effect port.)
- **Phase 2.** Characterize the manager with `TestClock` tests *first*: write
  `it.effect` tests that drive the debounce and the join-race against the *current*
  implementation's observable behavior. This pays down the untested-timing-invariant
  debt immediately and de-risks the port.
- **Phase 3.** Port the manager lifecycle to `Scope`/`acquireRelease`/fork +
  interruption under those green tests. Keep the `RoomSnapshot` state machine and the
  synchronous snapshot at the React seam.
- **Phase 4.** If/when `loro-websocket` is forked, rebuild the transport as an Effect
  service with `Schema`-typed protocol messages and exhaustive `Match` over connection
  state. This is where the fork pain and the Effect payoff meet.
- **Phase 5.** Model the AI suggestion pipeline as effects-as-data; test the
  choreography via test layers (`Layer.sync` with in-memory state, asserting the
  sequence of requested effects, the `Emails.testLayer`/`sent` pattern from
  `testing.md`).

## Decision: when full Effect is the right bet

Graded against the three motivations:

- **Timing bugs: strong.** `TestClock` + structured concurrency target your most
  expensive, recurring bug class head-on. This alone is a credible justification.
- **Event-heavy async incoming: moderate-to-strong.** Breaks even earlier as
  composition deepens, but size the TS spine honestly: the heaviest orchestration
  (actors, real concurrency) already lives server-side in Rust, behind the WebSocket.
- **`loro-websocket` fork: strong and decisive if it happens.** A fork is the natural
  adoption point, and type-safe transport is precisely the bug class you are fighting.
  If you fork, adopt Effect for the transport. If you never fork, this leg weakens.

Net: the conservative doc and this one **agree on the first moves** (pure tiers,
component runner, error seam, and the `TestClock` tests, which are worth writing even
without porting). They diverge only at the manager rewrite and the transport fork. So
there is no decision to force today: do the convergent work now, and let the
`loro-websocket` fork decision be the trigger that tips you from "conditional Effect"
to "full Effect."

## Open decisions

- The Effect-for-React integration approach (verify current libraries/patterns at impl).
- Fork `loro-websocket`: yes/no and when. This is the keystone trigger.
- SPA bundle budget for the full runtime.

## References

- `effect-ts` v4 skill: `joelhooks/effectts-skills` @ `0a7a0d984033fa6d6ff4ef2b50bdd9eb06a3a6c5`
  (`skills/effect-ts/SKILL.md`, `references/testing.md`, `references/processes.md`).
- Sibling: [2026-06-04-frontend-test-tiers-decision.md](2026-06-04-frontend-test-tiers-decision.md).
