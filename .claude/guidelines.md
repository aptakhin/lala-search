# Claude Code Guidelines for LalaSearch

**IMPORTANT**: This project follows strict Test-Driven Development (TDD) practices.

All development work MUST follow the comprehensive guidelines documented in:

**[docs/claude-guidelines.md](../docs/claude-guidelines.md)**

## Quick Reference

### TDD Cycle
1. **Analyze**: Identify corner cases and requirements
2. **Red**: Write failing tests
3. **Green**: Minimal implementation to pass tests
4. **Refactor**: Improve code quality (optional)

### Before Every Commit
```bash
./scripts/pre-commit.sh
```

This runs:
- `cargo fmt --check` (formatting)
- `cargo clippy -- -D warnings` (linting)
- `cargo test` (all tests)

### Completing Features

**Every feature MUST be completed with**:
1. Run `./scripts/pre-commit.sh`
2. Commit: `git add . && git commit -m "feat: description"`

**Never consider a feature complete until it is committed!**

## Project Structure

- `lala-agent/` - Core Rust agent (axum + tokio)
- `docs/` - All documentation
- `scripts/` - Development tools

## Key Principles

1. Tests before code
2. High code quality (zero clippy warnings)
3. Proper formatting (rustfmt)
4. Complete features with commits
5. Document architectural decisions

See [docs/claude-guidelines.md](../docs/claude-guidelines.md) for full details.
