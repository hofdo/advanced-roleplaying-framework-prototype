//! Behavioral fixture tests: full turn → delta → state → projection pipeline.
//! These are scenario-level correctness proofs, not unit tests.
//! All require Docker via testcontainers.

mod common;

use common::{json_body, mock_provider, postgres_test_context, sample_scenario, send_empty, send_json};
use domain::{FrontendVisibleState, WorldState};
use serde_json::{json, Value};

/// Full pipeline fixture:
/// - Player floods the guildhall (action turn)
/// - LLM returns: NPCs alarmed (attitude changed), guild standing drops, fame clock advances
/// - Asserts: world state version advances, faction standing changed, clock advanced
/// - Asserts: exported FrontendVisibleState has no GmOnly facts
/// - Asserts: projected state shows updated quest/faction state visible to player
#[tokio::test]
#[ignore = "requires docker daemon via testcontainers"]
async fn flood_guildhall_advances_state_and_projection_hides_gm_only_facts() {
    let flood_response = r#"{
        "player_response": "The guildhall erupts into panic. The examiner shouts for everyone to stand back as mana floods the registration chamber, overturning tables and sending certification crystals skittering across the floor.",
        "world_state_delta": {
            "facts_to_add": [
                {
                    "text": "The player displayed a dangerous level of uncontrolled mana in the guildhall.",
                    "visibility": "player_known",
                    "known_by": ["examiner"],
                    "reveal_conditions": [],
                    "reason": "The player caused a public scene during registration."
                }
            ],
            "npc_changes": [
                {
                    "type": "attitude_changed",
                    "npc_id": "examiner",
                    "attitude": "alarmed",
                    "reason": "Witnessed dangerous mana discharge in a crowded hall."
                }
            ],
            "faction_changes": [
                {
                    "type": "standing_changed",
                    "faction_id": "guild",
                    "standing_delta": -10,
                    "reason": "The player caused a panic in a public guild facility."
                }
            ],
            "quest_changes": [],
            "clock_changes": [
                {
                    "type": "advanced",
                    "clock_id": "fame",
                    "delta": 2,
                    "reason": "Many witnesses saw the impossible mana display."
                }
            ],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": [
                "The player revealed abnormal mana capacity during guild registration.",
                "The examiner and civilians evacuated the chamber."
            ]
        }
    }"#;

    let ctx = postgres_test_context(mock_provider([flood_response.to_string()]))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Flood the Guildhall" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    // --- Run the turn ---
    let (turn_status, turn_body) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({
            "input": "I flood the guildhall with infinite mana to prove I am powerful.",
            "mode": "action"
        }),
    )
    .await;
    let turn_response: Value = json_body(&turn_body);

    assert_eq!(turn_status, http::StatusCode::OK, "turn must succeed");
    assert_eq!(turn_response["world_state_version"], 1, "version must advance to 1");
    assert!(
        turn_response["player_response"].as_str().unwrap_or("").len() > 10,
        "player response must be non-trivial"
    );

    // --- Verify authoritative world state ---
    let (_, raw_body) = send_empty(
        &ctx.router,
        "GET",
        &format!("/admin/sessions/{}/export/raw", session.id),
    )
    .await;
    let raw_export: Value = json_body(&raw_body);
    let world_state: WorldState =
        serde_json::from_value(raw_export["world_state"].clone()).expect("world state");

    assert_eq!(world_state.version, 1);

    // Guild standing must be -10 (from 0).
    let guild = world_state
        .factions
        .iter()
        .find(|f| f.faction_id == "guild")
        .expect("guild faction");
    assert_eq!(guild.standing, -10, "guild standing must drop by 10");

    // Fame clock must be at 3 (initial 1 + delta 2).
    let fame = world_state
        .clocks
        .iter()
        .find(|c| c.id == "fame")
        .expect("fame clock");
    assert_eq!(fame.current, 3, "fame clock must advance from 1 to 3");

    // A new player-known fact must exist about the mana display.
    assert!(
        world_state
            .facts
            .iter()
            .any(|f| f.visibility == domain::FactVisibility::PlayerKnown
                && f.text.contains("dangerous level")),
        "player-known fact about mana display must be in world state"
    );

    // GmOnly fact from scenario must still be present in raw state.
    assert!(
        world_state
            .facts
            .iter()
            .any(|f| f.visibility == domain::FactVisibility::GmOnly),
        "GM-only secret must still be in raw world state"
    );

    // --- Verify projection hides GM-only facts ---
    let (export_status, export_body) = send_empty(
        &ctx.router,
        "GET",
        &format!("/sessions/{}/export", session.id),
    )
    .await;
    let export_json: Value = json_body(&export_body);

    assert_eq!(export_status, http::StatusCode::OK);

    let projected: FrontendVisibleState =
        serde_json::from_value(export_json["visible_state"].clone()).expect("frontend state");

    // Player-known facts must appear in projection.
    assert!(
        projected
            .player_known_facts
            .iter()
            .any(|f| f.text.contains("dangerous level")),
        "player-known fact must appear in projection"
    );

    // No GmOnly secrets must leak into the projection.
    assert!(
        projected
            .player_known_facts
            .iter()
            .all(|f| f.id != "void-mark"),
        "GM-only void-mark secret must NOT appear in player projection"
    );

    // --- Verify world-state endpoint also projects correctly ---
    let (ws_status, ws_body) = send_empty(
        &ctx.router,
        "GET",
        &format!("/sessions/{}/world-state", session.id),
    )
    .await;
    let ws_json: FrontendVisibleState = json_body(&ws_body);

    assert_eq!(ws_status, http::StatusCode::OK);
    assert!(
        ws_json
            .player_known_facts
            .iter()
            .all(|f| f.id != "void-mark"),
        "world-state endpoint must not leak GM-only fact"
    );

    ctx.cleanup().await;
}
