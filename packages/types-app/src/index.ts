export type { CampaignId } from "./generated/id/CampaignId";
export type { UserId } from "./generated/id/UserId";
export type { MeResponse } from "./generated/auth/MeResponse";

// `paths` describes every route the platform server exposes. Generated
// from utoipa's OpenAPI spec; component schemas resolve back to the
// ts-rs branded types above (see tooling/openapi-codegen/generate.ts).
export type { paths as PlatformPaths } from "./openapi/platform";
