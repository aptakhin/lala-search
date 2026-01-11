# Claude Development Guidelines for LalaSearch

## Critical: Ask Questions First

**IMPORTANT**: Don't blindly execute requests. Always think critically about the best solution.

### When to Ask Questions

Ask clarifying questions when:
1. **Multiple approaches exist**: The request could be solved in different ways
2. **Potential conflicts**: The request might conflict with existing patterns or architecture
3. **Sub-optimal solution**: You see a better approach than what was requested
4. **Missing context**: You need more information to provide the best solution
5. **Trade-offs involved**: The solution has important implications to discuss
6. **Unclear requirements**: The request is ambiguous or could be interpreted differently

### Examples

**Good** - Asking when there's a better approach:
- User: "Add a new config file for the version"
- Assistant: "I notice the version is currently hardcoded. Would you prefer to extract it from Cargo.toml using a build script instead? That would keep it in sync automatically."

**Good** - Asking about conflicts:
- User: "Let's use JSON for configuration"
- Assistant: "I see we're using TOML for Cargo.toml. Should we use TOML for consistency, or is there a specific reason to prefer JSON?"

**Bad** - Executing without thinking:
- User: "Create a new file for every route handler"
- Assistant: *Creates files without asking if organizing by feature or by type would be better*

### Principle

**Your expertise matters.** You're not just a code executor - you're a development partner. Use your knowledge to:
- Suggest better alternatives
- Point out potential issues
- Ask about architectural decisions
- Clarify ambiguous requirements

**Balance**: Ask questions when they add value, but don't over-question trivial decisions.

## Technology Choices: Open Source Solutions Only

**CRITICAL**: LalaSearch is a fully open source project. All dependencies and technical solutions must be open source.

### Why Open Source

1. **Transparency**: Anyone can audit the code and security
2. **Community support**: Active communities provide better long-term support
3. **No vendor lock-in**: Freedom to modify, fork, or migrate
4. **Cost**: No licensing fees or proprietary restrictions
5. **Licensing compliance**: Ensures the project remains distributable under its open source license

### Making Technology Decisions

When choosing a solution (database, library, service, etc.), **always prioritize open source alternatives**:

#### ✅ GOOD - Open Source Solutions
- **PostgreSQL** - Open source relational database
- **Redis** - Open source in-memory data store
- **Elasticsearch** - Open source search and analytics engine
- **Apache Kafka** - Open source event streaming platform
- **Meilisearch** - Open source search engine
- **MinIO** - Open source object storage (S3-compatible)
- **Docker** - Open source containerization platform
- **Prometheus** - Open source monitoring and metrics

#### ❌ AVOID - Proprietary/Closed Source Solutions
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

### Example

**Bad decision**:
```rust
// ❌ Using proprietary tools without justification
// No discussion, breaks open source principle
use proprietary_service::Client;
```

**Good decision**:
```rust
// ✅ Using open source Apache Cassandra for distributed NoSQL
use scylla::Session;  // scylla driver works with both Cassandra and ScyllaDB

// ✅ Using open source PostgreSQL for relational data
use sqlx::postgres::PgPool;

// If no open source option exists, we'd discuss first:
// > We considered all open source alternatives (X, Y, Z), but
// > [specific technical requirement] cannot be met. We propose
// > using [proprietary tool] with [migration path to OSS].
```

**Key principle**: Open source decisions ensure LalaSearch remains free, transparent, and maintainable by the community long-term.

## Test-Driven Development (TDD) Workflow

All features in LalaSearch follow a strict TDD cycle to ensure code quality, maintainability, and correctness.

### TDD Cycle

```
1. Analyze → 2. Red → 3. Green → 4. Refactor → (repeat)
```

#### 1. **Analyze Phase**
Before writing any code, identify and document:
- **Corner cases**: Edge conditions that might break the feature
- **Error scenarios**: Invalid inputs, network failures, resource exhaustion
- **Happy path**: Expected normal operation
- **Boundary conditions**: Limits, empty states, maximum values
- **Concurrency issues**: Race conditions, deadlocks (for async code)

#### 2. **Red Phase** (Failing Test)
- Write a test that captures ONE specific behavior or corner case
- The test MUST fail initially (compile errors count as failures)
- Keep tests focused and isolated
- Use descriptive test names: `test_<scenario>_<expected_outcome>`

#### 3. **Green Phase** (Minimal Implementation)
- Write the MINIMUM code to make the test pass
- Don't worry about perfection or optimization
- Avoid implementing features not covered by current tests
- Run tests frequently: `cargo test`

#### 4. **Refactor Phase** (Optional)
- Improve code quality without changing behavior
- Extract duplicated code
- Improve naming and structure
- Tests must still pass after refactoring
- Skip if code is already clean

### Code Quality Standards

#### Formatting
- **Tool**: `rustfmt`
- **Config**: `.rustfmt.toml` at repository root
- **Command**: `cargo fmt`
- **Enforcement**: Runs automatically in pre-commit hook

