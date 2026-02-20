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
- **MinIO** - Open source object storage (S3-compatible)
- **Docker** - Open source containerization platform
- **Prometheus** - Open source monitoring and metrics

### AVOID - Proprietary/Closed Source Solutions
- **ScyllaDB** - Changed from AGPL to proprietary "source-available" license in Dec 2024 (use Apache Cassandra instead)
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
| Storage-dependent | `src/**/*.rs` | `#[ignore]` | Require Cassandra/MinIO/Meilisearch |

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

The script automatically starts Docker services (Cassandra, MinIO, Meilisearch) for storage-dependent tests.

If any check fails, fix the issues before committing.

## Completing Features

**Every feature MUST be completed with**:
1. Update `README.md` if project structure changed (new files, directories, tests, status)
2. Update any relevant files in `docs/` — keep architecture docs, keyspace names, env vars, and feature descriptions in sync with the code
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
│   └── cassandra/
│       ├── schema.cql            # Apache Cassandra database schema
│       └── migrations/           # Database migration files
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
let db_host = env::var("DB_HOST").unwrap_or("127.0.0.1:9042".to_string());
```

**GOOD** - Environment variables only:
```rust
let db_host = env::var("CASSANDRA_HOSTS")
    .expect("CASSANDRA_HOSTS environment variable must be set");
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

Cassandra doesn't have built-in migration tools, so we use a simple file-based approach:

1. **Schema file** (`docker/cassandra/schema.cql`): Contains the full current schema for fresh deployments
2. **Migrations directory** (`docker/cassandra/migrations/`): Contains numbered migration files for existing deployments

### Creating a Migration

When adding/modifying schema:

1. Update `schema.cql` with the new column/table
2. Create a migration file in `migrations/` with format: `NNN_description.cql`
3. Document the migration with date and purpose

**Example migration file** (`migrations/001_add_storage_compression.cql`):
```sql
-- Migration 001: Add storage_compression column to crawled_pages
-- Date: 2026-01-18
-- Description: Track compression type for stored content (0=none, 1=gzip)

USE ${KEYSPACE_NAME};

ALTER TABLE crawled_pages ADD storage_compression tinyint;
```

### Running Migrations

**For existing deployments:**
```bash
# Run specific migration
docker exec lalasearch-cassandra cqlsh -e "USE lalasearch; ALTER TABLE crawled_pages ADD storage_compression tinyint;"

# Or run migration file (after copying to container)
docker exec lalasearch-cassandra cqlsh -f /path/to/migration.cql
```

**For fresh deployments:**
```bash
# Schema already contains all columns - no migration needed
docker compose up -d cassandra cassandra-init
```

### Migration Principles

1. **Additive only**: Only add columns/tables, never remove or rename
2. **Safe to re-run**: Migrations should be idempotent (use IF NOT EXISTS, ADD ignores existing)
3. **No data loss**: Never drop tables or columns with production data
4. **Document changes**: Each migration file explains what and why
5. **Update schema.cql**: Always keep schema.cql in sync with migrations

### Handling New Columns in Rust Code

**CRITICAL**: When adding columns to existing tables, existing rows will have NULL values for the new column. Always use `Option<T>` in Rust deserialization.

**BAD** - Non-nullable type for new column:
```rust
// This will fail for existing rows where storage_compression is NULL
rows::<(String, String, i8)>()  // i8 can't deserialize NULL

pub fn from_db_value(value: i8) -> Self {
    match value {
        1 => CompressionType::Gzip,
        _ => CompressionType::None,
    }
}
```

**GOOD** - Use Option for new columns:
```rust
// Option<i8> handles NULL values from existing rows
rows::<(String, String, Option<i8>)>()

pub fn from_db_value(value: Option<i8>) -> Self {
    match value {
        Some(1) => CompressionType::Gzip,
        _ => CompressionType::None,  // NULL defaults to None
    }
}
```

**Checklist when adding a column to existing table:**
1. ✅ Create migration file with ALTER TABLE ADD
2. ✅ Update schema.cql with new column
3. ✅ Use `Option<T>` in Rust deserialization tuple
4. ✅ Handle `None` case with sensible default
5. ✅ Add test for NULL/None handling

### Applying Schema Changes

**How to apply schema changes for both single-tenant and multi-tenant:**

#### Method 1: Re-run cassandra-init (Recommended for Development)

The cassandra-init service is idempotent and can be run multiple times safely:

```bash
docker compose up cassandra-init
```

This will:
- Apply all schema changes from `schema_system.cql` (system keyspace)
- Apply all schema changes from `schema.cql` (tenant keyspace)
- Use `CREATE TABLE IF NOT EXISTS` to safely skip existing tables
- Substitute environment variables for keyspace names

#### Method 2: Manual Migration (For Production)

For production systems with existing data:

```bash
# Apply to system keyspace
docker exec lalasearch-cassandra cqlsh -e "
USE lalasearch_system;
ALTER TABLE existing_table ADD new_column text;
"

# Apply to tenant keyspace(s)
docker exec lalasearch-cassandra cqlsh -e "
USE lalasearch_default;
ALTER TABLE existing_table ADD new_column text;
"
```

#### Important Notes

1. **Reserved Keywords**: Avoid CQL reserved keywords (`token`, `user`, `key`, etc.)
   - Use descriptive names: `token_hash` instead of `token`
   - If unavoidable, escape with double quotes: `"token"` (but this makes it case-sensitive)

2. **Environment Variables**: Schema files use placeholders like `${SYSTEM_KEYSPACE_NAME}` and `${KEYSPACE_NAME}`
   - cassandra-init substitutes these automatically via sed
   - Manual execution requires replacing these first

3. **Single vs Multi-Tenant**:
   - **System keyspace** (`lalasearch_system`): Always one instance, shared across all tenants
   - **Tenant keyspaces** (`lalasearch_*`): One per tenant in multi-tenant mode
   - Both are created by cassandra-init based on env vars

4. **Verification**:
   ```bash
   # Check system keyspace tables
   docker exec lalasearch-cassandra cqlsh -e "DESCRIBE KEYSPACE lalasearch_system;"

   # Check tenant keyspace tables
   docker exec lalasearch-cassandra cqlsh -e "DESCRIBE KEYSPACE lalasearch_default;"
   ```

## Cross-Platform Compatibility

**CRITICAL**: Code must work across all major platforms and architectures.

**BAD** - Local-only fixes:
```rust
#[cfg(target_os = "windows")]
let config_path = "C:\\Users\\...";

let db_host = "127.0.0.1:9042";  // Fails in Docker
```

**GOOD** - Cross-platform solutions:
```rust
let config_path = env::var("CONFIG_PATH")
    .unwrap_or_else(|_| "config.toml".to_string());

let db_host = env::var("CASSANDRA_HOSTS")
    .expect("CASSANDRA_HOSTS must be set");
```

**Target Platforms**:
- Linux (x86_64, ARM64)
- macOS (Intel, Apple Silicon)
- Windows (x86_64, ARM64)
- Docker containers (Alpine, Debian base images)

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
