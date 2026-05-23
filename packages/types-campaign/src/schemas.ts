import { z } from "zod";
import type { ThingId } from "./generated/id/ThingId";

export const thingIdSchema = z
  .string()
  .min(1, "thing id required")
  .regex(/^[A-Za-z0-9_-]+$/, "thing id must be URL-safe")
  .transform((s) => s as ThingId);
