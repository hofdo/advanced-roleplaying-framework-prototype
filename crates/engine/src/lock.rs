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

#[derive(Debug)]
pub struct TurnLockGuard {
    session_id: SessionId,
    locked_sessions: Arc<Mutex<HashSet<SessionId>>>,
}

impl Drop for TurnLockGuard {
    fn drop(&mut self) {
        if let Ok(mut locked) = self.locked_sessions.lock() {
            locked.remove(&self.session_id);
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
        Ok(TurnLockGuard {
            session_id,
            locked_sessions: Arc::clone(&self.locked_sessions),
        })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TurnLockError {
    #[error("turn already in progress")]
    AlreadyInProgress,
    #[error("turn lock was poisoned")]
    Poisoned,
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
