// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Type of action performed on an entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Create,
    Edit,
    Delete,
    Rollback,
}

impl fmt::Display for ActionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionType::Create => write!(f, "create"),
            ActionType::Edit => write!(f, "edit"),
            ActionType::Delete => write!(f, "delete"),
            ActionType::Rollback => write!(f, "rollback"),
        }
    }
}

impl ActionType {
    pub fn parse(s: &str) -> Self {
        match s {
            "create" => ActionType::Create,
            "edit" => ActionType::Edit,
            "delete" => ActionType::Delete,
            "rollback" => ActionType::Rollback,
            other => panic!("Unknown action type: {other}"),
        }
    }
}

/// Type of entity the action was performed on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    AllowedDomain,
    Setting,
    OrgMembership,
}

impl fmt::Display for EntityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntityType::AllowedDomain => write!(f, "allowed_domain"),
            EntityType::Setting => write!(f, "setting"),
            EntityType::OrgMembership => write!(f, "org_membership"),
        }
    }
}

impl EntityType {
    pub fn parse(s: &str) -> Self {
        match s {
            "allowed_domain" => EntityType::AllowedDomain,
            "setting" => EntityType::Setting,
            "org_membership" => EntityType::OrgMembership,
            other => panic!("Unknown entity type: {other}"),
        }
    }
}

/// A recorded action that can be rolled back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub action_id: Uuid,
    pub tenant_id: Uuid,
    pub performed_by: Option<Uuid>,
    pub performed_at: DateTime<Utc>,
    pub rolled_back_at: Option<DateTime<Utc>>,
    pub rollback_of: Option<Uuid>,
    pub entity_type: String,
    pub action_type: String,
    pub entity_id: String,
    pub before_state: Option<serde_json::Value>,
    pub after_state: Option<serde_json::Value>,
    pub description: String,
}

/// API response: last undoable action (for Ctrl+Z).
#[derive(Debug, Serialize, Deserialize)]
pub struct LastUndoableResponse {
    pub action: Option<ActionRecord>,
}

/// API response after a rollback.
#[derive(Debug, Serialize, Deserialize)]
pub struct RollbackResponse {
    pub success: bool,
    pub message: String,
    pub rolled_back_action: ActionRecord,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_type_display_roundtrip_create() {
        assert_eq!(ActionType::Create.to_string(), "create");
        assert_eq!(ActionType::parse("create"), ActionType::Create);
    }

    #[test]
    fn test_action_type_display_roundtrip_edit() {
        assert_eq!(ActionType::Edit.to_string(), "edit");
        assert_eq!(ActionType::parse("edit"), ActionType::Edit);
    }

    #[test]
    fn test_action_type_display_roundtrip_delete() {
        assert_eq!(ActionType::Delete.to_string(), "delete");
        assert_eq!(ActionType::parse("delete"), ActionType::Delete);
    }

    #[test]
    fn test_action_type_display_roundtrip_rollback() {
        assert_eq!(ActionType::Rollback.to_string(), "rollback");
        assert_eq!(ActionType::parse("rollback"), ActionType::Rollback);
    }

    #[test]
    fn test_entity_type_display_roundtrip_allowed_domain() {
        assert_eq!(EntityType::AllowedDomain.to_string(), "allowed_domain");
        assert_eq!(
            EntityType::parse("allowed_domain"),
            EntityType::AllowedDomain
        );
    }

    #[test]
    fn test_entity_type_display_roundtrip_setting() {
        assert_eq!(EntityType::Setting.to_string(), "setting");
        assert_eq!(EntityType::parse("setting"), EntityType::Setting);
    }

    #[test]
    fn test_entity_type_display_roundtrip_org_membership() {
        assert_eq!(EntityType::OrgMembership.to_string(), "org_membership");
        assert_eq!(
            EntityType::parse("org_membership"),
            EntityType::OrgMembership
        );
    }
}
