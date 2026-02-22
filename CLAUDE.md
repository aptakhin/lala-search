# Claude Code Guidelines for LalaSearch

## Critical Rules

### 1. ASK QUESTIONS FIRST

**IMPORTANT**: Don't blindly execute requests. Always think critically about the best solution.

Ask clarifying questions when:
1. **Multiple approaches exist**: The request could be solved in different ways
2. **Potential conflicts**: The request might conflict with existing patterns or architecture
3. **Sub-optimal solution**: You see a better approach than what was requested
4. **Missing context**: You need more information to provide the best solution
5. **Trade-offs involved**: The solution has important implications to discuss
6. **Unclear requirements**: The request is ambiguous or could be interpreted differently

**Examples:**

**Good** - Asking when there's a better approach:
- User: "Add a new config file for the version"
- Assistant: "I notice the version is currently hardcoded. Would you prefer to extract it from Cargo.toml using a build script instead? That would keep it in sync automatically."

**Good** - Asking about conflicts:
- User: "Let's use JSON for configuration"
- Assistant: "I see we're using TOML for Cargo.toml. Should we use TOML for consistency, or is there a specific reason to prefer JSON?"

**Bad** - Executing without thinking:
- User: "Create a new file for every route handler"
- Assistant: *Creates files without asking if organizing by feature or by type would be better*

**Principle**: You're not just a code executor - you're a development partner. Use your knowledge to suggest better alternatives, point out potential issues, and clarify ambiguous requirements.

**Balance**: Ask questions when they add value, but don't over-question trivial decisions.

### 2. ALWAYS COMMIT CHANGES

**Every completed task must end with a git commit. Never consider work done until it is committed!**

## Technology Choices: Open Source Solutions Only

**CRITICAL**: LalaSearch is a fully open source project. All dependencies and technical solutions must be open source.

### Why Open Source

1. **Transparency**: Anyone can audit the code and security
2. **Community support**: Active communities provide better long-term support
3. **No vendor lock-in**: Freedom to modify, fork, or migrate
4. **Cost**: No licensing fees or proprietary restrictions
5. **Licensing compliance**: Ensures the project remains distributable under its open source license

### GOOD - Open Source Solutions
- **PostgreSQL** - Open source relational database
- **Redis** - Open source in-memory data store
- **Elasticsearch** - Open source search and analytics engine
- **Apache Kafka** - Open source event streaming platform
- **Meilisearch** - Open source search engine
- **SeaweedFS** - Open source object storage (S3-compatible, Apache 2.0 license)
- **Docker** - Open source containerization platform
- **Prometheus** - Open source monitoring and metrics

### AVOID - Proprietary/Closed Source Solutions
- **ScyllaDB** - Changed from AGPL to proprietary "source-available" license in Dec 2024 (use PostgreSQL instead)
- **MinIO** - Abandoned open source community in Feb 2026, pushing users to proprietary AIStor (use SeaweedFS instead)
- **DataStax Astra** - Proprietary managed database service
- **Splunk** - Proprietary log aggregation and analysis
- **New Relic** - Proprietary application monitoring
- **Datadog** - Proprietary infrastructure monitoring

### When Open Source Isn't Available

If no viable open source solution exists:
1. **Ask first**: Before choosing a proprietary tool, discuss with the team
2. **Document the decision**: Add a comment explaining why open source wasn't suitable
3. **Minimize impact**: Keep proprietary tools isolated to specific components
4. **Plan migration path**: Document how to eventually switch to an open source alternative

## Test-Driven Development (TDD) Workflow

All features follow a strict TDD cycle to ensure code quality, maintainability, and correctness.

### TDD Cycle

```
1. Analyze → 2. Red → 3. Green → 4. Refactor → (repeat)
```

#### 1. Analyze Phase
Before writing any code, identify and document:
- **Corner cases**: Edge conditions that might break the feature
- **Error scenarios**: Invalid inputs, network failures, resource exhaustion
- **Happy path**: Expected normal operation
- **Boundary conditions**: Limits, empty states, maximum values
- **Concurrency issues**: Race conditions, deadlocks (for async code)

