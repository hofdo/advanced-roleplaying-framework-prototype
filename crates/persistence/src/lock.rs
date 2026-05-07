use async_trait::async_trait;
use domain::SessionId;
use engine::{SessionTurnLock, TurnLockError, TurnLockGuard};
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct PostgresSessionTurnLock {
    pool: PgPool,
}

impl PostgresSessionTurnLock {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SessionTurnLock for PostgresSessionTurnLock {
    async fn acquire(&self, session_id: SessionId) -> Result<TurnLockGuard, TurnLockError> {
        let rows_updated = sqlx::query(
            "UPDATE sessions
             SET processing_turn = true,
                 processing_turn_started_at = now()
             WHERE id = $1
               AND (
                 processing_turn = false
                 OR processing_turn_started_at < now() - INTERVAL '5 minutes'
               )",
        )
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(|error| TurnLockError::Store(error.to_string()))?
        .rows_affected();

        if rows_updated == 0 {
            return Err(TurnLockError::AlreadyInProgress);
        }

        let pool = self.pool.clone();
        Ok(TurnLockGuard::with_release(move || {
            // Spawn a task to release the lock asynchronously.
            // Drop is synchronous so we use tokio::spawn.
            tokio::spawn(async move {
                let _ = sqlx::query(
                    "UPDATE sessions
                     SET processing_turn = false,
                         processing_turn_started_at = NULL
                     WHERE id = $1",
                )
                .bind(session_id)
                .execute(&pool)
                .await;
            });
        }))
    }
}
