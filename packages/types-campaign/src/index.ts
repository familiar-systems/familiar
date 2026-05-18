// Campaign-scoped IDs
export type { ThingId } from "./generated/id/ThingId";
export type { BlockId } from "./generated/id/BlockId";
export type { SessionId } from "./generated/id/SessionId";
export type { JournalId } from "./generated/id/JournalId";
export type { SuggestionId } from "./generated/id/SuggestionId";
export type { ConversationId } from "./generated/id/ConversationId";

// Document types
export type { ThingHandle } from "./generated/document/ThingHandle";
export type { TocEntry } from "./generated/document/TocEntry";
export type { TocEntryKind } from "./generated/document/TocEntryKind";

// Onboarding wire types (catalog response + initialize request).
export type { CatalogResponse } from "./generated/onboarding/CatalogResponse";
export type { SystemEntry } from "./generated/onboarding/SystemEntry";
export type { ByoEntry } from "./generated/onboarding/ByoEntry";
export type { TemplateRef } from "./generated/onboarding/TemplateRef";
export type { InitializeRequest } from "./generated/onboarding/InitializeRequest";
export type { InitializeErrorResponse } from "./generated/onboarding/InitializeErrorResponse";
export type { AudioMode } from "./generated/onboarding/AudioMode";

// `paths` describes every route the campaign server exposes. Generated
// from utoipa's OpenAPI spec; component schemas resolve back to the
// ts-rs branded types above (see tooling/openapi-codegen/generate.ts).
export type { paths as CampaignPaths } from "./openapi/campaign";
