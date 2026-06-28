// Type-level proof of the i18n "no guinea pigs" guarantee. Runs via tsc --noEmit
// (this file is in the tsconfig src glob), not via vitest; vitest's default include
// glob is *.{test,spec}.* and doesn't match *.test-d.ts.
//
// Paraglide compiles every message to a typed function, so a deleted key or a
// wrong/missing param is a BUILD error at the call site, never a silent runtime
// fallback. Each @ts-expect-error below asserts the line does NOT compile; if
// Paraglide's typing ever regressed to permit these, the now-unused directive
// would itself fail the build. The accept side (correct calls compile) is proven
// by the app's real m.* call sites compiling.
//
// CI enforces this in the `ts` job: `pnpm typecheck` runs `generate:i18n`
// (paraglide compile) then tsc, so deleting messages/en.json keys reds the build.

import { m } from "../paraglide/messages.js";

// A key absent from messages/en.json is not a generated export.
// @ts-expect-error - no such message key
m.thisMessageKeyDoesNotExist();

// appError declares a {message} param; calling it with no inputs must not compile.
// @ts-expect-error - missing required `message`
m.appError();

// ...nor with the wrong param shape.
// @ts-expect-error - `message` is required; `notMessage` is not accepted
m.appError({ notMessage: "boom" });
