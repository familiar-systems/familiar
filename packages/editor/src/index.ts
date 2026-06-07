//! Public API of @familiar-systems/editor: the shared TipTap schema and the
//! Loro binding for the campaign editor. The transport (WebSocket sync) and the
//! React editor component live in the consuming app (apps/web).

export {
  HEADING_LEVELS,
  NODE_DOC,
  NODE_EXTENSIONS,
  NODE_HEADING,
  NODE_PARAGRAPH,
  NODE_TEXT,
} from "./schema";
export { BLOCK_ID_ATTR, BlockId } from "./block-id";
export { META_CONTAINER, META_TITLE_KEY, readPageTitle, writePageTitle } from "./meta";
export {
  CONTENT_CONTAINER,
  contentContainerId,
  LoroExtension,
  type LoroExtensionOptions,
} from "./loro-extension";
