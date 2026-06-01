//! Minimal TipTap node schema for the campaign editor: paragraphs and headings.
//!
//! Node-type names are stock TipTap (`doc`/`paragraph`/`heading`/`text`) and
//! must match the loro-prosemirror convention the campaign server mirrors in
//! `crates/campaign-shared/src/loro/prosemirror.rs`. We compose the node
//! extensions individually rather than via StarterKit so we can leave out the
//! History extension -- Loro owns undo/redo (see `loro-extension.ts`).

import type { Extensions } from "@tiptap/core";
import { Document } from "@tiptap/extension-document";
import { Heading } from "@tiptap/extension-heading";
import { Paragraph } from "@tiptap/extension-paragraph";
import { Text } from "@tiptap/extension-text";

/** ProseMirror node-type names, mirroring the server's loro-prosemirror keys. */
export const NODE_DOC = "doc";
export const NODE_PARAGRAPH = "paragraph";
export const NODE_HEADING = "heading";
export const NODE_TEXT = "text";

/** Heading levels offered in this slice (H1-H3). */
export const HEADING_LEVELS = [1, 2, 3] as const;

/**
 * The node extensions for the home-page editor. Pair with `LoroExtension` and
 * `BlockId` (from this package) when building the TipTap editor; do NOT add
 * StarterKit or History.
 */
export const NODE_EXTENSIONS: Extensions = [
  Document,
  Paragraph,
  Text,
  Heading.configure({ levels: [...HEADING_LEVELS] }),
];
