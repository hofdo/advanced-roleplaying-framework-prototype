use async_trait::async_trait;
use domain::{
    ActionResolution, ClockState, ClueState, Fact, FactSource, FactVisibility, FactionState,
    InventoryItem, MessageRecord, MessageRole, NpcAvailability, NpcState, PlayerCharacterState,
    QuestState, QuestStatus, RelationshipState, Scenario, ScenarioId, SessionId, ViewerContext,
    WorldState,
};
use engine::{
    BasicFrontendStateProjector, FrontendStateProjector, LoadedTurnState, TurnPipelineError,
    TurnStateStore, ValidatedWorldStateDelta,
};
use sqlx::Row;
use std::{collections::HashMap, sync::Mutex};
use uuid::Uuid;

use crate::{
    EventRecord, EventRepository, MessageRepository, PgPersistence, ProviderConfigRepository,
    ProviderRecord, RawTimeline, RepoError, ScenarioRepository, SessionRecord, SessionRepository,
    TimelineEntry, WorldStateDeltaRepository, WorldStateRepository,
};

#[async_trait]
pub trait ApplicationStore: TurnStateStore + Send + Sync {
    async fn storage_status(&self) -> String;
    async fn create_scenario(&self, scenario: Scenario) -> Result<Scenario, TurnPipelineError>;
    async fn list_scenarios(&self) -> Result<Vec<Scenario>, TurnPipelineError>;
    async fn get_scenario(&self, id: ScenarioId) -> Result<Option<Scenario>, TurnPipelineError>;
    async fn update_scenario(
        &self,
        scenario: Scenario,
    ) -> Result<Option<Scenario>, TurnPipelineError>;
    async fn delete_scenario(&self, id: ScenarioId) -> Result<bool, TurnPipelineError>;
    async fn create_session(
        &self,
        scenario_id: ScenarioId,
        title: String,
    ) -> Result<Option<SessionRecord>, TurnPipelineError>;
    async fn list_sessions(&self) -> Result<Vec<SessionRecord>, TurnPipelineError>;
    async fn get_session(&self, id: SessionId) -> Result<Option<SessionRecord>, TurnPipelineError>;
    async fn delete_session(&self, id: SessionId) -> Result<bool, TurnPipelineError>;
    async fn set_session_provider(
        &self,
        session_id: SessionId,
        provider_id: Option<Uuid>,
    ) -> Result<Option<SessionRecord>, TurnPipelineError>;
    async fn world_state(
        &self,
        session_id: SessionId,
    ) -> Result<Option<WorldState>, TurnPipelineError>;
    async fn events(&self, session_id: SessionId) -> Result<Vec<EventRecord>, TurnPipelineError>;
    async fn timeline(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<TimelineEntry>, TurnPipelineError>;
    async fn raw_timeline(
        &self,
        session_id: SessionId,
    ) -> Result<Option<RawTimeline>, TurnPipelineError>;
    async fn create_provider(
        &self,
        record: ProviderRecord,
    ) -> Result<ProviderRecord, TurnPipelineError>;
    async fn list_providers(&self) -> Result<Vec<ProviderRecord>, TurnPipelineError>;
    async fn delete_provider(&self, id: Uuid) -> Result<(), TurnPipelineError>;
}

#[derive(Debug, Default)]
pub struct InMemoryApplicationStore {
    inner: Mutex<InMemoryApplicationStoreInner>,
    store_raw_provider_output: bool,
}

#[derive(Debug, Default)]
struct InMemoryApplicationStoreInner {
    scenarios: HashMap<ScenarioId, Scenario>,
    sessions: HashMap<SessionId, SessionRecord>,
    world_states: HashMap<SessionId, WorldState>,
    messages: HashMap<SessionId, Vec<MessageRecord>>,
    events: HashMap<SessionId, Vec<EventRecord>>,
    timeline_entries: HashMap<SessionId, Vec<TimelineEntry>>,
    providers: Vec<ProviderRecord>,
}

impl InMemoryApplicationStore {
    pub fn new(store_raw_provider_output: bool) -> Self {
        Self {
            inner: Mutex::new(InMemoryApplicationStoreInner::default()),
            store_raw_provider_output,
        }
    }

    pub fn insert_scenario(&self, scenario: Scenario) -> Scenario {
        self.inner
            .lock()
            .expect("application store mutex")
            .scenarios
            .insert(scenario.id, scenario.clone());
        scenario
    }

    pub fn snapshot_scenarios(&self) -> Vec<Scenario> {
        self.inner
            .lock()
            .expect("application store mutex")
            .scenarios
            .values()
            .cloned()
            .collect()
    }

    pub fn snapshot_scenario(&self, id: ScenarioId) -> Option<Scenario> {
        self.inner
            .lock()
            .expect("application store mutex")
            .scenarios
            .get(&id)
            .cloned()
    }

    pub fn replace_scenario(&self, scenario: Scenario) -> Option<Scenario> {
        let mut inner = self.inner.lock().expect("application store mutex");
        if let std::collections::hash_map::Entry::Occupied(mut entry) =
            inner.scenarios.entry(scenario.id)
        {
            entry.insert(scenario.clone());
            Some(scenario)
        } else {
            None
        }
    }

    pub fn remove_scenario(&self, id: ScenarioId) -> bool {
        self.inner
            .lock()
            .expect("application store mutex")
            .scenarios
            .remove(&id)
            .is_some()
    }

