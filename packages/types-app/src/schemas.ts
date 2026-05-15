// Runtime-validating zod schemas for branded IDs.
//
// ts-rs only emits compile-time types; for parsing strings out of URL
// params or untyped JSON we need a runtime guard. These schemas brand a
// validated string into the correct nominal type so the consumer never
// has to cast.
//
// Convention: `<typeName>Schema` per branded ID. Co-located here so the
// router and any boundary parser import them from the same place.

import { z } from "zod";
import type { CampaignId } from "./generated/id/CampaignId";

/**
 * Validates a string as a Nanoid (21 URL-safe characters) and brands it
 * as `CampaignId`. The pattern is loose (`^[A-Za-z0-9_-]+$` of length 21)
 * so we accept whatever the platform's `Nanoid::new()` happens to emit
 * today; tightening the regex is a follow-up if a tighter alphabet ships.
 */
export const campaignIdSchema = z
  .string()
  .min(1, "campaign id required")
  .regex(/^[A-Za-z0-9_-]+$/, "campaign id must be URL-safe")
  .transform((s) => s as CampaignId);