#### 2. Red Phase (Failing Test)
- Write a test that captures ONE specific behavior or corner case
- The test MUST fail initially (compile errors count as failures)
- Keep tests focused and isolated
- Use descriptive test names: `test_<scenario>_<expected_outcome>`

#### 3. Green Phase (Minimal Implementation)
- Write the MINIMUM code to make the test pass
- Don't worry about perfection or optimization
- Avoid implementing features not covered by current tests
- Run tests frequently: `cargo test`

#### 4. Refactor Phase (Optional)
- Improve code quality without changing behavior
- Extract duplicated code
- Improve naming and structure
- Tests must still pass after refactoring
- Skip if code is already clean

## Code Quality Standards

### Formatting
- **Tool**: `rustfmt`
- **Command**: `cargo fmt`
- **Enforcement**: Runs automatically in pre-commit hook

### Linting
- **Tool**: `clippy`
- **Command**: `cargo clippy -- -D warnings`
- **Standard**: All warnings treated as errors

### Testing Tiers

LalaSearch uses a **two-tier testing strategy** to balance fast feedback with comprehensive coverage:

#### Tier 1: Pre-Commit Tests (Fast, < 500ms per test)

Run automatically by `./scripts/pre-commit.sh`. Must complete quickly.

| Test Type | Location | Marker | Description |
|-----------|----------|--------|-------------|
| Unit tests | `src/**/*.rs` | None | Pure logic, no external dependencies |
| Storage-dependent | `src/**/*.rs` | `#[ignore]` | Require PostgreSQL/SeaweedFS/Meilisearch |

**Rules for Tier 1 tests:**
- Each test must complete in **under 500ms**
- Tests requiring external services use `#[ignore]` attribute
- Pre-commit script starts Docker services automatically
- If a test takes longer, move it to Tier 2

**Commands:**
```bash
cargo test --lib                    # Unit tests only
cargo test --lib -- --ignored       # Storage-dependent tests only
./scripts/pre-commit.sh             # Both (starts Docker services)
```

#### Tier 2: Integration & E2E Tests (Slower, CI-focused)

Run in CI pipelines or manually. Can take longer.

| Test Type | Location | Description |
|-----------|----------|-------------|
| Integration | `tests/*.rs` | Multi-component workflows |
| End-to-end | `tests/*.rs` | Full system scenarios |

**Rules for Tier 2 tests:**
- Located in `tests/` directory (not `src/`)
- Can take multiple seconds or longer
- Run in CI pipelines, not pre-commit
- Test complete workflows and cross-service interactions

**Commands:**
```bash
cargo test --test '*'               # All integration tests
cargo test --test queue_processor   # Specific integration test
```

#### Deciding Which Tier

```
Is test < 500ms?
├── YES → Can it run without external services?
│         ├── YES → Unit test (no marker)
│         └── NO  → Storage-dependent test (#[ignore])
└── NO  → Integration/E2E test (tests/ directory)
```

### Coverage
- **Unit tests**: Co-located with code in same file
- **Integration tests**: In `tests/` directory
- **Coverage goal**: High coverage, especially for critical paths

## Pre-Commit Workflow

Before every commit, run:
```bash
./scripts/pre-commit.sh
```

This runs:
1. `cargo fmt --check` (formatting)
2. `cargo clippy -- -D warnings` (linting)
3. `cargo test --lib` (unit tests)
4. `docker compose up -d` + `cargo test --lib -- --ignored` (storage-dependent tests)

The script automatically starts Docker services (PostgreSQL, SeaweedFS, Meilisearch) for storage-dependent tests.

### Windows Support

On Windows (Git Bash / MSYS2), the script automatically runs **all checks inside Docker** via `docker compose run lala-agent`. This avoids PDB linker errors and other Windows-specific build issues. Docker Compose handles networking, env vars, and dependency startup automatically.

```bash
./scripts/pre-commit.sh           # Auto-detect: Docker on Windows, local on Linux/macOS
./scripts/pre-commit.sh --docker  # Force Docker mode (any OS)
./scripts/pre-commit.sh --local   # Force local Rust toolchain
```

If any check fails, fix the issues before committing.

## Completing Features

