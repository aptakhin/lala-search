WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM action_history
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM crawl_errors
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM crawl_queue
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM crawled_pages
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM allowed_domains
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM robots_cache
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM settings
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM org_invitations
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants)
   OR email LIKE '%@test.e2e'
   OR invited_by IN (SELECT user_id FROM test_users);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM sessions
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants)
   OR user_id IN (SELECT user_id FROM test_users);

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM magic_link_tokens
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants)
   OR email LIKE '%@test.e2e';

DELETE FROM magic_link_send_attempts
WHERE email LIKE '%@test.e2e';

WITH test_users AS (
    SELECT user_id
    FROM users
    WHERE email LIKE '%@test.e2e'
),
test_tenants AS (
    SELECT tenant_id
    FROM tenants
    WHERE name = 'test.e2e'
    UNION
    SELECT tenant_id
    FROM org_memberships
    WHERE user_id IN (SELECT user_id FROM test_users)
)
DELETE FROM org_memberships
WHERE tenant_id IN (SELECT tenant_id FROM test_tenants)
   OR user_id IN (SELECT user_id FROM test_users);

UPDATE tenants
SET deleted_at = now()
WHERE name = 'test.e2e'
   OR tenant_id IN (
        SELECT tenant_id
        FROM org_memberships
        WHERE user_id IN (
            SELECT user_id
            FROM users
            WHERE email LIKE '%@test.e2e'
        )
   );

DELETE FROM users
WHERE email LIKE '%@test.e2e';
