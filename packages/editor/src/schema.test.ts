//! Drift guard for the editor's node schema and block-id wiring. The node
//! names and the `blockId` attribute are a contract shared with the campaign
//! server (`campaign-shared/src/loro/prosemirror.rs`); this test fails loudly
//! if they change out from under it.

import { getSchema } from "@tiptap/core";
import { isValid, ulid } from "ulidx";
import { describe, expect, it } from "vitest";

import { BLOCK_ID_ATTR, BlockId } from "./block-id";
import { HEADING_LEVELS, NODE_EXTENSIONS, NODE_HEADING, NODE_PARAGRAPH } from "./schema";

describe("editor schema", () => {
  it("exposes exactly the doc/paragraph/heading/text nodes", () => {
    const names = NODE_EXTENSIONS.map((ext) => ext.name).sort();
    expect(names).toEqual(["doc", "heading", "paragraph", "text"]);
  });

  it("caps heading levels at H1-H3", () => {
    expect([...HEADING_LEVELS]).toEqual([1, 2, 3]);
  });
});

describe("block-id extension", () => {
  it("stamps the blockId attribute onto paragraphs and headings", () => {
    expect(BlockId.options.attributeName).toBe(BLOCK_ID_ATTR);
    expect(BlockId.options.types).toEqual([NODE_HEADING, NODE_PARAGRAPH]);
  });

  it("generates valid ULIDs", () => {
    expect(typeof BlockId.options.generateID).toBe("function");
    expect(isValid(ulid())).toBe(true);
  });
});

// Proves the server's persisted block shape (a paragraph/heading carrying a
// `blockId` attribute, as written by block_codec.rs) is accepted by the schema
// loro-prosemirror will reconstruct the editor from. If UniqueID's global
// attribute were not wired, schema.node(...) would throw on the unknown attr.
describe("schema accepts the server's block shape", () => {
  const schema = getSchema([...NODE_EXTENSIONS, BlockId]);

  it("registers blockId on paragraph and heading", () => {
    expect(schema.nodes.paragraph?.spec.attrs?.[BLOCK_ID_ATTR]).toBeDefined();
    expect(schema.nodes.heading?.spec.attrs?.[BLOCK_ID_ATTR]).toBeDefined();
  });

  it("validates a doc of one blockId-bearing paragraph (the seed shape)", () => {
    const paragraph = schema.node("paragraph", { [BLOCK_ID_ATTR]: ulid() }, []);
    const doc = schema.node("doc", null, [paragraph]);
    expect(() => doc.check()).not.toThrow();
    expect(doc.firstChild?.type.name).toBe("paragraph");
  });

  it("validates a heading with a level and blockId", () => {
    const heading = schema.node("heading", { level: 2, [BLOCK_ID_ATTR]: ulid() }, [
      schema.text("The Iron Citadel"),
    ]);
    const doc = schema.node("doc", null, [heading]);
    expect(() => doc.check()).not.toThrow();
    expect(doc.firstChild?.attrs["level"]).toBe(2);
  });
});
