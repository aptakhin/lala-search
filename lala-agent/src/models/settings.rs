// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use serde::{Deserialize, Serialize};

/// Request to set crawling enabled status
#[derive(Debug, Deserialize, Serialize)]
pub struct SetCrawlingEnabledRequest {
    pub enabled: bool,
}

/// Response for crawling enabled status
#[derive(Debug, Serialize, Deserialize)]
pub struct CrawlingEnabledResponse {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
}

/// Request to set the maximum indexed document capacity for a tenant.
#[derive(Debug, Deserialize, Serialize)]
pub struct SetIndexCapacityRequest {
    pub max_bytes: i64,
}

/// Response describing the tenant's current indexed document usage and maximum.
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexCapacityResponse {
    pub usage_bytes: i64,
    pub max_bytes: i64,
    pub limit_reached: bool,
    pub can_edit_max: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
}