**Every feature MUST be completed with**:
1. Update `README.md` if project structure changed (new files, directories, tests, status)
2. Update any relevant files in `docs/` — keep architecture docs, env vars, and feature descriptions in sync with the code
3. Run `./scripts/pre-commit.sh`
4. Commit: `git add . && git commit -m "feat: description"`

**Never consider a feature complete until it is committed!**

## Project Structure

```
lalasearch/
├── CLAUDE.md                     # This file - all guidelines
├── docs/
│   ├── overview.md               # Project vision and architecture
│   ├── docker.md                 # Docker setup and usage guide
│   └── versioning.md             # Version management
├── lala-agent/                   # Core agent implementation
│   ├── src/
│   │   ├── main.rs               # HTTP server entry point
│   │   ├── lib.rs                # Library root
│   │   ├── models/               # Data models
│   │   └── services/             # Business logic
│   ├── tests/                    # Integration tests
│   ├── Dockerfile                # Container image definition
│   └── Cargo.toml                # Rust dependencies
├── docker/                       # Docker configuration
│   └── postgres/
│       └── schema.sql            # PostgreSQL schema (all tables, RLS policies)
├── docker-compose.yml            # Multi-container setup
├── .env.example                  # Environment variables template
└── scripts/
    └── pre-commit.sh             # Pre-commit validation script
```

## Key Principles

1. Tests before code
2. High code quality (zero clippy warnings)
3. Proper formatting (rustfmt)
4. Complete features with commits
5. Document architectural decisions

## Error Handling & Data Integrity

**Never assume what's optional!** Treat all operations as critical for downstream processes.

- If a step fails (storage, database, search indexing), fail the entire operation
- Return failed items to queue with `attempt_count += 1` for retry
- Log errors to dedicated error tables for observability
- Don't silently skip failures or treat them as "non-critical"

### Preserve Root Cause in Error Chains

**CRITICAL**: Never wrap errors with generic context that hides the root cause. Use `{:#}` (anyhow full-chain display) in logs and error responses so the original error is always visible.

**BAD** - Generic context hides the real error:
```rust
// Log shows: "Failed to send email" — useless for debugging
self.transport.send(email).await.context("Failed to send email")?;
eprintln!("[EMAIL] Failed: {}", e);  // Only shows outermost context
```

**GOOD** - Context includes diagnostic info, logs show full chain:
```rust
// Log shows: "SMTP send failed (host=smtp.example.com:587, tls=true): Connection refused"
self.transport.send(email).await.with_context(|| {
    format!("SMTP send failed (host={}:{}, tls={})", host, port, tls)
})?;
eprintln!("[EMAIL] Failed: {:#}", e);  // {:#} shows full error chain
```

**Rules**:
1. Use `{:#}` for error logging so the full chain is visible (not just the outermost `.context()`)
2. Add operational context (host, port, IDs) to `.with_context()` — not just "Failed to do X"
3. Don't strip or obscure errors before they reach the logs — the root cause must be debuggable from log output alone

## Logging Guidelines

**CRITICAL**: Always log important actions with sufficient context for debugging and auditing.

### What to Log

1. **User actions with user_id**:
   - User sign-ins (magic link verification, invitation acceptance)
   - User creation (new account, invited user)
   - User sign-outs
   - Permission-sensitive operations (invitations, membership changes)

2. **Email operations**:
   - Email send attempts (both success and failure)
   - Magic link generation
   - Invitation generation

3. **Critical operations**:
   - Database failures
   - Storage operations
   - Search indexing
   - Queue processing

### How to Log

**Current approach**: Use `println!` for info logs and `eprintln!` for errors/warnings.

```rust
// Success - use println!
println!("[SERVICE] Action description: key=value, user_id={}", user_id);

// Error - use eprintln!
eprintln!("[SERVICE] Error description: key=value, error={}", error);
```

**Log format**:
- Use prefixes: `[AUTH]`, `[EMAIL]`, `[DB]`, `[STORAGE]`, etc.
- Include key-value pairs for structured data
- Always include `user_id` when available
- Include contextual information (tenant_id, ip_address, etc.)

### Email Anonymization

**CRITICAL**: Never log full email addresses. Always anonymize them for privacy.

Use the `anonymize_email()` helper from `services::logging`:

