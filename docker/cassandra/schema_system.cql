-- SPDX-License-Identifier: BSD-3-Clause
-- Copyright (c) 2026 Aleksandr Ptakhin

-- System keyspace for LalaSearch - stores global tenant registry
-- This keyspace is shared across all deployments.
-- Note: SYSTEM_KEYSPACE_NAME is replaced at runtime
CREATE KEYSPACE IF NOT EXISTS ${SYSTEM_KEYSPACE_NAME}
WITH replication = {
    'class': 'SimpleStrategy',
    'replication_factor': 1
};

USE ${SYSTEM_KEYSPACE_NAME};

-- Global tenant registry
-- Single-tenant mode: always one row with tenant_id = 'default'
-- Multi-tenant mode: one row per customer, inserted during tenant provisioning
CREATE TABLE IF NOT EXISTS tenants (
    tenant_id text PRIMARY KEY,
    name text,
    created_at timestamp
) WITH comment = 'Global tenant registry. Single-tenant: one default row. Multi-tenant: one row per customer.';
