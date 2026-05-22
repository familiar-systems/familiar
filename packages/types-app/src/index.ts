// ts-rs generated types, re-exported via auto-generated barrels.
// Individual type files live under generated/; barrel index.ts files are
// produced by `mise run generate-types`. Never edit generated/ by hand.
export * from "./generated/id";
export * from "./generated/auth";
export * from "./generated/campaigns";

// `paths` describes every route the platform server exposes. Generated
// from utoipa's OpenAPI spec; component schemas resolve back to the
// ts-rs branded types above (see tooling/openapi-codegen/generate.ts).
export type { paths as PlatformPaths } from "./openapi/platform";

// Runtime-validating zod schemas for branded IDs. ts-rs only emits
// compile-time types, so anything that comes in as an unverified string
// (URL params, untyped JSON, query strings) is parsed through these.
export { campaignIdSchema } from "./schemas";
