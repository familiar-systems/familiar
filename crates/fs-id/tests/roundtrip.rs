//! Behavioural tests for `#[fs_id]`. Each inner type gets a dedicated
//! struct so we can exercise the full cross-product (string vs number,
//! auto-generated vs value-taking) without reaching into the production
//! crates.

use fs_id::{Nanoid, Ulid, Uuid, fs_id};
use ts_rs::{Config, TS};

#[fs_id]
pub struct NanoidId(pub Nanoid);

#[fs_id]
pub struct UuidId(pub Uuid);

#[fs_id]
pub struct UlidId(pub Ulid);

#[fs_id]
pub struct U64Id(pub u64);

#[fs_id]
pub struct U32Id(pub u32);

#[fs_id]
pub struct I64Id(pub i64);

#[test]
fn auto_ids_generate_distinct_values() {
    assert_ne!(NanoidId::new(), NanoidId::new());
    assert_ne!(UuidId::new(), UuidId::new());
    assert_ne!(UlidId::new(), UlidId::new());
}

#[test]
fn numeric_ids_take_value() {
    let a = U64Id::new(42);
    let b = U64Id::new(42);
    assert_eq!(a, b);
    assert_eq!(U32Id::new(7).0, 7);
    assert_eq!(I64Id::new(-1).0, -1);
}

#[test]
fn serde_roundtrip_nanoid() {
    let id = NanoidId::new();
    let json = serde_json::to_string(&id).unwrap();
    let back: NanoidId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn serde_roundtrip_uuid() {
    let id = UuidId::new();
    let json = serde_json::to_string(&id).unwrap();
    let back: UuidId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn serde_roundtrip_ulid() {
    let id = UlidId::new();
    let json = serde_json::to_string(&id).unwrap();
    let back: UlidId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn serde_roundtrip_u64() {
    let id = U64Id::new(0xDEAD_BEEF);
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "3735928559");
    let back: U64Id = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn display_matches_inner() {
    let n = Nanoid::new();
    let id = NanoidId(n.clone());
    assert_eq!(format!("{id}"), format!("{n}"));

    let v = U64Id::new(123);
    assert_eq!(format!("{v}"), "123");
}

#[test]
fn from_inner_constructs_branded() {
    let n = Nanoid::new();
    let id: NanoidId = NanoidId::from(n.clone());
    assert_eq!(id.0, n);

    let u: U64Id = U64Id::from(99);
    assert_eq!(u.0, 99);
}

#[test]
fn ts_decl_carries_brand() {
    // The brand string is the load-bearing claim of the proc macro:
    // it must contain the struct ident literally so structural-typing
    // gaps in TS can't conflate two branded IDs that happen to wrap
    // the same primitive.
    let cfg = Config::default();
    assert!(NanoidId::decl(&cfg).contains("__brand: \"NanoidId\""));
    assert!(UuidId::decl(&cfg).contains("__brand: \"UuidId\""));
    assert!(UlidId::decl(&cfg).contains("__brand: \"UlidId\""));
    assert!(U64Id::decl(&cfg).contains("__brand: \"U64Id\""));

    // String-branded vs number-branded:
    assert!(NanoidId::decl(&cfg).contains("string &"));
    assert!(U64Id::decl(&cfg).contains("number &"));
}

#[test]
fn numeric_ids_are_copy() {
    // u64-backed IDs derive Copy. A move-after-use would not compile;
    // this is the runtime witness that the derive landed.
    let a = U64Id::new(1);
    let b = a;
    let _c = a;
    assert_eq!(a, b);
}
