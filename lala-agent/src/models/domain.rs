// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use serde::{Deserialize, Serialize};

/// Request to add an allowed domain
#[derive(Debug, Deserialize, Serialize)]
pub struct AddDomainRequest {
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Response after adding an allowed domain
#[derive(Debug, Serialize)]
pub struct AddDomainResponse {
    pub success: bool,
    pub message: String,
    pub domain: String,
}

/// Response for listing allowed domains
#[derive(Debug, Serialize)]
pub struct ListDomainsResponse {
    pub domains: Vec<DomainInfo>,
    pub count: usize,
}

/// Information about an allowed domain
#[derive(Debug, Serialize)]
pub struct DomainInfo {
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Response after deleting an allowed domain
#[derive(Debug, Serialize)]
pub struct DeleteDomainResponse {
    pub success: bool,
    pub message: String,
    pub domain: String,
}
