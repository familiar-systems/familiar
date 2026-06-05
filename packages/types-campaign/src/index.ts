// ts-rs generated types, re-exported via auto-generated barrels.
// Individual type files live under generated/; barrel index.ts files are
// produced by `mise run generate-types`. Never edit generated/ by hand.
export * from "./generated/id";
export * from "./generated/document";
export * from "./generated/onboarding";

// `paths` describes every route the campaign server exposes. Generated
// from utoipa's OpenAPI spec; component schemas resolve back to the
// ts-rs branded types above (see tooling/openapi-codegen/generate.ts).
export type { paths as CampaignPaths } from "./openapi/campaign";

// Runtime-validating schemas for branded IDs (hand-written, not generated).
export { pageIdSchema } from "./schemas";

// Hand-written Loro ToC schema constants (mirror the Rust source of truth; see
// the FIXME in loro/toc.ts).
export * from "./loro/toc";
