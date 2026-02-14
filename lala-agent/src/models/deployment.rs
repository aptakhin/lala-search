// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use std::env;

/// Deployment mode controlling single-tenant vs multi-tenant operation.
///
/// Single-tenant: One Cassandra keyspace per installation. Default for the open source
/// self-hosted version. No tenant isolation needed.
///
/// Multi-tenant: One Cassandra keyspace per customer. Used in the SaaS/hosted version.
/// Handlers scope database queries to the authenticated tenant's keyspace via
/// `CassandraClient::with_keyspace()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    /// Single installation, one keyspace, no tenant isolation needed.
    /// Default for the open source self-hosted version.
    SingleTenant,
    /// Multiple customers, one keyspace per tenant.
    /// Used in the SaaS/hosted version.
    MultiTenant,
}

impl DeploymentMode {
    /// Parse deployment mode from the DEPLOYMENT_MODE environment variable.
    /// Panics if the variable is not set or has an invalid value.
    pub fn from_env() -> Self {
        let mode =
            env::var("DEPLOYMENT_MODE").expect("DEPLOYMENT_MODE environment variable must be set");
        Self::parse(&mode)
    }

    fn parse(mode: &str) -> Self {
        match mode {
            "single_tenant" => DeploymentMode::SingleTenant,
            "multi_tenant" => DeploymentMode::MultiTenant,
            _ => panic!(
                "DEPLOYMENT_MODE must be 'single_tenant' or 'multi_tenant', got: {}",
                mode
            ),
        }
    }

    pub fn is_multi_tenant(&self) -> bool {
        matches!(self, DeploymentMode::MultiTenant)
    }
}

// Rust enums have no default Display. Manual impl needed to print "single_tenant"/"multi_tenant"
// rather than the Debug form "SingleTenant"/"MultiTenant".
impl std::fmt::Display for DeploymentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentMode::SingleTenant => write!(f, "single_tenant"),
            DeploymentMode::MultiTenant => write!(f, "multi_tenant"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_tenant() {
        assert_eq!(
            DeploymentMode::parse("single_tenant"),
            DeploymentMode::SingleTenant
        );
    }

    #[test]
    fn test_parse_multi_tenant() {
        assert_eq!(
            DeploymentMode::parse("multi_tenant"),
            DeploymentMode::MultiTenant
        );
    }

    #[test]
    #[should_panic(expected = "DEPLOYMENT_MODE must be 'single_tenant' or 'multi_tenant'")]
    fn test_parse_invalid_panics() {
        DeploymentMode::parse("saas");
    }

    #[test]
    fn test_is_multi_tenant_false_for_single() {
        assert!(!DeploymentMode::SingleTenant.is_multi_tenant());
    }

    #[test]
    fn test_is_multi_tenant_true_for_multi() {
        assert!(DeploymentMode::MultiTenant.is_multi_tenant());
    }

    #[test]
    fn test_display_produces_snake_case() {
        assert_eq!(DeploymentMode::SingleTenant.to_string(), "single_tenant");
        assert_eq!(DeploymentMode::MultiTenant.to_string(), "multi_tenant");
    }
}
