export type { CampaignId } from "./generated/id/CampaignId";
export type { UserId } from "./generated/id/UserId";
export type { MeResponse } from "./generated/auth/MeResponse";
export type { Campaign } from "./generated/campaigns/Campaign";
export type { CreateCampaignRequest } from "./generated/campaigns/CreateCampaignRequest";
export type { CreateCampaignResponse } from "./generated/campaigns/CreateCampaignResponse";

// `paths` describes every route the platform server exposes. Generated
// from utoipa's OpenAPI spec; component schemas resolve back to the
// ts-rs branded types above (see tooling/openapi-codegen/generate.ts).
export type { paths as PlatformPaths } from "./openapi/platform";

// Runtime-validating zod schemas for branded IDs. ts-rs only emits
// compile-time types, so anything that comes in as an unverified string
// (URL params, untyped JSON, query strings) is parsed through these.
export { campaignIdSchema } from "./schemas";
