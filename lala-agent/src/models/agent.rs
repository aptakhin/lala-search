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
    /// Panics if AGENT_MODE is not set or invalid
    pub fn from_env() -> Self {
        let mode = env::var("AGENT_MODE").expect("AGENT_MODE environment variable must be set");
        Self::parse(&mode)
    }

    /// Parse agent mode from string
    /// Panics if the value is invalid
    fn parse(mode: &str) -> Self {
        match mode {
            "worker" => AgentMode::Worker,
            "manager" => AgentMode::Manager,
            "all" => AgentMode::All,
            _ => panic!(
                "AGENT_MODE must be 'worker', 'manager', or 'all', got: {}",
                mode
            ),
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
    fn test_agent_mode_parse_all() {
        let mode = AgentMode::parse("all");
        assert_eq!(mode, AgentMode::All);
    }

    #[test]
    fn test_agent_mode_parse_worker() {
        let mode = AgentMode::parse("worker");
        assert_eq!(mode, AgentMode::Worker);
    }

    #[test]
    fn test_agent_mode_parse_manager() {
        let mode = AgentMode::parse("manager");
        assert_eq!(mode, AgentMode::Manager);
    }

    #[test]
    #[should_panic(expected = "AGENT_MODE must be 'worker', 'manager', or 'all'")]
    fn test_agent_mode_parse_invalid() {
        AgentMode::parse("invalid");
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
