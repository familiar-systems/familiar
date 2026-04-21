import { z } from "zod";

// Runtime validators for platform API responses. The ts-rs-generated types
// in @familiar-systems/types-app are compile-time-only; these schemas are
// the system-boundary check at the fetch/response boundary, per CLAUDE.md
// ("Zod at system boundaries"). A parsed value is assignable to its
// ts-rs-generated counterpart via a cast — the `UserId` brand is nominal
// and vanishes at runtime, so validating `id` as a uuid-shaped string is
// equivalent to the Rust-side invariant.
export const MeResponseSchema = z.object({
  id: z.uuid(),
  email: z.email(),
});
