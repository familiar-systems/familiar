//! Prints the platform OpenAPI spec as pretty-printed JSON to stdout.
//!
//! Used by `mise run generate-openapi` to feed `openapi-typescript`. Runs
//! without any AppState, database, or environment variables — pure spec
//! emission, no I/O.

use familiar_systems_platform::openapi::api_router;

fn main() {
    let (_router, spec) = api_router().split_for_parts();
    let json = spec
        .to_pretty_json()
        .expect("OpenAPI spec must serialize to JSON");
    println!("{json}");
}
