use anyhow::Result;
use domain::FrontendVisibleState;
use serde_json::Value;
use shared::ReplayFixture;

use crate::common::{
    json_body, mock_provider, send_empty, send_empty_with_bearer, send_json, turn_responses,
};
use crate::common_postgres::{postgres_test_context_with_config, send_json_with_bearer};

const ADMIN_TOKEN: &str = "test-admin-token";

fn admin_postgres_config() -> shared::AppConfig {
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Postgres;
    config.admin.enabled = true;
    config.admin.token = Some(ADMIN_TOKEN.into());
    config
}

pub async fn run_fixture(raw: &str) -> Result<()> {
    let fixture: ReplayFixture = serde_json::from_str(raw)?;
    let provider_responses = fixture
        .turns
        .iter()
        .map(|turn| turn.provider_response.to_string())
        .collect::<Vec<_>>();
    let ctx = postgres_test_context_with_config(
        mock_provider(turn_responses(provider_responses)),
        admin_postgres_config(),
    )
    .await?;

    let (scenario_status, _) = send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&fixture.scenario)?,
    )
    .await;
    anyhow::ensure!(
        scenario_status == http::StatusCode::OK,
        "scenario creation failed with status {scenario_status}"
    );

    let (session_status, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        serde_json::json!({
            "scenario_id": fixture.scenario.id,
            "title": fixture.name,
        }),
    )
    .await;
    anyhow::ensure!(
        session_status == http::StatusCode::OK,
        "session creation failed with status {session_status}"
    );
    let session: persistence::SessionRecord = json_body(&session_body);

    for turn in &fixture.turns {
        let request = serde_json::json!({
            "input": turn.input,
            "mode": turn.mode,
        });

        let (status, body) = if turn.expected_delta.is_some() && turn.expected_status_code() == 200
        {
            send_json_with_bearer(
                &ctx.router,
                "POST",
                &format!("/admin/sessions/{}/turn/debug", session.id),
                ADMIN_TOKEN,
                request,
            )
            .await
        } else {
            send_json(
                &ctx.router,
                "POST",
                &format!("/sessions/{}/turn", session.id),
                request,
            )
            .await
        };

        anyhow::ensure!(
            status.as_u16() == turn.expected_status_code(),
            "turn status mismatch: expected {}, got {}",
            turn.expected_status_code(),
            status.as_u16()
        );

        if status == http::StatusCode::OK {
            let payload: Value = json_body(&body);
            let player_response = payload["player_response"].as_str().unwrap_or("");
            for needle in &turn.expected_response_contains {
                anyhow::ensure!(
                    player_response.contains(needle),
                    "response missing expected substring '{needle}'"
                );
            }

            if let Some(expected_delta) = &turn.expected_delta {
                let applied_delta: domain::WorldStateDelta =
                    serde_json::from_value(payload["applied_delta"].clone())?;
                anyhow::ensure!(
                    &applied_delta == expected_delta,
                    "applied delta did not match fixture expectation"
                );
            }
        }
    }

    let (export_status, export_body) = send_empty(
        &ctx.router,
        "GET",
        &format!("/sessions/{}/export", session.id),
    )
    .await;
    anyhow::ensure!(
        export_status == http::StatusCode::OK,
        "export failed with status {export_status}"
    );
    let export_json: Value = json_body(&export_body);
    let projected: FrontendVisibleState =
        serde_json::from_value(export_json["visible_state"].clone())?;

    anyhow::ensure!(
        projected.state_version == fixture.expected_final.world_state_version,
        "final state version mismatch: expected {}, got {}",
        fixture.expected_final.world_state_version,
        projected.state_version
    );

    for needle in &fixture.expected_final.visible_fact_contains {
        anyhow::ensure!(
            projected
                .player_known_facts
                .iter()
                .any(|fact| fact.text.contains(needle)),
            "projection missing visible fact substring '{needle}'"
        );
    }

    for needle in &fixture.expected_final.visible_memory_contains {
        anyhow::ensure!(
            projected
                .visible_memories
                .iter()
                .any(|memory| memory.text.contains(needle)),
            "projection missing visible memory substring '{needle}'"
        );
    }

    for hidden_id in &fixture
        .expected_final
        .hidden_fact_ids_absent_from_projection
    {
        anyhow::ensure!(
            projected
                .player_known_facts
                .iter()
                .all(|fact| &fact.id != hidden_id),
            "hidden fact id '{hidden_id}' leaked into projection"
        );
    }

    let (_, raw_body) = send_empty_with_bearer(
        &ctx.router,
        "GET",
        &format!("/admin/sessions/{}/export/raw", session.id),
        ADMIN_TOKEN,
    )
    .await;
    let raw_export: Value = json_body(&raw_body);
    let world_state: domain::WorldState =
        serde_json::from_value(raw_export["world_state"].clone())?;
    anyhow::ensure!(
        world_state.version == fixture.expected_final.world_state_version,
        "authoritative final version mismatch: expected {}, got {}",
        fixture.expected_final.world_state_version,
        world_state.version
    );

    ctx.cleanup().await;
    Ok(())
}
