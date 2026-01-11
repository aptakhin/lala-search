# Claude Development Guidelines for LalaSearch

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

Before every commit, the following checks run automatically:

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
2. **One test, one behavior**: Keep tests focused
3. **Descriptive names**: Tests should document behavior
4. **Fast feedback**: Run tests frequently during development
5. **Refactor with confidence**: Tests protect against regressions
6. **Commit working code**: All tests pass before committing
7. **Document assumptions**: Use comments for non-obvious decisions
8. **Always finish with commit**: Run pre-commit.sh and commit before moving to next feature

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
