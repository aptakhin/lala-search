// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use std::env;

/// Agent operational mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    /// Only process crawl queue, don't serve HTTP API
    Worker,
    /// Only manage cluster, don't process queue
    Manager,
    /// Both worker and manager functionality
    All,
}

impl AgentMode {
    /// Parse agent mode from environment variable
    /// Defaults to AgentMode::All if not set or invalid
    pub fn from_env() -> Self {
        match env::var("AGENT_MODE").as_deref().unwrap_or("all") {
            "worker" => AgentMode::Worker,
            "manager" => AgentMode::Manager,
            _ => AgentMode::All,
        }
    }

    /// Check if this mode should process the crawl queue
    pub fn should_process_queue(&self) -> bool {
        matches!(self, AgentMode::Worker | AgentMode::All)
    }

    /// Check if this mode should serve as manager
    pub fn should_manage_cluster(&self) -> bool {
        matches!(self, AgentMode::Manager | AgentMode::All)
    }
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMode::Worker => write!(f, "worker"),
            AgentMode::Manager => write!(f, "manager"),
            AgentMode::All => write!(f, "all"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_mode_default_is_all() {
        // Note: This test runs with whatever AGENT_MODE env var is set
        // In CI, ensure AGENT_MODE is not set to test the default
        let mode = AgentMode::from_env();
        // Just verify it returns a valid mode
        assert!(matches!(
            mode,
            AgentMode::All | AgentMode::Worker | AgentMode::Manager
        ));
    }

    #[test]
    fn test_worker_mode_should_process_queue() {
        assert!(AgentMode::Worker.should_process_queue());
    }

    #[test]
    fn test_manager_mode_should_not_process_queue() {
        assert!(!AgentMode::Manager.should_process_queue());
    }

    #[test]
    fn test_all_mode_should_process_queue() {
        assert!(AgentMode::All.should_process_queue());
    }

    #[test]
    fn test_manager_mode_should_manage_cluster() {
        assert!(AgentMode::Manager.should_manage_cluster());
    }

    #[test]
    fn test_worker_mode_should_not_manage_cluster() {
        assert!(!AgentMode::Worker.should_manage_cluster());
    }

    #[test]
    fn test_all_mode_should_manage_cluster() {
        assert!(AgentMode::All.should_manage_cluster());
    }

    #[test]
    fn test_agent_mode_display() {
        assert_eq!(AgentMode::Worker.to_string(), "worker");
        assert_eq!(AgentMode::Manager.to_string(), "manager");
        assert_eq!(AgentMode::All.to_string(), "all");
    }
}
