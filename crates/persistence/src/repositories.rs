use async_trait::async_trait;
use domain::{
    MessageRecord, MessageRole, Scenario, ScenarioId, SceneReasoningStyle, SessionId, WorldState,
};
use engine::{LoadedTurnState, TurnPipelineError, TurnStateStore, ValidatedWorldStateDelta};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PgPersistence {
    pool: PgPool,
}

impl PgPersistence {
    pub async fn connect(database_url: &str) -> Result<Self, RepoError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), RepoError> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
pub trait ScenarioRepository: Send + Sync {
    async fn create(&self, scenario: Scenario) -> Result<Scenario, RepoError>;
    async fn get(&self, id: ScenarioId) -> Result<Option<Scenario>, RepoError>;
    async fn list(&self) -> Result<Vec<ScenarioSummary>, RepoError>;
    async fn update(&self, scenario: Scenario) -> Result<Scenario, RepoError>;
    async fn delete(&self, id: ScenarioId) -> Result<(), RepoError>;
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn create(
        &self,
        scenario_id: ScenarioId,
        title: String,
    ) -> Result<SessionRecord, RepoError>;
    async fn get(&self, id: SessionId) -> Result<Option<SessionRecord>, RepoError>;
    async fn list(&self) -> Result<Vec<SessionRecord>, RepoError>;
    async fn delete(&self, id: SessionId) -> Result<(), RepoError>;
    async fn set_provider(
        &self,
        session_id: SessionId,
        provider_id: Option<uuid::Uuid>,
    ) -> Result<SessionRecord, RepoError>;
}

#[async_trait]
pub trait WorldStateRepository: Send + Sync {
    async fn get(&self, session_id: SessionId) -> Result<Option<WorldState>, RepoError>;
    async fn save(
        &self,
        state: &WorldState,
        expected_version: Option<i64>,
    ) -> Result<(), RepoError>;
}

