// Type-level navigation check. Runs via tsc --noEmit (this file is in the
// tsconfig src glob), not via vitest — vitest's default include glob is
// *.{test,spec}.* and doesn't match *.test-d.ts. The file's purpose is to
// fail compilation if the router's navigate signature loses the shape we
// rely on.
//
// Placeholder for the assertion the dynamic-paths branch will add once
// /campaigns/$id exists:
//
//   // @ts-expect-error: id must be CampaignId, not a plain string.
//   router.navigate({ to: "/campaigns/$id", params: { id: "plain-string" } });
//
// Without that test, a regression in branded-ID enforcement at the
// navigation boundary would only surface at the API call site — exactly
// the gap the router migration is meant to close.

import type { router } from "./router";

// Smoke check that the navigate API still accepts a `to` field. If
// TanStack Router renames or removes this argument, the assignment fails.
type NavigateAcceptsTo = Parameters<typeof router.navigate>[0] extends { to?: unknown }
  ? true
  : never;

export const navigateAcceptsTo: NavigateAcceptsTo = true;
