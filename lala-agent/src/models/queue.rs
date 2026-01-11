// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use serde::{Deserialize, Serialize};

/// Request to add a URL to the crawl queue
#[derive(Debug, Serialize, Deserialize)]
pub struct AddToQueueRequest {
    pub url: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    1
}

/// Response after adding to queue
#[derive(Debug, Serialize, Deserialize)]
pub struct AddToQueueResponse {
    pub success: bool,
    pub message: String,
    pub url: String,
    pub domain: String,
}
