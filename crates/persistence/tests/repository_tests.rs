use domain::{MessageRecord, MessageRole, Scenario, WorldState, fixtures};
use engine::{SessionTurnLock, TurnLockError};
use persistence::{
    EventRepository, MessageRepository, PgPersistence, PostgresSessionTurnLock,
    ProviderConfigRepository, ProviderRecord, ScenarioRepository, SessionRepository,
    WorldStateRepository,
};
use sqlx::Executor;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Setup helper
// ---------------------------------------------------------------------------

async fn setup() -> (PgPersistence, Option<ContainerAsync<Postgres>>) {
    let (url, container) = if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        (url, None)
    } else {
        let container = Postgres::default()
            .with_db_name("test")
            .with_user("test")
            .with_password("test")
            .start()
            .await
            .unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        (
            format!("postgres://test:test@{host}:{port}/test"),
            Some(container),
        )
    };
    let p = PgPersistence::connect(&url).await.unwrap();
    p.pool()
        .execute("DROP SCHEMA IF EXISTS public CASCADE")
        .await
        .unwrap();
    p.pool().execute("CREATE SCHEMA public").await.unwrap();
    p.migrate().await.unwrap();
    (p, container)
}

// ---------------------------------------------------------------------------
// Sample constructors
// ---------------------------------------------------------------------------

fn sample_scenario() -> Scenario {
    let mut scenario = fixtures::scenario()
        .with_title("Test Scenario")
        .with_setting("test setting")
        .build();
    scenario.tone = "neutral".into();
    scenario
}

fn sample_world_state(session_id: Uuid, scenario_id: Uuid) -> WorldState {
    let scenario = sample_scenario();
    let mut state = fixtures::world_state(&scenario)
        .with_session_id(session_id)
        .with_version(0)
        .build();
    state.scenario_id = scenario_id;
    state.current_location_id = None;
    state.active_speaker_id = None;
    state.facts.clear();
    state.npcs.clear();
    state.factions.clear();
    state.quests.clear();
    state.clocks.clear();
    state
}

fn sample_message(session_id: Uuid) -> MessageRecord {
    MessageRecord {
        id: Uuid::new_v4(),
        session_id,
        role: MessageRole::User,
        speaker_id: None,
        content: "Hello world".into(),
        scene_type: None,
        prompt_template_version: None,
        raw_provider_output: None,
    }
}

fn sample_provider(name: &str, is_default: bool) -> ProviderRecord {
    ProviderRecord {
        id: Uuid::new_v4(),
        name: name.into(),
        provider_type: "openai".into(),
        base_url: "https://api.openai.com".into(),
        model: "gpt-4".into(),
        api_key_secret_ref: None,
        capabilities: serde_json::json!({}),
        is_default,
    }
}

