use async_trait::async_trait;
use domain::{
    ClockState, Fact, FactSource, FactVisibility, FactionState, InventoryItem, MessageRecord,
    NpcState, QuestState, QuestStatus, RelationshipState, Scenario, ScenarioId, SessionId,
    WorldState,
};
use engine::{LoadedTurnState, TurnPipelineError, TurnStateStore, ValidatedWorldStateDelta};
use std::{collections::HashMap, sync::Mutex};
use uuid::Uuid;

use crate::{
    EventRecord, EventRepository, PgPersistence, ProviderConfigRepository, ProviderRecord,
    RepoError, ScenarioRepository, SessionRecord, SessionRepository, WorldStateRepository,
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

    pub fn insert_session(
        &self,
        scenario_id: ScenarioId,
        title: String,
    ) -> Option<SessionRecord> {
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
        inner.sessions.insert(id, session.clone());
        inner.world_states.insert(id, world_state);
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
        if !self.store_raw_provider_output {
            assistant_message.raw_provider_output = None;
        }
        let mut inner = self.inner.lock().expect("application store mutex");
        inner
            .messages
            .entry(updated_state.session_id)
            .or_default()
            .extend([user_message, assistant_message.clone()]);
        inner
            .world_states
            .insert(updated_state.session_id, updated_state);
        for description in delta.0.event_log_entries {
            let session_id = assistant_message.session_id;
            inner
                .events
                .entry(session_id)
                .or_default()
                .push(EventRecord {
                    id: Uuid::new_v4(),
                    session_id,
                    event_type: "world_event".into(),
                    description,
                });
        }
        Ok(())
    }

    async fn persist_error_event(
        &self,
        session_id: SessionId,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        self.inner
            .lock()
            .expect("application store mutex")
            .events
            .entry(session_id)
            .or_default()
            .push(EventRecord {
                id: Uuid::new_v4(),
                session_id,
                event_type: "turn_error".into(),
                description,
            });
        Ok(())
    }

    async fn persist_pipeline_event(
        &self,
        session_id: SessionId,
        event_type: &'static str,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        self.inner
            .lock()
            .expect("application store mutex")
            .events
            .entry(session_id)
            .or_default()
            .push(EventRecord {
                id: Uuid::new_v4(),
                session_id,
                event_type: event_type.into(),
                description,
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
                visible_to_player: true,
                location_id: scenario
                    .locations
                    .first()
                    .map(|location| location.id.clone()),
                attitude_to_player: None,
                known_facts: vec![],
                notes: vec![],
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
        relationships: Vec::<RelationshipState>::new(),
        inventory: Vec::<InventoryItem>::new(),
        summary: None,
        recent_events: vec![],
    }
}

/// Backward-compatibility alias. New code should use [`InMemoryApplicationStore`].
pub type ApiStore = InMemoryApplicationStore;
