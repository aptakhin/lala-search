# Claude Code Guidelines for LalaSearch

**CRITICAL**: Read the full guidelines at **[docs/claude-guidelines.md](../docs/claude-guidelines.md)**

## Most Important Rule

**ASK QUESTIONS FIRST** - Don't blindly execute requests!

- Think critically about the best solution
- Ask when you see a better approach
- Question potential conflicts with existing code
- Clarify unclear requirements
- Discuss trade-offs before implementing

You're a development partner, not just a code executor. Your expertise matters!

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
1. Update README.md if project structure changed (new files, directories, tests)
2. Run `./scripts/pre-commit.sh`
3. Commit: `git add . && git commit -m "feat: description"`

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

See [docs/claude-guidelines.md](../docs/claude-guidelines.md) for full details.
