#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineEventKind {
    TurnStarted,
    TurnLockAcquired,
    ContextBuilt,
    ProviderCalled,
    ProviderResponded,
    DeltaApplied,
    FrontendStateProjected,
    TurnFinished,
    TurnLockReleasing,
    ProviderUsageCaptured,
}

impl PipelineEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TurnStarted => "turn_started",
            Self::TurnLockAcquired => "turn_lock_acquired",
            Self::ContextBuilt => "context_built",
            Self::ProviderCalled => "provider_called",
            Self::ProviderResponded => "provider_responded",
            Self::DeltaApplied => "delta_applied",
            Self::FrontendStateProjected => "frontend_state_projected",
            Self::TurnFinished => "turn_finished",
            Self::TurnLockReleasing => "turn_lock_releasing",
            Self::ProviderUsageCaptured => "provider_usage_captured",
        }
    }
}
