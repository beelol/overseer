//! Owner-approved Audio Mode vocabulary. Packs resolve these meanings; they
//! cannot introduce notifications. Voice's nonspoken Heard signal is separate.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Line {
    AgentStarted,
    AgentComplete,
    AgentPermissionRequired,
    AgentReplyRequired,
    AgentSignInRequired,
    AgentCannotContinue,
    AgentFailed,
    AgentStoppedUnexpectedly,
    AgentsNeedAttention,
    SwarmInitiated,
    SwarmComplete,
    SwarmNeedsAttention,
}

impl Line {
    pub(crate) const ALL: [Self; 12] = [
        Self::AgentStarted, Self::AgentComplete, Self::AgentPermissionRequired,
        Self::AgentReplyRequired, Self::AgentSignInRequired, Self::AgentCannotContinue,
        Self::AgentFailed, Self::AgentStoppedUnexpectedly, Self::AgentsNeedAttention,
        Self::SwarmInitiated, Self::SwarmComplete, Self::SwarmNeedsAttention,
    ];

    pub(crate) fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|line| line.key() == key)
    }

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::AgentStarted => "agent_started",
            Self::AgentComplete => "agent_complete",
            Self::AgentPermissionRequired => "agent_permission_required",
            Self::AgentReplyRequired => "agent_reply_required",
            Self::AgentSignInRequired => "agent_sign_in_required",
            Self::AgentCannotContinue => "agent_cannot_continue",
            Self::AgentFailed => "agent_failed",
            Self::AgentStoppedUnexpectedly => "agent_stopped_unexpectedly",
            Self::AgentsNeedAttention => "agents_need_attention",
            Self::SwarmInitiated => "swarm_initiated",
            Self::SwarmComplete => "swarm_complete",
            Self::SwarmNeedsAttention => "swarm_needs_attention",
        }
    }

    pub(crate) const fn phrase(self) -> &'static str {
        match self {
            Self::AgentStarted => "Agent started.",
            Self::AgentComplete => "Agent complete.",
            Self::AgentPermissionRequired => "Agent needs permission.",
            Self::AgentReplyRequired => "Agent needs a reply.",
            Self::AgentSignInRequired => "Agent needs you to sign in.",
            Self::AgentCannotContinue => "Agent cannot continue.",
            Self::AgentFailed => "Agent failed.",
            Self::AgentStoppedUnexpectedly => "Agent stopped unexpectedly.",
            Self::AgentsNeedAttention => "Several agents need attention.",
            Self::SwarmInitiated => "Swarm initiated.",
            Self::SwarmComplete => "Swarm complete.",
            Self::SwarmNeedsAttention => "Swarm needs attention.",
        }
    }

    pub(crate) const fn urgent(self) -> bool {
        !matches!(self, Self::AgentStarted | Self::AgentComplete
            | Self::SwarmInitiated | Self::SwarmComplete)
    }
}