#### Linting
- **Tool**: `clippy`
- **Command**: `cargo clippy -- -D warnings`
- **Enforcement**: Runs automatically in pre-commit hook
- **Standard**: All warnings treated as errors

#### Testing
- **Unit tests**: Co-located with code in same file
- **Integration tests**: In `tests/` directory
- **Command**: `cargo test`
- **Coverage**: Aim for high coverage, especially for critical paths

### Pre-Commit Workflow

A git pre-commit hook runs automatically before every commit to ensure code quality.

**First-Time Setup** (after cloning):
```bash
cp scripts/pre-commit.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

The hook runs these checks automatically:
```bash
1. cargo fmt --check      # Verify formatting
2. cargo clippy -- -D warnings  # Lint for issues
3. cargo test             # Run all tests
```

If any check fails, the commit is blocked.

### Project Structure

```
lalasearch/
├── docs/
│   ├── overview.md           # Project vision and architecture
│   └── claude-guidelines.md  # This file
├── lala-agent/              # Core agent implementation
│   ├── src/
│   │   ├── main.rs          # Entry point
│   │   ├── lib.rs           # Library root
│   │   ├── routes/          # HTTP route handlers
│   │   └── models/          # Data models
│   ├── tests/               # Integration tests
│   ├── Cargo.toml           # Dependencies
│   └── .rustfmt.toml        # Formatting config
└── .git/
    └── hooks/
        └── pre-commit       # Git pre-commit hook
```

### Example TDD Workflow: Version Endpoint

#### Analyze
**Feature**: HTTP GET endpoint returning agent version

**Corner Cases to Consider**:
1. Valid GET request to `/version` → returns 200 with JSON
2. Version format is semantic versioning (MAJOR.MINOR.PATCH)
3. Response includes version string and agent name
4. Invalid HTTP methods (POST, PUT, DELETE) → 405 Method Not Allowed
5. Concurrent requests → all succeed with same version
6. Server startup → version is available immediately

#### Red Phase
```rust
#[tokio::test]
async fn test_version_endpoint_returns_200() {
    // This test will fail initially
    let response = get_version().await;
    assert_eq!(response.status(), 200);
}
```

#### Green Phase
```rust
async fn get_version() -> Response {
    // Minimal implementation
    Response::builder()
        .status(200)
        .body(Body::empty())
        .unwrap()
}
```

#### Refactor Phase
```rust
// After all tests pass, improve structure
#[derive(Serialize)]
struct VersionResponse {
    agent: String,
    version: String,
}
```

### Completing Features

**IMPORTANT**: Every feature MUST be completed with these steps:

1. **Run pre-commit checks**:
   ```bash
   ./scripts/pre-commit.sh
   ```

2. **Commit the feature** (only if all checks pass):
   ```bash
   git add .
   git commit -m "feat: descriptive commit message"
   ```

**Never consider a feature complete until it is committed!**

### Best Practices

1. **Write tests first**: No production code without a failing test
2. **One test, one behavior**: Keep tests focused on a single concern
3. **Merge tests with identical inputs**: When multiple assertions test the same request/input without variations, combine them into one test to reduce redundancy
   - ✅ Good: One test that checks status code, content-type, and structure of the same response
   - ❌ Avoid: Three separate tests making identical requests just to check different response aspects
4. **Descriptive names**: Tests should document behavior
5. **Fast feedback**: Run tests frequently during development
6. **Refactor with confidence**: Tests protect against regressions
7. **Commit working code**: All tests pass before committing
8. **Document assumptions**: Use comments for non-obvious decisions
9. **Always finish with commit**: Run pre-commit.sh and commit before moving to next feature

### Type Safety: Avoid Magic Strings

**IMPORTANT**: Never use raw string comparisons for configuration values or enum-like states.

Instead of:
```rust
// ❌ BAD: Magic strings, error-prone, no compile-time checking
let agent_mode = env::var("AGENT_MODE").unwrap_or_else(|_| "all".to_string());
if agent_mode == "worker" || agent_mode == "all" {
    // start worker
}
```

Use enums:
```rust
// ✅ GOOD: Type-safe, compiler enforces valid values, exhaustive matching
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

