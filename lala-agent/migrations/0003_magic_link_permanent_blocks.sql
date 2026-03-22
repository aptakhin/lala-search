-- SPDX-License-Identifier: BSD-3-Clause
-- Copyright (c) 2026 Aleksandr Ptakhin
--
-- Add lifetime unverified-attempt tracking for permanent magic link blocks.

ALTER TABLE magic_link_send_attempts
    ADD COLUMN IF NOT EXISTS total_unverified_attempt_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE magic_link_send_attempts
    ADD COLUMN IF NOT EXISTS permanently_blocked_at TIMESTAMPTZ;

UPDATE magic_link_send_attempts
SET total_unverified_attempt_count = attempt_count
WHERE total_unverified_attempt_count = 0;
