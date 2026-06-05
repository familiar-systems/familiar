// Runtime-validating zod schemas for campaign-tier branded IDs. Mirrors the
// pattern in @familiar-systems/types-app: validate a string from a URL param
// or untyped JSON and brand it into the nominal type, so consumers never cast.
//
// Convention: `<typeName>Schema` per branded ID.

import { z } from "zod";

import type { ThingId } from "./generated/id/ThingId";

/**
 * Validates a string as a ULID (26 Crockford-base32 chars) and brands it as
 * `ThingId`. The alphabet is loose (any 26 alphanumerics) to accept whatever
 * the server's ULID encoder emits; tighten later if needed.
 */
export const thingIdSchema = z
  .string()
  .regex(/^[0-9A-Za-z]{26}$/, "thing id must be a ULID")
  .transform((s) => s as ThingId);
