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
}