// Usage: Type-safe, self-documenting
let mode = AgentMode::from_env();
if mode.should_process_queue() {
    // start worker
}
```

**Benefits**:
- Compiler enforces exhaustiveness checking
- No runtime typos or invalid values
- Self-documenting code
- Easy refactoring (compiler catches all usages)
- Enables custom methods with domain logic

### Environment Variables Management

**CRITICAL**: Never use default values directly in code. Always use environment variables for configuration.

#### Standard Approach

1. **Create `.env` file** (gitignored) for local development:
   ```bash
   cp .env.example .env
   # Edit .env with your local configuration
   ```

2. **Maintain `.env.example`** with all required variables and documentation:
   ```bash
   # Example structure
   AGENT_MODE=all
   CASSANDRA_HOSTS=127.0.0.1:9042
   CASSANDRA_KEYSPACE=lalasearch
   ```

3. **Docker Compose integration** using `env_file`:
   ```yaml
   services:
     lala-agent:
       env_file:
         - .env
       environment:
         # Override Docker-specific values
         - CASSANDRA_HOSTS=cassandra:9042
   ```

#### ❌ BAD - Hardcoded defaults:
```rust
// Never hardcode defaults in application code
let db_host = env::var("DB_HOST").unwrap_or("127.0.0.1:9042".to_string());
let keyspace = env::var("KEYSPACE").unwrap_or("lalasearch".to_string());
```

#### ✅ GOOD - Environment variables only:
```rust
// Fail early if required variables are missing
let db_host = env::var("CASSANDRA_HOSTS")
    .expect("CASSANDRA_HOSTS environment variable must be set");
let keyspace = env::var("CASSANDRA_KEYSPACE")
    .expect("CASSANDRA_KEYSPACE environment variable must be set");
```

Or with defaults from `.env` file:
```rust
// Load from .env file (using dotenvy crate)
dotenvy::dotenv().ok();

let db_host = env::var("CASSANDRA_HOSTS")
    .expect("CASSANDRA_HOSTS must be set in .env file");
```

#### Benefits:
- **Explicit configuration**: Forces developers to think about configuration
- **No surprises**: Clear what values are being used
- **Environment parity**: Development, staging, and production use same pattern
- **Security**: Sensitive values never committed to git
- **Documentation**: `.env.example` serves as configuration reference

#### File Structure:
```
.env.example       # Committed - Template with all variables documented
.env               # Gitignored - Your local configuration
.gitignore         # Must include .env
docker-compose.yml # Uses env_file: .env
```

### Docker Compose Usage

**CRITICAL**: Always use `docker compose` (two words) instead of the legacy `docker-compose` (hyphenated).

#### ✅ GOOD - Modern Docker Compose V2:
```bash
docker compose up -d
docker compose down
docker compose logs -f
docker compose ps
```

#### ❌ BAD - Legacy Docker Compose V1:
```bash
docker-compose up -d      # Don't use hyphenated version
docker-compose down       # Don't use hyphenated version
```

**Why:**
- Docker Compose V2 (`docker compose`) is integrated into Docker CLI since 2020
- V1 (`docker-compose`) is deprecated and requires separate installation
- V2 is faster, better integrated, and actively maintained
- V2 is the default in all modern Docker installations

**Apply everywhere:**
- Documentation (README.md, docs/*.md)
- Scripts and automation
- CI/CD pipelines
- Comments and examples

### Cross-Platform Compatibility

**CRITICAL**: Code must work across all major platforms and architectures.

When debugging issues, **never** apply platform-specific or machine-specific workarounds:

❌ **BAD** - Local-only fixes:
```rust
// Windows-only path hack
#[cfg(target_os = "windows")]
let config_path = "C:\\Users\\aptak\\...";

// Hard-coded absolute paths
let db_host = "127.0.0.1:9042";  // Fails in Docker

// Binary-specific checks
if std::env::consts::ARCH == "x86_64" {
    // Different behavior for different architectures
}
```

✅ **GOOD** - Cross-platform solutions:
```rust
// Use environment variables (works everywhere)
let config_path = env::var("CONFIG_PATH")
    .unwrap_or_else(|_| "config.toml".to_string());

// Use environment variables for network config
let db_host = env::var("SCYLLA_HOSTS")
    .unwrap_or_else(|_| "127.0.0.1:9042".to_string());

// Use conditional compilation for abstractions, not behaviors
#[cfg(unix)]
fn set_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path) -> Result<()> {
    Ok(()) // No-op on non-Unix
}
```

**Target Platforms**:
- ✅ Linux (x86_64, ARM64)
- ✅ macOS (Intel, Apple Silicon)
- ✅ Windows (x86_64, ARM64)
- ✅ Docker containers (Alpine, Debian base images)

**When debugging**:
1. If it fails locally, don't hardcode a fix for your machine
2. Think about why it fails in Docker or on different OS/arch
3. Use environment variables for configuration
4. Use standard library functions that are cross-platform
5. Test the fix works in Docker before considering it done

**Example**: The Meilisearch health check failed due to SDK JSON parsing, not platform differences. The fix (removing the health check) works on all platforms, not just Windows.

### Commands Reference

```bash
# Create new Rust project
cargo new lala-agent

# Add dependencies
cargo add axum tokio serde

# Run tests
cargo test

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

# Watch mode (requires cargo-watch)
cargo watch -x test
```

### Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Axum Documentation](https://docs.rs/axum/)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Test-Driven Development](https://en.wikipedia.org/wiki/Test-driven_development)