```rust
use crate::services::logging::anonymize_email;

// GOOD - Anonymized email
println!("[AUTH] User signed in: user_id={}, email={}", user_id, anonymize_email(&email));
// Output: [AUTH] User signed in: user_id=123..., email=a***@example.com

// BAD - Full email exposed
println!("[AUTH] User signed in: user_id={}, email={}", user_id, email);
// Output: [AUTH] User signed in: user_id=123..., email=alice@example.com
```

### Examples

**Good logging examples**:

```rust
// User creation
println!(
    "[AUTH] New user created: user_id={}, email={}, tenant={}, role=Owner",
    user_id,
    anonymize_email(email),
    tenant_id
);

// Email sending
println!(
    "[EMAIL] Sending invitation to {} for org '{}' from {}",
    anonymize_email(to_email),
    org_name,
    anonymize_email(inviter_email)
);

// Sign-in with context
println!(
    "[AUTH] User signed in via magic link: user_id={}, email={}, tenant={}{}",
    user.user_id,
    anonymize_email(&user.email),
    tenant_id,
    ip_address.map(|ip| format!(", ip={}", ip)).unwrap_or_default()
);

// Error with context
eprintln!(
    "[EMAIL] Failed to send magic link to {}: {}",
    anonymize_email(to_email),
    error
);
```

### What NOT to Log

- Full email addresses (always use `anonymize_email()`)
- Raw tokens or passwords
- Session tokens or hashes
- Sensitive personal information
- Credit card numbers or payment details

## Database Query Optimization: Avoid N+1 Queries

**CRITICAL**: Always check if you're introducing N+1 query patterns. While they may work fine on tiny loads, they cause severe performance degradation at scale.

### What is an N+1 Query?

