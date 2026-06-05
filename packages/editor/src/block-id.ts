//! Stable per-block identity via TipTap's UniqueID extension.
//!
//! Each paragraph/heading gets a ULID `blockId` attribute, generated on the
//! client as the user creates blocks. The id rides through Loro into the
//! persisted content; the campaign server reads it back from a node's
//! `attributes.blockId` and uses it as the durable `blocks.id`, so a block
//! keeps its identity across edits and reloads (instead of the server minting a
//! fresh id on every persist).
//!
//! `@tiptap/extension-unique-id` is MIT-licensed (formerly a Pro extension).
//! It regenerates duplicate ids on copy/paste and block splits.

import type { Transaction } from "@tiptap/pm/state";
import { UniqueID } from "@tiptap/extension-unique-id";
import { loroSyncPluginKey } from "loro-prosemirror";
import { ulid } from "ulidx";

import { NODE_HEADING, NODE_PARAGRAPH } from "./schema";

/**
 * The node attribute holding a block's stable ULID. Must match `ATTR_BLOCK_ID`
 * ("blockId") in `crates/campaign-shared/src/loro/prosemirror.rs`.
 */
export const BLOCK_ID_ATTR = "blockId";

/**
 * UniqueID configured to stamp ULIDs onto paragraphs and headings.
 *
 * `filterTransaction` skips loro-prosemirror's remote-update transactions (it
 * tags them `{ type: "non-local-updates" }` under `loroSyncPluginKey`) so the
 * plugin never assigns ids while the CRDT is applying changes from the server
 * or other clients -- the Loro analogue of y-prosemirror's `isChangeOrigin`.
 */
export const BlockId = UniqueID.configure({
  attributeName: BLOCK_ID_ATTR,
  types: [NODE_HEADING, NODE_PARAGRAPH],
  generateID: () => ulid(),
  filterTransaction: (transaction: Transaction): boolean => {
    const meta: { readonly type?: string } | undefined = transaction.getMeta(loroSyncPluginKey);
    return meta?.type !== "non-local-updates";
  },
});
