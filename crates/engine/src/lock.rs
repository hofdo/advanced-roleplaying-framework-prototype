use async_trait::async_trait;
use domain::SessionId;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use thiserror::Error;

#[async_trait]
pub trait SessionTurnLock: Send + Sync {
    async fn acquire(&self, session_id: SessionId) -> Result<TurnLockGuard, TurnLockError>;
}

/// Blanket impl so that `Arc<dyn SessionTurnLock>` can be used wherever
/// a `SessionTurnLock` bound is required (e.g. as a type parameter on
/// `DefaultTurnPipeline`).
#[async_trait]
impl SessionTurnLock for Arc<dyn SessionTurnLock> {
    async fn acquire(&self, session_id: SessionId) -> Result<TurnLockGuard, TurnLockError> {
        (**self).acquire(session_id).await
    }
}

/// A guard that releases the turn lock when dropped.
///
/// Supports both in-memory and persistent (e.g. Postgres) release strategies
/// via a boxed release function that is called synchronously on drop.
pub struct TurnLockGuard {
    release: Option<Box<dyn FnOnce() + Send>>,
}

impl TurnLockGuard {
    /// Create a guard backed by an in-memory `HashSet`.
    pub fn in_memory(
        session_id: SessionId,
        locked_sessions: Arc<Mutex<HashSet<SessionId>>>,
    ) -> Self {
        Self {
            release: Some(Box::new(move || {
                if let Ok(mut locked) = locked_sessions.lock() {
                    locked.remove(&session_id);
                }
            })),
        }
    }

    /// Create a guard that runs an arbitrary release function on drop.
    /// Used by the Postgres lock to spawn a blocking reset of `processing_turn`.
    pub fn with_release(release: impl FnOnce() + Send + 'static) -> Self {
        Self {
            release: Some(Box::new(release)),
        }
    }
}

impl std::fmt::Debug for TurnLockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnLockGuard").finish_non_exhaustive()
    }
}

impl Drop for TurnLockGuard {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release();
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct InMemorySessionTurnLock {
    locked_sessions: Arc<Mutex<HashSet<SessionId>>>,
}

#[async_trait]
impl SessionTurnLock for InMemorySessionTurnLock {
    async fn acquire(&self, session_id: SessionId) -> Result<TurnLockGuard, TurnLockError> {
        let mut locked = self
            .locked_sessions
            .lock()
            .map_err(|_| TurnLockError::Poisoned)?;
        if !locked.insert(session_id) {
            return Err(TurnLockError::AlreadyInProgress);
        }
        Ok(TurnLockGuard::in_memory(
            session_id,
            Arc::clone(&self.locked_sessions),
        ))
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TurnLockError {
    #[error("turn already in progress")]
    AlreadyInProgress,
    #[error("turn lock was poisoned")]
    Poisoned,
    #[error("turn lock store error: {0}")]
    Store(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn rejects_second_lock_for_same_session_until_guard_drops() {
        let lock = InMemorySessionTurnLock::default();
        let session_id = Uuid::new_v4();
        let first = lock.acquire(session_id).await.expect("first lock");

        let second = lock
            .acquire(session_id)
            .await
            .expect_err("second lock rejected");
        assert_eq!(second, TurnLockError::AlreadyInProgress);

        drop(first);
        lock.acquire(session_id)
            .await
            .expect("lock released by drop");
    }
}
