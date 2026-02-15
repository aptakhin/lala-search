// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 Aleksandr Ptakhin

//! Route handlers for the HTTP API.

pub mod auth;

pub use auth::{auth_router, AuthApiDoc, AuthState};