    pub fn insert_session(&self, scenario_id: ScenarioId, title: String) -> Option<SessionRecord> {
        let mut inner = self.inner.lock().expect("application store mutex");
        let scenario = inner.scenarios.get(&scenario_id)?.clone();
        let id = Uuid::new_v4();
        let session = SessionRecord {
            id,
            scenario_id,
            title,
            status: "active".into(),
            provider_id: None,
        };
        let world_state = initial_world_state(id, &scenario);
        let intro_message = opening_intro_message(id, &scenario, &world_state);
        inner.sessions.insert(id, session.clone());
        inner.world_states.insert(id, world_state);
        inner
            .messages
            .entry(id)
            .or_default()
            .push(intro_message.clone());
        inner
            .timeline_entries
            .entry(id)
            .or_default()
            .push(TimelineEntry {
                kind: "system_message".into(),
                description: intro_message.content,
                message_id: Some(intro_message.id),
                event_id: None,
                world_state_version: None,
            });
        Some(session)
    }

    pub fn snapshot_sessions(&self) -> Vec<SessionRecord> {
        self.inner
            .lock()
            .expect("application store mutex")
            .sessions
            .values()
            .cloned()
            .collect()
    }

    pub fn snapshot_session(&self, id: SessionId) -> Option<SessionRecord> {
        self.inner
            .lock()
            .expect("application store mutex")
            .sessions
            .get(&id)
            .cloned()
    }

    pub fn remove_session(&self, id: SessionId) -> bool {
        let mut inner = self.inner.lock().expect("application store mutex");
        let existed = inner.sessions.remove(&id).is_some();
        inner.world_states.remove(&id);
        inner.messages.remove(&id);
        inner.events.remove(&id);
        inner.timeline_entries.remove(&id);
        existed
    }

    pub fn update_session_provider(
        &self,
        session_id: SessionId,
        provider_id: Option<Uuid>,
    ) -> Option<SessionRecord> {
        let mut inner = self.inner.lock().expect("application store mutex");
        let session = inner.sessions.get_mut(&session_id)?;
        session.provider_id = provider_id;
        Some(session.clone())
    }

    pub fn snapshot_world_state(&self, session_id: SessionId) -> Option<WorldState> {
        self.inner
            .lock()
            .expect("application store mutex")
            .world_states
            .get(&session_id)
            .cloned()
    }

    pub fn snapshot_events(&self, session_id: SessionId) -> Vec<EventRecord> {
        self.inner
            .lock()
            .expect("application store mutex")
            .events
            .get(&session_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn snapshot_timeline(&self, session_id: SessionId) -> Option<Vec<TimelineEntry>> {
        let inner = self.inner.lock().expect("application store mutex");
        let _ = inner.sessions.get(&session_id)?;
        Some(
            inner
                .timeline_entries
                .get(&session_id)
                .cloned()
                .unwrap_or_default(),
        )
    }

    pub fn snapshot_messages(&self, session_id: SessionId) -> Option<Vec<MessageRecord>> {
        let inner = self.inner.lock().expect("application store mutex");
        let _ = inner.sessions.get(&session_id)?;
        Some(inner.messages.get(&session_id).cloned().unwrap_or_default())
    }
}

#[async_trait]
impl ApplicationStore for InMemoryApplicationStore {
    async fn storage_status(&self) -> String {
        "memory".into()
    }

    async fn create_scenario(&self, scenario: Scenario) -> Result<Scenario, TurnPipelineError> {
        Ok(InMemoryApplicationStore::insert_scenario(self, scenario))
    }

    async fn list_scenarios(&self) -> Result<Vec<Scenario>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::snapshot_scenarios(self))
    }

