//! Cross-language drift guard for section container names.
//!
//! Each section's Loro root-container name is a contract shared by this package
//! and the Rust server: the editor binds `LoroSyncPlugin` to a container by name
//! (`preambleContainerId` / `bodyContainerId`), and the server reads/writes the
//! same-named container plus the matching `blocks.section` value. There is no
//! generated-constant pipeline between them (the strings are mirrored by hand),
//! so this test pins the literals. The Rust side pins the same literals in
//! `crates/campaign-shared/src/page_kind.rs`
//! (`entity_and_template_share_preamble_body_layout`) and
//! `loro::page::{CONTAINER_PREAMBLE, CONTAINER_BODY}`. If either side renames a
//! section without the other, one of the two tests fails loudly.

import { describe, expect, it } from "vitest";

import { BODY_CONTAINER, PREAMBLE_CONTAINER } from "./loro-extension";

describe("section container name contract (must match campaign-shared)", () => {
  it("preamble and body container names are pinned", () => {
    // Mirror of CONTAINER_PREAMBLE / CONTAINER_BODY in campaign-shared.
    expect(PREAMBLE_CONTAINER).toBe("preamble");
    expect(BODY_CONTAINER).toBe("body");
  });
});