An N+1 query pattern occurs when you:
1. Query for N items (e.g., list of members)
2. Loop through results and make 1 additional query per item (e.g., fetch each user's email)
3. Result: 1 + N total queries instead of 1-2 queries

### BAD - N+1 Query Pattern

```rust
// Fetch members (1 query)
let members = db.get_org_members(tenant_id).await?;

// Loop and query for each member (N queries)
for member in members {
    let user = db.get_user_by_id(member.user_id).await?;  // ❌ N queries!
    emails.push(user.email);
}
// Total: 1 + N queries
```

### GOOD - Batch Query Pattern

```rust
// Fetch members (1 query)
let members = db.get_org_members(tenant_id).await?;

// Collect all user IDs
let user_ids: Vec<Uuid> = members.iter().map(|m| m.user_id).collect();

// Batch fetch all users in ONE query using IN clause (1 query)
let users = db.get_users_by_ids(user_ids).await?;  // ✅ Single batch query!

// Create lookup map for O(1) access
let email_map: HashMap<Uuid, String> = users
    .into_iter()
    .map(|u| (u.user_id, u.email))
    .collect();

// Total: 2 queries (constant, not N+2)
```

### When to Use Batch Queries

- Fetching related data for multiple items (users, metadata, etc.)
- Loading child objects for parent objects
- Resolving foreign keys or references
- Any loop that makes database queries

### PostgreSQL Tips

PostgreSQL supports JOINs, so prefer them over N+1 queries:

1. **JOIN for related data**:
   ```sql
   SELECT m.user_id, m.role, u.email
   FROM org_memberships m
   JOIN users u ON u.user_id = m.user_id
   WHERE m.tenant_id = $1
   ```

2. **IN clause for batch primary key lookups**:
   ```sql
   SELECT * FROM users WHERE user_id = ANY($1)
   ```

3. **Subqueries** (when JOINs are awkward):
   ```sql
   SELECT * FROM users WHERE user_id IN (
     SELECT user_id FROM org_memberships WHERE tenant_id = $1
   )
   ```

### When in Doubt, Ask!

If avoiding N+1 queries seems very complicated:
1. **Ask first**: "I'm implementing X and might need N queries. Is there a batch approach?"
2. Explain the use case
3. Discuss trade-offs (complexity vs performance)

**Remember**: It's better to ask and find a simple solution than to implement a complex workaround or ship slow code.

### Detection Checklist

Before committing code, check:
- [ ] Are there any loops that call async database functions?
- [ ] Can multiple queries be combined into a single batch query?
- [ ] Is there a more efficient query pattern (IN clause, batch fetch)?
- [ ] Have I tested with realistic data volumes (100+ items)?

## Command Verification

**Before suggesting commands to the user, always verify them yourself first!**

- Run the command to ensure it works
- Check for typos, correct paths, and valid syntax
- If a command fails, fix it before presenting to the user
- Don't give untested commands - execute them directly when possible

## Best Practices

1. **Write tests first**: No production code without a failing test
2. **One test, one behavior**: Keep tests focused on a single concern
3. **Merge tests with identical inputs**: When multiple assertions test the same request/input without variations, combine them into one test
4. **Descriptive names**: Tests should document behavior
5. **Fast feedback**: Run tests frequently during development
6. **Refactor with confidence**: Tests protect against regressions
7. **Commit working code**: All tests pass before committing
8. **Document assumptions**: Use comments for non-obvious decisions
9. **Always finish with commit**: Run pre-commit.sh and commit before moving to next feature

## Avoid Hardcoded Values in Comments

**CRITICAL**: Never put specific values in comments that are configured elsewhere.

Comments with hardcoded values become stale when configuration changes, leading to confusion.

**BAD** - Hardcoded values in comments:
```sql
-- Session expires in 1 year
expires_at timestamp,

-- Magic link valid for 15 minutes
expires_at timestamp,
```

**GOOD** - Reference configuration source:
```sql
-- Expiry configured via SESSION_MAX_AGE_DAYS env var
expires_at timestamp,

-- Expiry configured via MAGIC_LINK_EXPIRY_MINUTES env var
expires_at timestamp,
```

**Why this matters**:
- Comments become incorrect when config values change
- Developers may trust stale comments over actual configuration
- Single source of truth (config) is more reliable than scattered comments

## Integration Tests

### Reuse Existing Code

**CRITICAL**: Integration tests must reuse existing production code, not reimplement it.

**BAD** - Reimplementing logic in tests:
```rust
// Don't rebuild workflows step-by-step in tests
#[tokio::test]
async fn test_crawl_workflow() {
    // Manually creating queue entries
    let entry = CrawlQueueEntry { ... };
    db_client.insert_queue_entry(&entry).await?;

    // Manually simulating crawl
    let content = fetch_url(&url).await?;

    // Manually uploading to storage
    let storage_id = storage_client.upload_content(&content, &url).await?;

    // Manually creating crawled page
    let page = CrawledPage { storage_id: Some(storage_id), ... };
    db_client.upsert_crawled_page(&page).await?;
}
```

**GOOD** - Using existing high-level services:
```rust
// Use production code to test production behavior
#[tokio::test]
async fn test_crawl_workflow() {
    let processor = QueueProcessor::with_storage(db_client, storage_client, ...);

    // Use the actual production workflow
    db_client.insert_queue_entry(&entry).await?;
    processor.process_next_entry().await?;

    // Verify results
    let page = db_client.get_crawled_page(&domain, &path).await?;
    assert!(page.is_some());
}
```

**Why this matters**:
- Tests verify actual production behavior, not a parallel implementation
- Bugs in production code get caught, not masked by test-specific implementations
- Less code to maintain (single implementation, not two)
- Tests stay in sync with production automatically

### Proper Environment Setup

**CRITICAL**: Every test must set up its own prerequisites. "May be skipped" or "might not work" is NOT acceptable.

**BAD** - Tests with uncertain prerequisites:
```rust
/// Prerequisites:
/// - allowed_domains table should have the test domain (or this test may be skipped)
#[tokio::test]
async fn test_queue_workflow() {
    // Retrieve entry (note: may get a different entry if queue is not empty)
    let retrieved = db_client.get_next_queue_entry().await?;
}
```

**GOOD** - Tests that set up their own environment:
```rust
#[tokio::test]
async fn test_queue_workflow() {
    // Setup: Ensure test domain is allowed
    db_client.insert_allowed_domain("test.example.com").await
        .expect("Failed to set up test domain");

    // Now run the actual test with known state
    let entry = CrawlQueueEntry { domain: "test.example.com".into(), ... };
    db_client.insert_queue_entry(&entry).await?;

    let retrieved = db_client.get_next_queue_entry().await?;
    assert_eq!(retrieved.unwrap().domain, "test.example.com");

    // Cleanup
    db_client.delete_allowed_domain("test.example.com").await?;
}
```

**Principles**:
1. **Tests own their environment**: Set up all prerequisites in the test itself
2. **No external dependencies**: Don't rely on pre-existing data or manual setup
3. **Deterministic results**: Tests must produce the same result every run
4. **Clean state**: Either clean up after tests, or use unique identifiers to avoid collisions
5. **Fail explicitly**: If setup fails, the test should fail with a clear error, not skip silently

## Type Safety: Avoid Magic Strings

**IMPORTANT**: Never use raw string comparisons for configuration values or enum-like states.

**BAD** - Magic strings:
```rust
let agent_mode = env::var("AGENT_MODE").unwrap_or_else(|_| "all".to_string());
if agent_mode == "worker" || agent_mode == "all" {
    // start worker
}
```

**GOOD** - Use enums:
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
enum AgentMode {
    Worker,
    Manager,
    All,
}

impl AgentMode {
    fn from_env() -> Self {
        match env::var("AGENT_MODE").as_deref().unwrap_or("all") {
            "worker" => AgentMode::Worker,
            "manager" => AgentMode::Manager,
            _ => AgentMode::All,
        }
    }

    fn should_process_queue(&self) -> bool {
        matches!(self, AgentMode::Worker | AgentMode::All)
    }
}
```

**Benefits**:
- Compiler enforces exhaustiveness checking
- No runtime typos or invalid values
- Self-documenting code
- Easy refactoring (compiler catches all usages)

## Early Returns: Flat Code Structure

**IMPORTANT**: Use early returns to keep code flat. Avoid nested `match` or `if` ladders.

The pattern: Handle errors and edge cases first with early returns, then write the success path at the same indentation level as the function start.

**BAD** - Nested match ladder:
```rust
async fn enqueue_link(&self, link: &str) {
    match url::Url::parse(link) {
        Ok(parsed) => {
            let domain = parsed.host_str().unwrap_or("").to_string();
            if domain.is_empty() {
                return;
            }

            match self.db_client.is_domain_allowed(&domain).await {
                Ok(is_allowed) => {
                    if !is_allowed {
                        return;
                    }

                    match self.db_client.crawled_page_exists(&domain, &path).await {
                        Ok(exists) => {
                            if exists {
                                return;
                            }
                            // Finally do the work here, deeply nested
                            self.db_client.insert_queue_entry(&entry).await;
                        }
                        Err(e) => eprintln!("Error: {}", e),
                    }
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Err(e) => eprintln!("Failed to parse: {}", e),
    }
}
```

**GOOD** - Early returns, flat structure:
```rust
async fn enqueue_link(&self, link: &str) {
    // Parse URL - early return on error
    let parsed = match url::Url::parse(link) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to parse: {}", e);
            return;
        }
    };

    // Validate domain - early return if empty
    let domain = parsed.host_str().unwrap_or("").to_string();
    if domain.is_empty() {
        return;
    }

    // Check allowlist - early return on error or not allowed
    let is_allowed = match self.db_client.is_domain_allowed(&domain).await {
        Ok(allowed) => allowed,
        Err(e) => {
            eprintln!("Error checking allowlist: {}", e);
            return;
        }
    };
    if !is_allowed {
        return;
    }

    // Check if already crawled - early return on error or exists
    let exists = match self.db_client.crawled_page_exists(&domain, &path).await {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error checking existence: {}", e);
            return;
        }
    };
    if exists {
        return;
    }

    // Success path - at same indentation level as function start
    if let Err(e) = self.db_client.insert_queue_entry(&entry).await {
        eprintln!("Failed to insert: {}", e);
    }
}
```

**Principles**:
1. **Handle errors first**: Check for error conditions and return early
2. **Flat success path**: Main logic stays at the top indentation level
3. **One concern per block**: Each early-return block handles one validation
4. **Readable flow**: Code reads top-to-bottom without mental stack management

## Environment Variables Management

**CRITICAL**: Never use default values directly in code. Always use environment variables for configuration.

### Standard Approach

1. **Create `.env` file** (gitignored) for local development:
   ```bash
   cp .env.example .env
   ```

2. **Maintain `.env.example`** with all required variables and documentation

3. **Docker Compose integration** using `env_file`:
   ```yaml
   services:
     lala-agent:
       env_file:
         - .env
   ```

**BAD** - Hardcoded defaults:
```rust
let db_url = env::var("DB_URL").unwrap_or("postgres://localhost:5432".to_string());
```

**GOOD** - Environment variables only:
```rust
let db_url = env::var("DATABASE_URL")
    .expect("DATABASE_URL environment variable must be set");
```

### File Structure
```
.env.example       # Committed - Template with all variables documented
.env               # Gitignored - Your local configuration
.gitignore         # Must include .env
docker-compose.yml # Uses env_file: .env
```

## Docker Compose Usage

**CRITICAL**: Always use `docker compose` (two words) instead of the legacy `docker-compose` (hyphenated).

**GOOD** - Modern Docker Compose V2:
```bash
docker compose up -d
docker compose down
docker compose logs -f
```

**BAD** - Legacy Docker Compose V1:
```bash
docker-compose up -d      # Don't use hyphenated version
```

### Always Use --build Flag

**CRITICAL**: Always use `--build` flag when starting services to ensure fresh builds with latest code and environment variables.

**GOOD** - Always rebuild:
```bash
docker compose up -d --build
docker compose -f docker-compose.yml -f docker-compose.test.yml up -d --build lala-agent
```

**BAD** - Using cached builds:
```bash
docker compose up -d                    # May use stale cached build
docker compose restart lala-agent       # Doesn't rebuild, just restarts
```

**Why this matters**:
- Ensures environment variables are properly read from docker-compose.yml overrides
- Picks up latest code changes without manual rebuild steps
- Prevents subtle bugs from cached builds with old configuration
- Standard practice for CI/CD pipelines

## Database Migrations

**CRITICAL**: Never drop tables to add new columns. Use ALTER TABLE migrations instead.

### Migration Strategy

PostgreSQL schema is defined in `docker/postgres/schema.sql`. This file contains the full current schema and is applied automatically on first database start via Docker's `/docker-entrypoint-initdb.d/` mechanism.

For existing deployments, use standard PostgreSQL migrations:

### Creating a Migration

When adding/modifying schema:

1. Update `docker/postgres/schema.sql` with the new column/table
2. Write the corresponding `ALTER TABLE` SQL for existing deployments
3. Apply to running database via `psql`

**Example migration:**
```sql
-- Add storage_compression column to crawled_pages
ALTER TABLE crawled_pages ADD COLUMN IF NOT EXISTS storage_compression SMALLINT DEFAULT 0;
```

### Running Migrations

**For existing deployments:**
```bash
# Connect to PostgreSQL and run migration
docker exec -it lalasearch-postgres psql -U lalasearch -d lalasearch -c "
ALTER TABLE crawled_pages ADD COLUMN IF NOT EXISTS storage_compression SMALLINT DEFAULT 0;
"
```

**For fresh deployments:**
```bash
# Schema is applied automatically on first start
docker compose up -d postgres
```

### Migration Principles

1. **Additive only**: Only add columns/tables, never remove or rename in production
2. **Safe to re-run**: Use `IF NOT EXISTS` / `IF EXISTS` for idempotency
3. **No data loss**: Never drop tables or columns with production data
4. **Document changes**: Comment each migration with date and purpose
5. **Update schema.sql**: Always keep `docker/postgres/schema.sql` in sync

### Handling New Columns in Rust Code

**CRITICAL**: When adding nullable columns to existing tables, use `Option<T>` in Rust structs.

**BAD** - Non-nullable type for new column:
```rust
// This will fail for existing rows where storage_compression is NULL
pub storage_compression: i16,  // i16 can't deserialize NULL
```

**GOOD** - Use Option for nullable columns:
```rust
// Option<i16> handles NULL values from existing rows
pub storage_compression: Option<i16>,
```

**Checklist when adding a column to existing table:**
1. Update `docker/postgres/schema.sql` with new column
2. Write `ALTER TABLE ADD COLUMN IF NOT EXISTS` for existing deployments
3. Use `Option<T>` in Rust struct if column can be NULL
4. Handle `None` case with sensible default
5. Add test for NULL/None handling

### Verification

```bash
# List all tables
docker exec lalasearch-postgres psql -U lalasearch -d lalasearch -c "\dt"

# Describe a specific table
docker exec lalasearch-postgres psql -U lalasearch -d lalasearch -c "\d crawled_pages"

# Check RLS policies
docker exec lalasearch-postgres psql -U lalasearch -d lalasearch -c "SELECT * FROM pg_policies;"
```

## Cross-Platform Compatibility

**CRITICAL**: Code must work across all major platforms and architectures.

**BAD** - Local-only fixes:
```rust
#[cfg(target_os = "windows")]
let config_path = "C:\\Users\\...";

let db_url = "postgres://localhost:5432/lalasearch";  // Fails in Docker
```

**GOOD** - Cross-platform solutions:
```rust
let config_path = env::var("CONFIG_PATH")
    .unwrap_or_else(|_| "config.toml".to_string());

let db_url = env::var("DATABASE_URL")
    .expect("DATABASE_URL must be set");
```

**Target Platforms**:
- Linux (x86_64, ARM64)
- macOS (Intel, Apple Silicon)
- Windows (x86_64, ARM64)
- Docker containers (Alpine, Debian base images)

## Troubleshooting Build Issues

### Windows PDB Linker Error (LNK1318)

**CRITICAL**: Never use `git commit --no-verify` as a first resort. Always troubleshoot properly first.

**Symptom**:
```
LINK : fatal error LNK1318: Unexpected PDB error; LIMIT (12)
error: could not compile `lala-agent` (bin "lala-agent") due to 1 previous error
```

**Root Cause**: Windows linker cannot write PDB (Program Database) files. Common causes:
- Antivirus locking files
- Multiple build processes running
- Corrupted build cache
- Disk space or permissions issues

**Resolution Steps** (try in order):

1. **Clean build cache and retry**:
   ```bash
   cargo clean
   cargo build
   # Or for tests:
   cargo clean
   cargo test
   ```

2. **If step 1 fails, close all IDEs and processes**:
   - Close VS Code, Visual Studio, or any IDE
   - Kill any running `rust-analyzer` processes
   - Retry: `cargo clean && cargo build`

3. **If step 2 fails, check antivirus**:
   - Temporarily disable antivirus
   - Add `target/` directory to antivirus exclusions
   - Retry: `cargo clean && cargo build`

4. **If step 3 fails, verify disk space and permissions**:
   - Ensure sufficient disk space (>5GB free)
   - Run terminal as Administrator
   - Retry: `cargo clean && cargo build`

5. **Last resort - verify code only**:
   ```bash
   cargo check  # Verifies code without linking
   ```

**When to use `--no-verify`**:

Only use `git commit --no-verify` if ALL of these are true:
- ✅ `cargo fmt --check` passes
- ✅ `cargo clippy -- -D warnings` passes
- ✅ `cargo check` passes (code compiles)
- ✅ You've tried ALL troubleshooting steps above
- ✅ The error is confirmed to be a Windows environment issue, not a code issue

**Document when skipping hooks**:
```bash
# Document WHY you're skipping in the commit message
git commit --no-verify -m "feat: add feature

Note: Used --no-verify due to Windows PDB linker error (LNK1318).
Code passes fmt, clippy, and check. Tests will run in CI.
"
```

**Never skip for**:
- Code that doesn't compile (`cargo check` fails)
- Code with clippy warnings
- Code with formatting issues
- Untested changes (unless it's a Windows-only environment issue)

## Commands Reference

```bash
# Tier 1: Unit tests (fast, no dependencies)
cargo test --lib

# Tier 1: Storage-dependent tests (requires Docker services)
cargo test --lib -- --ignored

# Tier 2: Integration tests (slower, CI-focused)
cargo test --test '*'

# Run specific test
cargo test test_name

# Format code
cargo fmt

# Lint code
cargo clippy -- -D warnings

# Build project
cargo build

# Run project
cargo run

# Pre-commit checks (runs Tier 1 tests, starts Docker automatically)
./scripts/pre-commit.sh
```

## Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Axum Documentation](https://docs.rs/axum/)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Test-Driven Development](https://en.wikipedia.org/wiki/Test-driven_development)
