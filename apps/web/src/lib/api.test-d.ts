// Type-level guards. No runtime code, no test runner — TypeScript itself
// checks these on every `tsc --noEmit` (run via `mise run typecheck`).
//
// What we're protecting: openapi-fetch's mapped-type expansion turns ts-rs
// branded aliases (`string & { __brand: "X" }`) into an object-typed
// lookalike on the response side. The brand *property* survives that
// expansion, which is what keeps a `Me["id"]` from silently accepting an
// unrelated `Campaign["id"]` even before we cast back to the alias form
// at the SPA boundary in home.tsx. If a future openapi-fetch update
// changed its internal types in a way that erased the property, the
// runtime cross-wire protection would quietly disappear. This file
// catches that at compile time and forces a rethink of the boundary.

import type { MethodResponse } from "openapi-fetch";
import { client } from "./api";

type AssertExtends<T extends true> = T;

// TODO: extend this file with one AssertExtends block per branded response
// type as more endpoints land.
export type MeIdKeepsBrand = AssertExtends<
  MethodResponse<typeof client, "get", "/me">["id"] extends {
    readonly __brand: "UserId";
  }
    ? true
    : false
>;