#[async_trait]
pub trait MessageRepository: Send + Sync {
    async fn append(&self, message: &MessageRecord) -> Result<(), RepoError>;
    async fn recent(
        &self,
        session_id: SessionId,
        limit: i64,
    ) -> Result<Vec<MessageRecord>, RepoError>;
}

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn append(
        &self,
        session_id: SessionId,
        event_type: &str,
        description: &str,
    ) -> Result<(), RepoError>;
    async fn list(&self, session_id: SessionId) -> Result<Vec<EventRecord>, RepoError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScenarioSummary {
    pub id: ScenarioId,
    pub title: String,
    pub scenario_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: SessionId,
    pub scenario_id: ScenarioId,
    pub title: String,
    pub status: String,
    pub provider_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventRecord {
    pub id: Uuid,
    pub session_id: SessionId,
    pub event_type: String,
    pub description: String,
}

#[async_trait]
impl ScenarioRepository for PgPersistence {
    async fn create(&self, scenario: Scenario) -> Result<Scenario, RepoError> {
        sqlx::query(
            "INSERT INTO scenarios (id, title, scenario_type, definition)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(scenario.id)
        .bind(&scenario.title)
        .bind(format!("{:?}", scenario.scenario_type))
        .bind(sqlx::types::Json(&scenario))
        .execute(&self.pool)
        .await?;
        Ok(scenario)
    }

    async fn get(&self, id: ScenarioId) -> Result<Option<Scenario>, RepoError> {
        let row = sqlx::query("SELECT definition FROM scenarios WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(|row| {
            row.try_get::<sqlx::types::Json<Scenario>, _>("definition")
                .map(|json| json.0)
        })
        .transpose()
        .map_err(RepoError::from)
    }

    async fn list(&self) -> Result<Vec<ScenarioSummary>, RepoError> {
        let rows =
            sqlx::query("SELECT id, title, scenario_type FROM scenarios ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await?;

        rows.into_iter()
            .map(|row| {
                Ok(ScenarioSummary {
                    id: row.try_get("id")?,
                    title: row.try_get("title")?,
                    scenario_type: row.try_get("scenario_type")?,
                })
            })
            .collect()
    }

    async fn update(&self, scenario: Scenario) -> Result<Scenario, RepoError> {
        let result = sqlx::query(
            "UPDATE scenarios
             SET title = $2, scenario_type = $3, definition = $4, updated_at = now()
             WHERE id = $1",
        )
        .bind(scenario.id)
        .bind(&scenario.title)
        .bind(format!("{:?}", scenario.scenario_type))
        .bind(sqlx::types::Json(&scenario))
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            Err(RepoError::NotFound)
        } else {
            Ok(scenario)
        }
    }

    async fn delete(&self, id: ScenarioId) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM scenarios WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl SessionRepository for PgPersistence {
    async fn create(
        &self,
        scenario_id: ScenarioId,
        title: String,
    ) -> Result<SessionRecord, RepoError> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO sessions (id, scenario_id, title) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(scenario_id)
            .bind(&title)
            .execute(&self.pool)
            .await?;
        Ok(SessionRecord {
            id,
            scenario_id,
            title,
            status: "active".into(),
            provider_id: None,
        })
    }

    async fn get(&self, id: SessionId) -> Result<Option<SessionRecord>, RepoError> {
        let row = sqlx::query(
            "SELECT id, scenario_id, title, status, provider_id FROM sessions WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_session).transpose()
    }

    async fn list(&self) -> Result<Vec<SessionRecord>, RepoError> {
        sqlx::query(
            "SELECT id, scenario_id, title, status, provider_id FROM sessions ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(row_to_session)
        .collect()
    }

    async fn delete(&self, id: SessionId) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_provider(
        &self,
        session_id: SessionId,
        provider_id: Option<uuid::Uuid>,
    ) -> Result<SessionRecord, RepoError> {
        let row = sqlx::query(
            "UPDATE sessions
             SET provider_id = $2, updated_at = now()
             WHERE id = $1
             RETURNING id, scenario_id, title, status, provider_id",
        )
        .bind(session_id)
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_session)
            .transpose()?
            .ok_or(RepoError::NotFound)
    }
}

#[async_trait]
impl WorldStateRepository for PgPersistence {
    async fn get(&self, session_id: SessionId) -> Result<Option<WorldState>, RepoError> {
        let row = sqlx::query("SELECT state FROM world_states WHERE session_id = $1")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            row.try_get::<sqlx::types::Json<WorldState>, _>("state")
                .map(|json| json.0)
        })
        .transpose()
        .map_err(RepoError::from)
    }

    async fn save(
        &self,
        state: &WorldState,
        expected_version: Option<i64>,
    ) -> Result<(), RepoError> {
        let result = if let Some(expected_version) = expected_version {
            sqlx::query(
                "UPDATE world_states
                 SET state = $2, version = $3, updated_at = now()
                 WHERE session_id = $1 AND version = $4",
            )
            .bind(state.session_id)
            .bind(sqlx::types::Json(state))
            .bind(state.version)
            .bind(expected_version)
            .execute(&self.pool)
            .await?
        } else {
            sqlx::query(
                "INSERT INTO world_states (session_id, state, version)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (session_id)
                 DO UPDATE SET state = EXCLUDED.state, version = EXCLUDED.version, updated_at = now()",
            )
            .bind(state.session_id)
            .bind(sqlx::types::Json(state))
            .bind(state.version)
            .execute(&self.pool)
            .await?
        };

        if expected_version.is_some() && result.rows_affected() == 0 {
            Err(RepoError::Conflict("world state version mismatch".into()))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl MessageRepository for PgPersistence {
    async fn append(&self, message: &MessageRecord) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO messages
             (id, session_id, role, speaker_id, content, scene_type, prompt_template_version, raw_provider_output)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(message.id)
        .bind(message.session_id)
        .bind(format!("{:?}", message.role))
        .bind(&message.speaker_id)
        .bind(&message.content)
        .bind(message.scene_type.map(|style| format!("{style:?}")))
        .bind(&message.prompt_template_version)
        .bind(message.raw_provider_output.as_ref().map(sqlx::types::Json))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn recent(
        &self,
        session_id: SessionId,
        limit: i64,
    ) -> Result<Vec<MessageRecord>, RepoError> {
        let rows = sqlx::query(
            "SELECT id, session_id, role, speaker_id, content, scene_type, prompt_template_version, raw_provider_output
             FROM messages
             WHERE session_id = $1
             ORDER BY created_at DESC
             LIMIT $2",
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_message).collect()
    }
}

#[async_trait]
impl EventRepository for PgPersistence {
    async fn append(
        &self,
        session_id: SessionId,
        event_type: &str,
        description: &str,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO events (id, session_id, event_type, description)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(Uuid::new_v4())
        .bind(session_id)
        .bind(event_type)
        .bind(description)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list(&self, session_id: SessionId) -> Result<Vec<EventRecord>, RepoError> {
        sqlx::query("SELECT id, session_id, event_type, description FROM events WHERE session_id = $1 ORDER BY created_at")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|row| {
                Ok(EventRecord {
                    id: row.try_get("id")?,
                    session_id: row.try_get("session_id")?,
                    event_type: row.try_get("event_type")?,
                    description: row.try_get("description")?,
                })
            })
            .collect()
    }
}

#[async_trait]
impl TurnStateStore for PgPersistence {
    async fn load_turn_state(
        &self,
        session_id: SessionId,
    ) -> Result<LoadedTurnState, TurnPipelineError> {
        let session = <Self as SessionRepository>::get(self, session_id)
            .await
            .map_err(repo_to_pipeline)?
            .ok_or(TurnPipelineError::NotFound)?;
        let scenario = <Self as ScenarioRepository>::get(self, session.scenario_id)
            .await
            .map_err(repo_to_pipeline)?
            .ok_or(TurnPipelineError::NotFound)?;
        let world_state = <Self as WorldStateRepository>::get(self, session_id)
            .await
            .map_err(repo_to_pipeline)?
            .ok_or(TurnPipelineError::NotFound)?;
        let recent_messages = <Self as MessageRepository>::recent(self, session_id, 6)
            .await
            .map_err(repo_to_pipeline)?;

        Ok(LoadedTurnState {
            scenario,
            world_state,
            recent_messages,
        })
    }

    async fn persist_successful_turn(
        &self,
        user_message: MessageRecord,
        assistant_message: MessageRecord,
        delta: ValidatedWorldStateDelta,
        updated_state: WorldState,
    ) -> Result<(), TurnPipelineError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| TurnPipelineError::Store(error.to_string()))?;

        sqlx::query(
            "INSERT INTO messages
             (id, session_id, role, speaker_id, content, scene_type, prompt_template_version, raw_provider_output)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NULL)",
        )
        .bind(user_message.id)
        .bind(user_message.session_id)
        .bind(format!("{:?}", user_message.role))
        .bind(&user_message.speaker_id)
        .bind(&user_message.content)
        .bind(user_message.scene_type.map(|style| format!("{style:?}")))
        .bind(&user_message.prompt_template_version)
        .execute(&mut *tx)
        .await
        .map_err(|error| TurnPipelineError::Store(error.to_string()))?;

        sqlx::query(
            "INSERT INTO messages
             (id, session_id, role, speaker_id, content, scene_type, prompt_template_version, raw_provider_output)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NULL)",
        )
        .bind(assistant_message.id)
        .bind(assistant_message.session_id)
        .bind(format!("{:?}", assistant_message.role))
        .bind(&assistant_message.speaker_id)
        .bind(&assistant_message.content)
        .bind(assistant_message.scene_type.map(|style| format!("{style:?}")))
        .bind(&assistant_message.prompt_template_version)
        .execute(&mut *tx)
        .await
        .map_err(|error| TurnPipelineError::Store(error.to_string()))?;

        sqlx::query(
            "INSERT INTO world_state_deltas (id, session_id, message_id, delta, validation_status)
             VALUES ($1, $2, $3, $4, 'applied')",
        )
        .bind(Uuid::new_v4())
        .bind(assistant_message.session_id)
        .bind(assistant_message.id)
        .bind(sqlx::types::Json(&delta.0))
        .execute(&mut *tx)
        .await
        .map_err(|error| TurnPipelineError::Store(error.to_string()))?;

        let update_result = sqlx::query(
            "UPDATE world_states
             SET state = $2, version = $3, updated_at = now()
             WHERE session_id = $1 AND version = $4",
        )
        .bind(updated_state.session_id)
        .bind(sqlx::types::Json(&updated_state))
        .bind(updated_state.version)
        .bind(updated_state.version - 1)
        .execute(&mut *tx)
        .await
        .map_err(|error| TurnPipelineError::Store(error.to_string()))?;

        if update_result.rows_affected() == 0 {
            return Err(TurnPipelineError::Store(
                "world state version mismatch".into(),
            ));
        }

        for description in &delta.0.event_log_entries {
            sqlx::query(
                "INSERT INTO events (id, session_id, event_type, description)
                 VALUES ($1, $2, 'world_event', $3)",
            )
            .bind(Uuid::new_v4())
            .bind(updated_state.session_id)
            .bind(description)
            .execute(&mut *tx)
            .await
            .map_err(|error| TurnPipelineError::Store(error.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|error| TurnPipelineError::Store(error.to_string()))?;
        Ok(())
    }

    async fn persist_error_event(
        &self,
        session_id: SessionId,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        <Self as EventRepository>::append(self, session_id, "turn_error", &description)
            .await
            .map_err(repo_to_pipeline)
    }
}

fn row_to_session(row: sqlx::postgres::PgRow) -> Result<SessionRecord, RepoError> {
    Ok(SessionRecord {
        id: row.try_get("id")?,
        scenario_id: row.try_get("scenario_id")?,
        title: row.try_get("title")?,
        status: row.try_get("status")?,
        provider_id: row.try_get("provider_id")?,
    })
}

fn row_to_message(row: sqlx::postgres::PgRow) -> Result<MessageRecord, RepoError> {
    Ok(MessageRecord {
        id: row.try_get("id")?,
        session_id: row.try_get("session_id")?,
        role: parse_message_role(row.try_get::<String, _>("role")?.as_str()),
        speaker_id: row.try_get("speaker_id")?,
        content: row.try_get("content")?,
        scene_type: row
            .try_get::<Option<String>, _>("scene_type")?
            .as_deref()
            .and_then(parse_scene_style),
        prompt_template_version: row.try_get("prompt_template_version")?,
        raw_provider_output: row
            .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("raw_provider_output")?
            .map(|json| json.0),
    })
}

fn parse_message_role(value: &str) -> MessageRole {
    match value {
        "Assistant" => MessageRole::Assistant,
        "System" => MessageRole::System,
        _ => MessageRole::User,
    }
}

fn parse_scene_style(value: &str) -> Option<SceneReasoningStyle> {
    match value {
        "CharacterDialogue" => Some(SceneReasoningStyle::CharacterDialogue),
        "EmotionalScene" => Some(SceneReasoningStyle::EmotionalScene),
        "PoliticalNegotiation" => Some(SceneReasoningStyle::PoliticalNegotiation),
        "MysteryInvestigation" => Some(SceneReasoningStyle::MysteryInvestigation),
        "TacticalCombat" => Some(SceneReasoningStyle::TacticalCombat),
        "WorldSimulation" => Some(SceneReasoningStyle::WorldSimulation),
        "RulesAdjudication" => Some(SceneReasoningStyle::RulesAdjudication),
        "TravelExploration" => Some(SceneReasoningStyle::TravelExploration),
        "Downtime" => Some(SceneReasoningStyle::Downtime),
        "QuestResolution" => Some(SceneReasoningStyle::QuestResolution),
        _ => None,
    }
}

fn repo_to_pipeline(error: RepoError) -> TurnPipelineError {
    match error {
        RepoError::NotFound => TurnPipelineError::NotFound,
        other => TurnPipelineError::Store(other.to_string()),
    }
}

#[derive(Debug, Error)]
pub enum RepoError {
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}
