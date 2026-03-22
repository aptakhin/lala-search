-- SPDX-License-Identifier: BSD-3-Clause
-- Copyright (c) 2026 Aleksandr Ptakhin
--
-- Track per-email magic link send throttling to prevent abuse.

CREATE TABLE IF NOT EXISTS magic_link_send_attempts (
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    first_attempt_at  TIMESTAMPTZ NOT NULL,
    last_attempt_at   TIMESTAMPTZ NOT NULL,
    blocked_until     TIMESTAMPTZ,
    attempt_count     INTEGER NOT NULL DEFAULT 0,
    email             TEXT PRIMARY KEY
);
