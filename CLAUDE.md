# Claude Code Guidelines for LalaSearch

## Critical Rules

### 1. ASK QUESTIONS FIRST

**IMPORTANT**: Don't blindly execute requests. Always think critically about the best solution.

Ask clarifying questions when:
1. **Multiple approaches exist** or a better alternative is apparent
2. **Potential conflicts** with existing patterns or architecture
3. **Missing context** or **unclear requirements**
4. **Trade-offs** that need discussion

You're a development partner, not a code executor. Suggest better alternatives and point out issues. Balance: ask when it adds value, don't over-question trivial decisions.

### 2. TEST-DRIVEN DEVELOPMENT

All features follow: **Analyze → Red → Green → Refactor → (repeat)**

1. **Analyze**: Identify corner cases, error scenarios, boundary conditions, concurrency issues
2. **Red**: Write ONE failing test (`test_<scenario>_<expected_outcome>`)
3. **Green**: Write MINIMUM code to pass
4. **Refactor**: Improve without changing behavior (skip if clean)

### 3. ALWAYS COMMIT CHANGES

**Every completed task must end with a git commit. Never consider work done until it is committed!**

Optional: when work is collaboratively produced by multiple coding agents, the commit message may include `Co-authored-by:` trailers so results can be compared later. Only add co-author trailers when they are intentional for that task.


On Windows, the script runs all checks inside Docker automatically.

## Open Source Only

All dependencies must be open source. Avoid: ScyllaDB (proprietary since Dec 2024), MinIO (proprietary since Feb 2026). Use PostgreSQL, SeaweedFS, Meilisearch, Redis, etc. If no open source option exists, ask first.

## Code Quality

- **Format**: `cargo fmt` / **Lint**: `cargo clippy -- -D warnings` (zero warnings)
- **Early returns** over nested match/if ladders — flat success path
- **Enums over magic strings** for config values and states
- **No hardcoded defaults** — use env vars with `.expect()`, maintain `.env.example`
- **No hardcoded values in comments** — reference the config source instead
- **No trivial comments** — don't restate what the code does (e.g., `// Get user` before `get_user_by_id()`). Only comment to explain *why*, document non-obvious behavior, or clarify complex logic.
- **Cross-platform**: use env vars, no platform-specific paths
- **Natural pluralization in web UI** — use proper English plurals, not parenthetical hacks. Write "10 pages", "0 pages", "1 page" — never "10 page(s)"

## Error Handling

- Fail the entire operation on errors — don't silently skip
- Return failed items to queue with `attempt_count += 1`
- Use `{:#}` for error logging (full anyhow chain)
- Add operational context to `.with_context()` (host, port, IDs) — not just "Failed to do X"

## Logging

- `println!` for info, `eprintln!` for errors. Prefix: `[AUTH]`, `[EMAIL]`, `[DB]`, etc.
- Always include `user_id` when available
- **Never log full emails** — use `anonymize_email()` from `services::logging`
- Never log tokens, passwords, session hashes, or PII

## Database

- **No N+1 queries** — use JOINs, `ANY($1)`, or batch fetches instead of loops with queries
- **Migrations**: forward-only, managed by sqlx in `lala-agent/migrations/`. No rollbacks.
  - Add new migrations as `NNNN_description.sql` (sequential numbering)
  - Use `CREATE TABLE IF NOT EXISTS`, `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` — never drop tables
  - Don't allow long locks migrations to run. There should be an external approval for this.
  - Run with `lala-agent migrate` before starting the server
- New nullable columns → `Option<T>` in Rust structs

## Testing Tiers

| Tier | Location | Speed | Marker | Command |
|------|----------|-------|--------|---------|
| 1: Unit | `src/**/*.rs` | < 500ms | None | `cargo test --lib` |
| 1: Storage | `src/**/*.rs` | < 500ms | `#[ignore]` | `cargo test --lib -- --ignored` |
| 2: Integration | `tests/*.rs` | Slower | None | `cargo test --test '*'` |

- Integration tests must reuse production code, not reimplement workflows
- Every test must set up its own prerequisites — no relying on pre-existing data
- One test, one behavior. Merge tests with identical inputs into one
- Tests must be deterministic — clean up or use unique identifiers

## Docker

- Use `docker compose` (not `docker-compose`)
- Always use `--build` flag: `docker compose up -d --build`

## Completing Features

1. Update `README.md` if structure changed
2. Update relevant `docs/` files
3. `cargo fmt`
4. Commit

## Project Structure

```
lalasearch/
├── CLAUDE.md
├── docs/                         # overview.md, docker.md, versioning.md
├── lala-agent/
│   ├── src/
│   │   ├── main.rs               # CLI entry point (migrate / serve subcommands)
│   │   ├── lib.rs                # Library root
│   │   ├── models/               # Data models
│   │   └── services/             # Business logic
│   ├── migrations/               # SQL migrations (forward-only, sqlx)
│   ├── tests/                    # Integration tests (Tier 2)
│   ├── Dockerfile
│   └── Cargo.toml
├── docker-compose.yml
├── .env.example
└── scripts/pre-commit.sh
```

## Windows PDB Linker Error (LNK1318)

Run build within docker.

## Commands Reference

```bash
cargo test --lib                    # Unit tests
cargo test --lib -- --ignored       # Storage-dependent tests
cargo test --test '*'               # Integration tests
cargo fmt                           # Format
cargo clippy -- -D warnings         # Lint
./scripts/pre-commit.sh             # Pre-commit (auto Docker on Windows)
lala-agent migrate                  # Apply database migrations
lala-agent serve                    # Start HTTP server (default)
```
