// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct VersionResponse {
    pub agent: String,
    pub version: String,
    pub deployment_mode: String,
}