// ===========================================================================
// 6.1  ScenarioRepository — 5 tests
// ===========================================================================

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn create_and_get_scenario() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let title = scenario.title.clone();
    let id = scenario.id;

    ScenarioRepository::create(&p, scenario).await.unwrap();
    let fetched = ScenarioRepository::get(&p, id)
        .await
        .unwrap()
        .expect("scenario should exist");

    assert_eq!(fetched.title, title);
    assert_eq!(fetched.id, id);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn list_scenarios_returns_all() {
    let (p, _container) = setup().await;

    ScenarioRepository::create(&p, sample_scenario())
        .await
        .unwrap();
    ScenarioRepository::create(&p, sample_scenario())
        .await
        .unwrap();

    let list = ScenarioRepository::list(&p).await.unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn update_scenario_changes_title() {
    let (p, _container) = setup().await;

    let mut scenario = sample_scenario();
    let id = scenario.id;
    ScenarioRepository::create(&p, scenario.clone())
        .await
        .unwrap();

    scenario.title = "Updated Title".into();
    ScenarioRepository::update(&p, scenario).await.unwrap();

    let fetched = ScenarioRepository::get(&p, id)
        .await
        .unwrap()
        .expect("scenario should exist");
    assert_eq!(fetched.title, "Updated Title");
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn delete_scenario() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    ScenarioRepository::delete(&p, id).await.unwrap();

    let result = ScenarioRepository::get(&p, id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn get_unknown_scenario_returns_none() {
    let (p, _container) = setup().await;

    let result = ScenarioRepository::get(&p, Uuid::new_v4()).await.unwrap();
    assert!(result.is_none());
}

// ===========================================================================
// 6.2  SessionRepository — 5 tests
// ===========================================================================

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn create_and_get_session() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();

    let session = SessionRepository::create(&p, scenario_id, "My Session".into())
        .await
        .unwrap();
    let fetched = SessionRepository::get(&p, session.id)
        .await
        .unwrap()
        .expect("session should exist");

    assert_eq!(fetched.id, session.id);
    assert_eq!(fetched.title, "My Session");
    assert_eq!(fetched.scenario_id, scenario_id);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn list_sessions() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();

    SessionRepository::create(&p, scenario_id, "Session A".into())
        .await
        .unwrap();
    SessionRepository::create(&p, scenario_id, "Session B".into())
        .await
        .unwrap();

    let list = SessionRepository::list(&p).await.unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn delete_session() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();

    let session = SessionRepository::create(&p, scenario_id, "To Delete".into())
        .await
        .unwrap();

    SessionRepository::delete(&p, session.id).await.unwrap();

    let result = SessionRepository::get(&p, session.id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn set_provider_on_session() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();

    let session = SessionRepository::create(&p, scenario_id, "Provider Session".into())
        .await
        .unwrap();

    let provider = sample_provider("session-provider", false);
    let provider_id = provider.id;
    ProviderConfigRepository::create(&p, provider)
        .await
        .unwrap();

    let updated = SessionRepository::set_provider(&p, session.id, Some(provider_id))
        .await
        .unwrap();

    assert_eq!(updated.provider_id, Some(provider_id));

    // Confirm it persisted
    let fetched = SessionRepository::get(&p, session.id)
        .await
        .unwrap()
        .expect("session should exist");
    assert_eq!(fetched.provider_id, Some(provider_id));
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn set_provider_unknown_session_returns_not_found() {
    let (p, _container) = setup().await;

    let result = SessionRepository::set_provider(&p, Uuid::new_v4(), Some(Uuid::new_v4())).await;

    assert!(
        matches!(result, Err(persistence::RepoError::NotFound)),
        "expected NotFound, got: {result:?}"
    );
}

// ===========================================================================
// 6.3  WorldStateRepository — 3 tests
// ===========================================================================

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn save_and_get_world_state() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    let session = SessionRepository::create(&p, scenario_id, "WS Session".into())
        .await
        .unwrap();

    let ws = sample_world_state(session.id, scenario_id);
    WorldStateRepository::save(&p, &ws, None).await.unwrap();

    let fetched = WorldStateRepository::get(&p, session.id)
        .await
        .unwrap()
        .expect("world state should exist");

    assert_eq!(fetched.session_id, session.id);
    assert_eq!(fetched.version, 0);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn save_second_version_replaces_first() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    let session = SessionRepository::create(&p, scenario_id, "WS Session v2".into())
        .await
        .unwrap();

    // Save version 0 (insert path)
    let ws0 = sample_world_state(session.id, scenario_id);
    WorldStateRepository::save(&p, &ws0, None).await.unwrap();

    // Save version 1 via optimistic-concurrency update path
    let mut ws1 = ws0.clone();
    ws1.version = 1;
    WorldStateRepository::save(&p, &ws1, Some(0)).await.unwrap();

    let fetched = WorldStateRepository::get(&p, session.id)
        .await
        .unwrap()
        .expect("world state should exist");

    assert_eq!(fetched.version, 1);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn get_unknown_session_world_state_returns_none() {
    let (p, _container) = setup().await;

    let result = WorldStateRepository::get(&p, Uuid::new_v4()).await.unwrap();
    assert!(result.is_none());
}

// ===========================================================================
// 6.4  MessageRepository — 2 tests
// ===========================================================================

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn append_and_recent_messages() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    let session = SessionRepository::create(&p, scenario_id, "Msg Session".into())
        .await
        .unwrap();

    for _ in 0..4 {
        let msg = sample_message(session.id);
        MessageRepository::append(&p, &msg).await.unwrap();
    }

    let recent = MessageRepository::recent(&p, session.id, 3).await.unwrap();
    assert_eq!(recent.len(), 3);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn recent_messages_respects_limit() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    let session = SessionRepository::create(&p, scenario_id, "Msg Limit Session".into())
        .await
        .unwrap();

    for _ in 0..2 {
        let msg = sample_message(session.id);
        MessageRepository::append(&p, &msg).await.unwrap();
    }

    // Asking for more than available — should return only 2
    let recent = MessageRepository::recent(&p, session.id, 10).await.unwrap();
    assert_eq!(recent.len(), 2);
}

// ===========================================================================
// 6.5  EventRepository — 2 tests
// ===========================================================================

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn append_and_list_events() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    let session = SessionRepository::create(&p, scenario_id, "Event Session".into())
        .await
        .unwrap();

    EventRepository::append(&p, session.id, "test_event", "First event")
        .await
        .unwrap();
    EventRepository::append(&p, session.id, "test_event", "Second event")
        .await
        .unwrap();

    let events = EventRepository::list(&p, session.id).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].description, "First event");
    assert_eq!(events[1].description, "Second event");
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn list_events_for_unknown_session_returns_empty() {
    let (p, _container) = setup().await;

    let events = EventRepository::list(&p, Uuid::new_v4()).await.unwrap();
    assert!(events.is_empty());
}

// ===========================================================================
// 6.6  ProviderConfigRepository — 4 tests
// ===========================================================================

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn create_and_get_provider() {
    let (p, _container) = setup().await;

    let record = sample_provider("openai-default", false);
    let id = record.id;
    ProviderConfigRepository::create(&p, record).await.unwrap();

    let fetched = ProviderConfigRepository::get(&p, id)
        .await
        .unwrap()
        .expect("provider should exist");

    assert_eq!(fetched.name, "openai-default");
    assert_eq!(fetched.id, id);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn get_provider_by_name() {
    let (p, _container) = setup().await;

    let record = sample_provider("named-provider", false);
    ProviderConfigRepository::create(&p, record.clone())
        .await
        .unwrap();

    let fetched = ProviderConfigRepository::get_by_name(&p, "named-provider")
        .await
        .unwrap()
        .expect("provider should exist by name");

    assert_eq!(fetched.id, record.id);
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn delete_provider() {
    let (p, _container) = setup().await;

    let record = sample_provider("delete-me", false);
    let id = record.id;
    ProviderConfigRepository::create(&p, record).await.unwrap();
    ProviderConfigRepository::delete(&p, id).await.unwrap();

    let result = ProviderConfigRepository::get(&p, id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn get_default_returns_is_default_true_provider() {
    let (p, _container) = setup().await;

    let non_default = sample_provider("regular", false);
    let default_record = sample_provider("the-default", true);
    let default_id = default_record.id;

    ProviderConfigRepository::create(&p, non_default)
        .await
        .unwrap();
    ProviderConfigRepository::create(&p, default_record)
        .await
        .unwrap();

    let fetched = ProviderConfigRepository::get_default(&p)
        .await
        .unwrap()
        .expect("default provider should exist");

    assert_eq!(fetched.id, default_id);
    assert!(fetched.is_default);
}

// ===========================================================================
// 6.7  PostgresSessionTurnLock — 3 tests
// ===========================================================================

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn acquire_lock_on_fresh_session_succeeds() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    let session = SessionRepository::create(&p, scenario_id, "Lock Session".into())
        .await
        .unwrap();

    let lock = PostgresSessionTurnLock::new(p.pool().clone());
    let guard = lock.acquire(session.id).await;
    assert!(guard.is_ok(), "acquire should succeed on fresh session");
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn second_acquire_returns_already_in_progress() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    let session = SessionRepository::create(&p, scenario_id, "Lock Session 2".into())
        .await
        .unwrap();

    let lock = PostgresSessionTurnLock::new(p.pool().clone());

    // Hold the first guard — do not drop it
    let _guard = lock.acquire(session.id).await.unwrap();

    let second = lock.acquire(session.id).await;
    assert!(
        matches!(second, Err(TurnLockError::AlreadyInProgress)),
        "expected AlreadyInProgress, got: {second:?}"
    );
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn stale_lock_is_recovered() {
    let (p, _container) = setup().await;

    let scenario = sample_scenario();
    let scenario_id = scenario.id;
    ScenarioRepository::create(&p, scenario).await.unwrap();
    let session = SessionRepository::create(&p, scenario_id, "Stale Lock Session".into())
        .await
        .unwrap();

    // Manually set processing_turn=true with a timestamp 10 minutes in the past
    // to simulate a stale lock left by a crashed process.
    sqlx::query(
        "UPDATE sessions
         SET processing_turn = true,
             processing_turn_started_at = now() - INTERVAL '10 minutes'
         WHERE id = $1",
    )
    .bind(session.id)
    .execute(p.pool())
    .await
    .unwrap();

    let lock = PostgresSessionTurnLock::new(p.pool().clone());
    let guard = lock.acquire(session.id).await;
    assert!(
        guard.is_ok(),
        "stale lock (>5 min) should be recovered; got: {guard:?}"
    );
}
