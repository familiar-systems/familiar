//! Negative compile tests for `#[fs_id]`. These guard the contract that
//! the brand string can never drift from the struct ident.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
