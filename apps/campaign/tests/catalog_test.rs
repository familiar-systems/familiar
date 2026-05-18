mod common;

#[tokio::test]
async fn list_systems_returns_locale_resolved_catalog() {
    let app = common::spawn_app().await;

    let resp = reqwest::get(format!("{}/catalog/systems", app.base_url))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    let systems = body["systems"].as_array().expect("systems is an array");
    assert!(
        !systems.is_empty(),
        "embedded catalog should ship at least one system"
    );

    // dnd-5e present and resolved to English by default.
    let dnd = systems
        .iter()
        .find(|s| s["id"] == "dnd-5e")
        .expect("dnd-5e in catalog");
    assert_eq!(dnd["name"], "D&D 5e (2014)");
    let bundle = dnd["bundle"].as_array().unwrap();
    assert!(!bundle.is_empty(), "dnd-5e bundle must include templates");
    let npc = bundle
        .iter()
        .find(|t| t["slug"] == "common/npc")
        .expect("common/npc in dnd-5e bundle");
    assert_eq!(npc["name"], "NPC");
    assert_eq!(npc["icon"], "person-standing");

    // BYO is a top-level sibling, not an entry in `systems`. Its UI copy
    // lives in the wizard frontend, so the wire only carries the bundle.
    assert!(
        systems.iter().all(|s| s["id"] != "freeform"),
        "freeform should not appear in systems anymore; it lives under `byo`"
    );
    let byo_bundle = body["byo"]["bundle"]
        .as_array()
        .expect("byo.bundle is an array");
    assert!(!byo_bundle.is_empty(), "byo bundle must include templates");
}

#[tokio::test]
async fn explicit_locale_query_takes_precedence_and_falls_back_for_unknown() {
    let app = common::spawn_app().await;

    let resp = reqwest::get(format!("{}/catalog/systems?locale=de", app.base_url))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    // Falls back to English content since systems.yaml only has 'en' today.
    let dnd = body["systems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "dnd-5e")
        .unwrap()
        .clone();
    assert_eq!(dnd["name"], "D&D 5e (2014)");
    // BYO ships its bundle on the locale-fallback path too.
    assert!(
        !body["byo"]["bundle"].as_array().unwrap().is_empty(),
        "byo bundle should still be present on the locale-fallback path"
    );
}