    async fn get_scenario(&self, id: ScenarioId) -> Result<Option<Scenario>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::snapshot_scenario(self, id))
    }

    async fn update_scenario(
        &self,
        scenario: Scenario,
    ) -> Result<Option<Scenario>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::replace_scenario(self, scenario))
    }

    async fn delete_scenario(&self, id: ScenarioId) -> Result<bool, TurnPipelineError> {
        Ok(InMemoryApplicationStore::remove_scenario(self, id))
    }

    async fn create_session(
        &self,
        scenario_id: ScenarioId,
        title: String,
    ) -> Result<Option<SessionRecord>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::insert_session(
            self,
            scenario_id,
            title,
        ))
    }

    async fn list_sessions(&self) -> Result<Vec<SessionRecord>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::snapshot_sessions(self))
    }

    async fn get_session(&self, id: SessionId) -> Result<Option<SessionRecord>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::snapshot_session(self, id))
    }

    async fn delete_session(&self, id: SessionId) -> Result<bool, TurnPipelineError> {
        Ok(InMemoryApplicationStore::remove_session(self, id))
    }

    async fn set_session_provider(
        &self,
        session_id: SessionId,
        provider_id: Option<Uuid>,
    ) -> Result<Option<SessionRecord>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::update_session_provider(
            self,
            session_id,
            provider_id,
        ))
    }

    async fn world_state(
        &self,
        session_id: SessionId,
    ) -> Result<Option<WorldState>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::snapshot_world_state(
            self, session_id,
        ))
    }

    async fn events(&self, session_id: SessionId) -> Result<Vec<EventRecord>, TurnPipelineError> {
        Ok(InMemoryApplicationStore::snapshot_events(self, session_id))
    }

    async fn timeline(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<TimelineEntry>, TurnPipelineError> {
        InMemoryApplicationStore::snapshot_timeline(self, session_id)
            .ok_or(TurnPipelineError::NotFound)
    }

    async fn raw_timeline(
        &self,
        session_id: SessionId,
    ) -> Result<Option<RawTimeline>, TurnPipelineError> {
        let Some(session) = InMemoryApplicationStore::snapshot_session(self, session_id) else {
            return Ok(None);
        };
        Ok(Some(RawTimeline {
            session,
            messages: InMemoryApplicationStore::snapshot_messages(self, session_id)
                .unwrap_or_default(),
            deltas: vec![],
            events: InMemoryApplicationStore::snapshot_events(self, session_id),
        }))
    }

    async fn create_provider(
        &self,
        record: ProviderRecord,
    ) -> Result<ProviderRecord, TurnPipelineError> {
        self.inner
            .lock()
            .expect("application store mutex")
            .providers
            .push(record.clone());
        Ok(record)
    }

    async fn list_providers(&self) -> Result<Vec<ProviderRecord>, TurnPipelineError> {
        Ok(self
            .inner
            .lock()
            .expect("application store mutex")
            .providers
            .clone())
    }

    async fn delete_provider(&self, id: Uuid) -> Result<(), TurnPipelineError> {
        let mut inner = self.inner.lock().expect("application store mutex");
        inner.providers.retain(|p| p.id != id);
        for session in inner.sessions.values_mut() {
            if session.provider_id == Some(id) {
                session.provider_id = None;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl TurnStateStore for InMemoryApplicationStore {
    async fn load_turn_state(
        &self,
        session_id: SessionId,
    ) -> Result<LoadedTurnState, TurnPipelineError> {
        let inner = self.inner.lock().expect("application store mutex");
        let session = inner
            .sessions
            .get(&session_id)
            .cloned()
            .ok_or(TurnPipelineError::NotFound)?;
        let scenario = inner
            .scenarios
            .get(&session.scenario_id)
            .cloned()
            .ok_or(TurnPipelineError::NotFound)?;
        let world_state = inner
            .world_states
            .get(&session_id)
            .cloned()
            .ok_or(TurnPipelineError::NotFound)?;
        let recent_messages = inner
            .messages
            .get(&session_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .rev()
            .filter(|message| message.role != MessageRole::System)
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        Ok(LoadedTurnState {
            scenario,
            world_state,
            recent_messages,
        })
    }

    async fn persist_successful_turn(
        &self,
        user_message: MessageRecord,
        mut assistant_message: MessageRecord,
        delta: ValidatedWorldStateDelta,
        updated_state: WorldState,
    ) -> Result<(), TurnPipelineError> {
        let session_id = updated_state.session_id;
        let world_state_version = updated_state.version;
        let user_timeline_entry = TimelineEntry {
            kind: "user_message".into(),
            description: user_message.content.clone(),
            message_id: Some(user_message.id),
            event_id: None,
            world_state_version: None,
        };
        if !self.store_raw_provider_output {
            assistant_message.raw_provider_output = None;
        }
        let assistant_timeline_entry = TimelineEntry {
            kind: "assistant_message".into(),
            description: assistant_message.content.clone(),
            message_id: Some(assistant_message.id),
            event_id: None,
            world_state_version: Some(world_state_version),
        };
        let mut inner = self.inner.lock().expect("application store mutex");
        inner
            .messages
            .entry(session_id)
            .or_default()
            .extend([user_message, assistant_message.clone()]);
        inner
            .timeline_entries
            .entry(session_id)
            .or_default()
            .extend([user_timeline_entry, assistant_timeline_entry]);
        inner.world_states.insert(session_id, updated_state);
        for description in delta.0.event_log_entries {
            let record = EventRecord {
                id: Uuid::new_v4(),
                session_id,
                event_type: "world_event".into(),
                description,
            };
            inner
                .events
                .entry(session_id)
                .or_default()
                .push(record.clone());
            inner
                .timeline_entries
                .entry(session_id)
                .or_default()
                .push(TimelineEntry {
                    kind: record.event_type.clone(),
                    description: record.description.clone(),
                    message_id: None,
                    event_id: Some(record.id),
                    world_state_version: Some(world_state_version),
                });
        }
        Ok(())
    }

    async fn persist_error_event(
        &self,
        session_id: SessionId,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        let record = EventRecord {
            id: Uuid::new_v4(),
            session_id,
            event_type: "turn_error".into(),
            description,
        };
        let mut inner = self.inner.lock().expect("application store mutex");
        inner
            .events
            .entry(session_id)
            .or_default()
            .push(record.clone());
        inner
            .timeline_entries
            .entry(session_id)
            .or_default()
            .push(TimelineEntry {
                kind: record.event_type.clone(),
                description: record.description,
                message_id: None,
                event_id: Some(record.id),
                world_state_version: None,
            });
        Ok(())
    }

    async fn persist_pipeline_event(
        &self,
        session_id: SessionId,
        event_type: &'static str,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        let record = EventRecord {
            id: Uuid::new_v4(),
            session_id,
            event_type: event_type.into(),
            description,
        };
        let mut inner = self.inner.lock().expect("application store mutex");
        inner
            .events
            .entry(session_id)
            .or_default()
            .push(record.clone());
        inner
            .timeline_entries
            .entry(session_id)
            .or_default()
            .push(TimelineEntry {
                kind: record.event_type.clone(),
                description: record.description,
                message_id: None,
                event_id: Some(record.id),
                world_state_version: None,
            });
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PostgresApplicationStore {
    persistence: PgPersistence,
    store_raw_provider_output: bool,
}

impl PostgresApplicationStore {
    pub fn new(persistence: PgPersistence, store_raw_provider_output: bool) -> Self {
        Self {
            persistence,
            store_raw_provider_output,
        }
    }
}

#[async_trait]
impl ApplicationStore for PostgresApplicationStore {
    async fn storage_status(&self) -> String {
        match sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(self.persistence.pool())
            .await
        {
            Ok(_) => "postgres:ok".into(),
            Err(error) => format!("postgres:error:{error}"),
        }
    }

    async fn create_scenario(&self, scenario: Scenario) -> Result<Scenario, TurnPipelineError> {
        ScenarioRepository::create(&self.persistence, scenario)
            .await
            .map_err(repo_to_pipeline)
    }

    async fn list_scenarios(&self) -> Result<Vec<Scenario>, TurnPipelineError> {
        let summaries = ScenarioRepository::list(&self.persistence)
            .await
            .map_err(repo_to_pipeline)?;
        let mut scenarios = Vec::with_capacity(summaries.len());
        for summary in summaries {
            if let Some(scenario) = ScenarioRepository::get(&self.persistence, summary.id)
                .await
                .map_err(repo_to_pipeline)?
            {
                scenarios.push(scenario);
            }
        }
        Ok(scenarios)
    }

    async fn get_scenario(&self, id: ScenarioId) -> Result<Option<Scenario>, TurnPipelineError> {
        ScenarioRepository::get(&self.persistence, id)
            .await
            .map_err(repo_to_pipeline)
    }

    async fn update_scenario(
        &self,
        scenario: Scenario,
    ) -> Result<Option<Scenario>, TurnPipelineError> {
        match ScenarioRepository::update(&self.persistence, scenario).await {
            Ok(scenario) => Ok(Some(scenario)),
            Err(RepoError::NotFound) => Ok(None),
            Err(error) => Err(repo_to_pipeline(error)),
        }
    }

    async fn delete_scenario(&self, id: ScenarioId) -> Result<bool, TurnPipelineError> {
        if ScenarioRepository::get(&self.persistence, id)
            .await
            .map_err(repo_to_pipeline)?
            .is_none()
        {
            return Ok(false);
        }
        ScenarioRepository::delete(&self.persistence, id)
            .await
            .map_err(repo_to_pipeline)?;
        Ok(true)
    }

    async fn create_session(
        &self,
        scenario_id: ScenarioId,
        title: String,
    ) -> Result<Option<SessionRecord>, TurnPipelineError> {
        let Some(scenario) = ScenarioRepository::get(&self.persistence, scenario_id)
            .await
            .map_err(repo_to_pipeline)?
        else {
            return Ok(None);
        };
        let session_id = Uuid::new_v4();
        let session = SessionRecord {
            id: session_id,
            scenario_id,
            title,
            status: "active".into(),
            provider_id: None,
        };
        let world_state = initial_world_state(session.id, &scenario);
        let intro_message = opening_intro_message(session.id, &scenario, &world_state);

        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(|error| TurnPipelineError::Store(error.to_string()))?;
        sqlx::query("INSERT INTO sessions (id, scenario_id, title) VALUES ($1, $2, $3)")
            .bind(session.id)
            .bind(session.scenario_id)
            .bind(&session.title)
            .execute(&mut *tx)
            .await
            .map_err(|error| TurnPipelineError::Store(error.to_string()))?;
        sqlx::query(
            "INSERT INTO world_states (session_id, state, version)
             VALUES ($1, $2, $3)",
        )
        .bind(world_state.session_id)
        .bind(sqlx::types::Json(&world_state))
        .bind(world_state.version)
        .execute(&mut *tx)
        .await
        .map_err(|error| TurnPipelineError::Store(error.to_string()))?;
        sqlx::query(
            "INSERT INTO messages
             (id, session_id, role, speaker_id, content, scene_type, prompt_template_version, raw_provider_output)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(intro_message.id)
        .bind(intro_message.session_id)
        .bind("System")
        .bind(Option::<String>::None)
        .bind(&intro_message.content)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(Option::<sqlx::types::Json<serde_json::Value>>::None)
        .execute(&mut *tx)
        .await
        .map_err(|error| TurnPipelineError::Store(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| TurnPipelineError::Store(error.to_string()))?;

        Ok(Some(session))
    }

    async fn list_sessions(&self) -> Result<Vec<SessionRecord>, TurnPipelineError> {
        SessionRepository::list(&self.persistence)
            .await
            .map_err(repo_to_pipeline)
    }

    async fn get_session(&self, id: SessionId) -> Result<Option<SessionRecord>, TurnPipelineError> {
        SessionRepository::get(&self.persistence, id)
            .await
            .map_err(repo_to_pipeline)
    }

    async fn delete_session(&self, id: SessionId) -> Result<bool, TurnPipelineError> {
        if SessionRepository::get(&self.persistence, id)
            .await
            .map_err(repo_to_pipeline)?
            .is_none()
        {
            return Ok(false);
        }
        SessionRepository::delete(&self.persistence, id)
            .await
            .map_err(repo_to_pipeline)?;
        Ok(true)
    }

    async fn set_session_provider(
        &self,
        session_id: SessionId,
        provider_id: Option<Uuid>,
    ) -> Result<Option<SessionRecord>, TurnPipelineError> {
        match SessionRepository::set_provider(&self.persistence, session_id, provider_id).await {
            Ok(session) => Ok(Some(session)),
            Err(RepoError::NotFound) => Ok(None),
            Err(error) => Err(repo_to_pipeline(error)),
        }
    }

    async fn world_state(
        &self,
        session_id: SessionId,
    ) -> Result<Option<WorldState>, TurnPipelineError> {
        WorldStateRepository::get(&self.persistence, session_id)
            .await
            .map_err(repo_to_pipeline)
    }

    async fn events(&self, session_id: SessionId) -> Result<Vec<EventRecord>, TurnPipelineError> {
        EventRepository::list(&self.persistence, session_id)
            .await
            .map_err(repo_to_pipeline)
    }

    async fn timeline(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<TimelineEntry>, TurnPipelineError> {
        if SessionRepository::get(&self.persistence, session_id)
            .await
            .map_err(repo_to_pipeline)?
            .is_none()
        {
            return Err(TurnPipelineError::NotFound);
        }

        let rows = sqlx::query(
            "WITH ordered_deltas AS (
                 SELECT message_id,
                        created_at,
                        row_number() OVER (ORDER BY created_at, id) AS world_state_version
                 FROM world_state_deltas
                 WHERE session_id = $1
                   AND validation_status = 'applied'
             ),
             timeline_items AS (
                 SELECT CASE
                            WHEN role = 'Assistant' THEN 'assistant_message'
                            WHEN role = 'System' THEN 'system_message'
                            ELSE 'user_message'
                        END AS kind,
                        content AS description,
                        id AS message_id,
                        NULL::uuid AS event_id,
                        created_at,
                        CASE
                            WHEN role = 'Assistant' THEN (
                                SELECT world_state_version
                                FROM ordered_deltas
                                WHERE ordered_deltas.message_id = messages.id
                                LIMIT 1
                            )
                            ELSE NULL
                        END AS world_state_version,
                        CASE
                            WHEN role = 'Assistant' THEN 2
                            WHEN role = 'System' THEN 0
                            ELSE 1
                        END AS source_rank
                 FROM messages
                 WHERE session_id = $1
                 UNION ALL
                 SELECT event_type AS kind,
                        description,
                        NULL::uuid AS message_id,
                        id AS event_id,
                        created_at,
                        (
                            SELECT MAX(world_state_version)
                            FROM ordered_deltas
                            WHERE ordered_deltas.created_at <= events.created_at
                        ) AS world_state_version,
                        3 AS source_rank
                 FROM events
                 WHERE session_id = $1
             )
             SELECT kind, description, message_id, event_id, world_state_version
             FROM timeline_items
             ORDER BY created_at, source_rank, COALESCE(message_id, event_id)",
        )
        .bind(session_id)
        .fetch_all(self.persistence.pool())
        .await
        .map_err(|error| TurnPipelineError::Store(error.to_string()))?;

        rows.into_iter()
            .map(|row| {
                Ok(TimelineEntry {
                    kind: row
                        .try_get("kind")
                        .map_err(|error| TurnPipelineError::Store(error.to_string()))?,
                    description: row
                        .try_get("description")
                        .map_err(|error| TurnPipelineError::Store(error.to_string()))?,
                    message_id: row
                        .try_get("message_id")
                        .map_err(|error| TurnPipelineError::Store(error.to_string()))?,
                    event_id: row
                        .try_get("event_id")
                        .map_err(|error| TurnPipelineError::Store(error.to_string()))?,
                    world_state_version: row
                        .try_get("world_state_version")
                        .map_err(|error| TurnPipelineError::Store(error.to_string()))?,
                })
            })
            .collect()
    }

    async fn raw_timeline(
        &self,
        session_id: SessionId,
    ) -> Result<Option<RawTimeline>, TurnPipelineError> {
        let session = SessionRepository::get(&self.persistence, session_id)
            .await
            .map_err(repo_to_pipeline)?;
        let Some(session) = session else {
            return Ok(None);
        };
        let messages = MessageRepository::list(&self.persistence, session_id)
            .await
            .map_err(repo_to_pipeline)?;
        let deltas = WorldStateDeltaRepository::list(&self.persistence, session_id)
            .await
            .map_err(repo_to_pipeline)?;
        let events = EventRepository::list(&self.persistence, session_id)
            .await
            .map_err(repo_to_pipeline)?;
        Ok(Some(RawTimeline {
            session,
            messages,
            deltas,
            events,
        }))
    }

    async fn create_provider(
        &self,
        record: ProviderRecord,
    ) -> Result<ProviderRecord, TurnPipelineError> {
        ProviderConfigRepository::create(&self.persistence, record)
            .await
            .map_err(repo_to_pipeline)
    }

    async fn list_providers(&self) -> Result<Vec<ProviderRecord>, TurnPipelineError> {
        ProviderConfigRepository::list(&self.persistence)
            .await
            .map_err(repo_to_pipeline)
    }

    async fn delete_provider(&self, id: Uuid) -> Result<(), TurnPipelineError> {
        ProviderConfigRepository::delete(&self.persistence, id)
            .await
            .map_err(repo_to_pipeline)
    }
}

#[async_trait]
impl TurnStateStore for PostgresApplicationStore {
    async fn load_turn_state(
        &self,
        session_id: SessionId,
    ) -> Result<LoadedTurnState, TurnPipelineError> {
        self.persistence.load_turn_state(session_id).await
    }

    async fn persist_successful_turn(
        &self,
        user_message: MessageRecord,
        mut assistant_message: MessageRecord,
        delta: ValidatedWorldStateDelta,
        updated_state: WorldState,
    ) -> Result<(), TurnPipelineError> {
        if !self.store_raw_provider_output {
            assistant_message.raw_provider_output = None;
        }
        self.persistence
            .persist_successful_turn(user_message, assistant_message, delta, updated_state)
            .await
    }

    async fn persist_error_event(
        &self,
        session_id: SessionId,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        self.persistence
            .persist_error_event(session_id, description)
            .await
    }

    async fn persist_pipeline_event(
        &self,
        session_id: SessionId,
        event_type: &'static str,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        EventRepository::append(&self.persistence, session_id, event_type, &description)
            .await
            .map_err(repo_to_pipeline)
    }
}

fn repo_to_pipeline(error: RepoError) -> TurnPipelineError {
    match error {
        RepoError::NotFound => TurnPipelineError::NotFound,
        other => TurnPipelineError::Store(other.to_string()),
    }
}

pub fn initial_world_state(session_id: SessionId, scenario: &Scenario) -> WorldState {
    WorldState {
        session_id,
        scenario_id: scenario.id,
        version: 0,
        current_location_id: scenario
            .locations
            .first()
            .map(|location| location.id.clone()),
        current_scene: None,
        active_speaker_id: scenario.npcs.first().map(|npc| npc.id.clone()),
        facts: scenario
            .secrets
            .iter()
            .map(|secret| Fact {
                id: secret.id.clone(),
                text: secret.text.clone(),
                visibility: FactVisibility::GmOnly,
                known_by: vec![],
                source: FactSource::Scenario,
                reveal_conditions: secret.reveal_conditions.clone(),
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            })
            .collect(),
        npcs: scenario
            .npcs
            .iter()
            .map(|npc| NpcState {
                npc_id: npc.id.clone(),
                status: npc.initial_status,
                visible_to_player: npc.initial_visible_to_player,
                location_id: npc.initial_location_id.clone().or_else(|| {
                    scenario
                        .locations
                        .first()
                        .map(|location| location.id.clone())
                }),
                attitude_to_player: None,
                known_facts: vec![],
                notes: vec![],
                availability: derive_npc_availability(
                    npc.initial_status,
                    npc.initial_visible_to_player,
                    npc.initial_location_id.as_ref(),
                    scenario.locations.first().map(|location| &location.id),
                ),
                current_intent: None,
                offscreen_actions: vec![],
            })
            .collect(),
        factions: scenario
            .factions
            .iter()
            .map(|faction| FactionState {
                faction_id: faction.id.clone(),
                standing: faction.initial_standing,
                public_notes: vec![],
                hidden_notes: vec![],
                revealed_goals: vec![],
                pressure: 0,
                public_pressure_notes: vec![],
                hidden_pressure_notes: vec![],
            })
            .collect(),
        quests: scenario
            .quests
            .iter()
            .map(|quest| QuestState {
                quest_id: quest.id.clone(),
                status: if quest.visible {
                    QuestStatus::Available
                } else {
                    QuestStatus::Hidden
                },
                completed_objectives: vec![],
                visible: quest.visible,
            })
            .collect(),
        clocks: scenario
            .clocks
            .iter()
            .map(|clock| ClockState {
                id: clock.id.clone(),
                title: clock.title.clone(),
                current: clock.current,
                max: clock.max,
                consequence: clock.consequence.clone(),
                visible_to_player: true,
            })
            .collect(),
        action_resolutions: Vec::<ActionResolution>::new(),
        relationships: Vec::<RelationshipState>::new(),
        inventory: Vec::<InventoryItem>::new(),
        player: PlayerCharacterState::default(),
        clues: Vec::<ClueState>::new(),
        memories: vec![],
        summary: None,
        recent_events: vec![],
    }
}

fn opening_intro_message(
    session_id: SessionId,
    scenario: &Scenario,
    world_state: &WorldState,
) -> MessageRecord {
    MessageRecord {
        id: Uuid::new_v4(),
        session_id,
        role: MessageRole::System,
        speaker_id: None,
        content: build_opening_intro(scenario, world_state),
        scene_type: None,
        prompt_template_version: None,
        raw_provider_output: None,
    }
}

fn build_opening_intro(scenario: &Scenario, world_state: &WorldState) -> String {
    let projected =
        BasicFrontendStateProjector.project(scenario, world_state, &ViewerContext::player());
    let mut sections = vec![scenario.title.trim().to_string()];

    if !scenario.setting.trim().is_empty() {
        sections.push(format!("Setting\n{}", scenario.setting.trim()));
    }

    let mut opening = Vec::new();
    if let Some(location) = projected.current_location.as_ref() {
        opening.push(format!("You begin in {}.", location.name));
    }
    if let Some(speaker) = projected.active_speaker.as_ref() {
        opening.push(format!("{} is the first visible voice in the scene.", speaker.name));
    }
    if !opening.is_empty() {
        sections.push(format!("Opening\n{}", opening.join(" ")));
    }

    let mut situation = Vec::new();
    if let Some(quest_state) = projected.visible_quests.first() {
        if let Some(quest) = scenario.quests.iter().find(|quest| quest.id == quest_state.id) {
            if quest.description.trim().is_empty() {
                situation.push(format!("Immediate concern: {}.", quest.title));
            } else {
                situation.push(format!(
                    "Immediate concern: {}. {}",
                    quest.title,
                    quest.description.trim()
                ));
            }
        }
    }
    if let Some(clock) = projected.visible_clocks.first() {
        situation.push(format!(
            "Pressure: {} stands at {}/{}.",
            clock.title, clock.current, clock.max
        ));
    }
    if let Some(goal) = projected.player.goals.first() {
        situation.push(format!("Your focus: {}.", goal.label));
    } else if let Some(condition) = projected.player.conditions.first() {
        situation.push(format!("You are carrying {}.", condition.label));
    } else if let Some(resource) = projected.player.resources.first() {
        situation.push(format!(
            "{} currently sits at {} within a range of {} to {}.",
            resource.label, resource.current, resource.min, resource.max
        ));
    }
    if !situation.is_empty() {
        sections.push(format!("Situation\n{}", situation.join(" ")));
    }

    sections.join("\n\n")
}

fn derive_npc_availability(
    status: domain::NpcStatus,
    visible_to_player: bool,
    location_id: Option<&String>,
    starting_location_id: Option<&String>,
) -> NpcAvailability {
    if matches!(
        status,
        domain::NpcStatus::Dead | domain::NpcStatus::Unconscious
    ) {
        return NpcAvailability::Unavailable;
    }
    if !visible_to_player {
        return NpcAvailability::Offscreen;
    }
    if location_id.is_some() && location_id == starting_location_id {
        NpcAvailability::Present
    } else if location_id.is_some() {
        NpcAvailability::Offscreen
    } else {
        NpcAvailability::Nearby
    }
}

/// Backward-compatibility alias. New code should use [`InMemoryApplicationStore`].
pub type ApiStore = InMemoryApplicationStore;

#[cfg(test)]
mod tests {
    use super::initial_world_state;
    use domain::{
        ClockTemplate, Faction, FactionIdentity, Location, MessageRecord, MessageRole, Npc,
        NpcStatus, Quest, RoleIdentity, Scenario, ScenarioType, WorldStateDelta,
    };
    use engine::{TurnStateStore, ValidatedWorldStateDelta};
    use uuid::Uuid;

    fn scenario() -> Scenario {
        Scenario {
            id: Uuid::new_v4(),
            title: "The Bride of the Iron Archduke".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "court intrigue".into(),
            tone: "gothic".into(),
            rules: vec![],
            locations: vec![
                Location {
                    id: "frostmere-citadel".into(),
                    name: "Frostmere Citadel".into(),
                    description: "The opening location.".into(),
                    visible: true,
                },
                Location {
                    id: "winter-orphan-house".into(),
                    name: "Winter Orphan House".into(),
                    description: "A remote refuge.".into(),
                    visible: true,
                },
            ],
            factions: vec![Faction {
                id: "house-falkenrath".into(),
                name: "House Falkenrath".into(),
                description: "Northern rulers.".into(),
                faction_identity: FactionIdentity {
                    public_goal: "protect the north".into(),
                    hidden_goal: None,
                    values: vec!["duty".into()],
                    fears: vec!["betrayal".into()],
                    methods: vec!["discipline".into()],
                },
                initial_standing: 0,
            }],
            npcs: vec![
                Npc {
                    id: "steward-marta".into(),
                    name: "Steward Marta Venn".into(),
                    description: "The opening speaker.".into(),
                    role_identity: RoleIdentity {
                        core_emotion: "guarded courtesy".into(),
                        motivation: "receive the princess correctly".into(),
                        worldview: "service reveals character".into(),
                        fear: None,
                        desire: None,
                        speech_style: "plain".into(),
                        boundaries: vec![],
                        values: vec!["stability".into()],
                    },
                    stats: None,
                    initial_status: NpcStatus::Active,
                    initial_location_id: Some("frostmere-citadel".into()),
                    initial_visible_to_player: true,
                },
                Npc {
                    id: "sister-adela".into(),
                    name: "Sister Adela Thorn".into(),
                    description: "Offstage at the orphan house.".into(),
                    role_identity: RoleIdentity {
                        core_emotion: "steady".into(),
                        motivation: "protect the children".into(),
                        worldview: "mercy matters".into(),
                        fear: None,
                        desire: None,
                        speech_style: "gentle".into(),
                        boundaries: vec![],
                        values: vec!["truth".into()],
                    },
                    stats: None,
                    initial_status: NpcStatus::Active,
                    initial_location_id: Some("winter-orphan-house".into()),
                    initial_visible_to_player: false,
                },
            ],
            quests: vec![Quest {
                id: "arrive".into(),
                title: "Arrive at Frostmere".into(),
                description: "Enter the citadel.".into(),
                objectives: vec![],
                visible: true,
            }],
            secrets: vec![],
            clocks: vec![ClockTemplate {
                id: "wedding".into(),
                title: "The wedding approaches".into(),
                current: 1,
                max: 6,
                consequence: "The date arrives.".into(),
            }],
        }
    }

    #[test]
    fn initial_world_state_respects_npc_authoring_fields() {
        let scenario = scenario();
        let world = initial_world_state(Uuid::new_v4(), &scenario);

        assert_eq!(
            world.current_location_id.as_deref(),
            Some("frostmere-citadel")
        );
        assert_eq!(world.active_speaker_id.as_deref(), Some("steward-marta"));

        let marta = world
            .npcs
            .iter()
            .find(|npc| npc.npc_id == "steward-marta")
            .expect("marta state");
        assert_eq!(marta.location_id.as_deref(), Some("frostmere-citadel"));
        assert!(marta.visible_to_player);

        let adela = world
            .npcs
            .iter()
            .find(|npc| npc.npc_id == "sister-adela")
            .expect("adela state");
        assert_eq!(adela.location_id.as_deref(), Some("winter-orphan-house"));
        assert!(!adela.visible_to_player);
    }

    #[tokio::test]
    async fn timeline_returns_turn_messages_and_world_events_in_order() {
        let store = super::InMemoryApplicationStore::new(true);
        let scenario = scenario();
        let scenario_id = scenario.id;
        store.insert_scenario(scenario);
        let session = store
            .insert_session(scenario_id, "Timeline Session".into())
            .expect("session should exist");

        let user_message = MessageRecord {
            id: Uuid::new_v4(),
            session_id: session.id,
            role: MessageRole::User,
            speaker_id: None,
            content: "I step into the citadel.".into(),
            scene_type: None,
            prompt_template_version: None,
            raw_provider_output: None,
        };
        let assistant_message = MessageRecord {
            id: Uuid::new_v4(),
            session_id: session.id,
            role: MessageRole::Assistant,
            speaker_id: Some("steward-marta".into()),
            content: "Steward Marta inclines her head.".into(),
            scene_type: None,
            prompt_template_version: Some("timeline-test".into()),
            raw_provider_output: None,
        };
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            event_log_entries: vec!["The gates grind open behind the player.".into()],
            ..WorldStateDelta::default()
        });
        let mut updated_state = store
            .snapshot_world_state(session.id)
            .expect("world state should exist");
        updated_state.version = 1;

        TurnStateStore::persist_successful_turn(
            &store,
            user_message.clone(),
            assistant_message.clone(),
            delta,
            updated_state,
        )
        .await
        .expect("turn should persist");

        let timeline = super::ApplicationStore::timeline(&store, session.id)
            .await
            .expect("timeline should load");

        assert_eq!(timeline.len(), 4);
        assert_eq!(timeline[0].kind, "system_message");
        assert_eq!(timeline[1].kind, "user_message");
        assert_eq!(timeline[1].description, user_message.content);
        assert_eq!(timeline[2].kind, "assistant_message");
        assert_eq!(timeline[2].description, assistant_message.content);
        assert_eq!(timeline[2].world_state_version, Some(1));
        assert_eq!(timeline[3].kind, "world_event");
        assert_eq!(
            timeline[3].description,
            "The gates grind open behind the player."
        );
        assert_eq!(timeline[3].world_state_version, Some(1));
    }

    #[tokio::test]
    async fn create_session_persists_opening_system_message_before_turns() {
        let store = super::InMemoryApplicationStore::new(true);
        let scenario = scenario();
        let scenario_id = scenario.id;
        store.insert_scenario(scenario);

        let session = super::ApplicationStore::create_session(
            &store,
            scenario_id,
            "Intro Session".into(),
        )
            .await
            .expect("create session")
            .expect("session should exist");

        let timeline = super::ApplicationStore::timeline(&store, session.id)
            .await
            .expect("timeline should load");
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].kind, "system_message");
        assert_eq!(timeline[0].world_state_version, None);

        let raw_timeline = super::ApplicationStore::raw_timeline(&store, session.id)
            .await
            .expect("raw timeline query")
            .expect("raw timeline should exist");
        assert_eq!(raw_timeline.messages.len(), 1);
        assert_eq!(raw_timeline.messages[0].role, MessageRole::System);
        assert!(raw_timeline.messages[0].scene_type.is_none());
        assert!(raw_timeline.messages[0].raw_provider_output.is_none());
    }

    #[tokio::test]
    async fn opening_system_message_is_excluded_from_recent_prompt_messages() {
        let store = super::InMemoryApplicationStore::new(true);
        let scenario = scenario();
        let scenario_id = scenario.id;
        store.insert_scenario(scenario);

        let session = super::ApplicationStore::create_session(
            &store,
            scenario_id,
            "Prompt Filter".into(),
        )
            .await
            .expect("create session")
            .expect("session should exist");

        let loaded = TurnStateStore::load_turn_state(&store, session.id)
            .await
            .expect("turn state should load");

        assert!(loaded.recent_messages.is_empty());

        let raw_timeline = super::ApplicationStore::raw_timeline(&store, session.id)
            .await
            .expect("raw timeline query")
            .expect("raw timeline should exist");
        assert_eq!(raw_timeline.messages.len(), 1);
        assert_eq!(raw_timeline.messages[0].role, MessageRole::System);
    }
}
