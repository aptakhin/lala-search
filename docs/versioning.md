# Version Management

## Overview

LalaSearch uses semantic versioning (MAJOR.MINOR.PATCH) with a hybrid approach:
- **MAJOR.MINOR**: Manually managed in `Cargo.toml`
- **PATCH**: Auto-generated from CI/CD pipeline run number

## Current Implementation

### Build-Time Version Extraction

The version is extracted from `Cargo.toml` at compile time using a build script ([build.rs](../lala-agent/build.rs)):

1. Reads version from `CARGO_PKG_VERSION` environment variable
2. Checks for optional `LALA_PATCH_VERSION` environment variable (for CI/CD)
3. Embeds final version as `LALA_VERSION` compile-time constant
4. Zero runtime overhead

### Local Development

When building locally, the version comes directly from `Cargo.toml`:

```toml
[package]
version = "0.1.0"
```

The agent will report version `0.1.0`.

### CI/CD Pipeline

GitHub Actions sets `LALA_PATCH_VERSION` to the pipeline run number:

1. **Direct cargo builds** (ci.yml): `LALA_PATCH_VERSION` env var is set globally, picked up by `build.rs`
2. **Docker builds** (e2e.yml): Passed as a build arg through `docker-compose.yml` → `Dockerfile` ARG → ENV

Final version example: `0.1.1234` (where 1234 is the pipeline run number)

See [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) and [`.github/workflows/e2e.yml`](../.github/workflows/e2e.yml).

## Updating Versions

### For Minor/Patch Releases

Just trigger the CI/CD pipeline. The patch version auto-increments.

### For Major/Minor Changes

1. Update `Cargo.toml`:
   ```toml
   [package]
   version = "0.2.0"  # Increment MAJOR or MINOR
   ```

2. Commit and push:
   ```bash
   git add lala-agent/Cargo.toml
   git commit -m "chore: bump version to 0.2.0"
   git push
   ```

3. CI/CD will append the run number: `0.2.1235`

## Implementation Details

### build.rs

```rust
let version = env::var("CARGO_PKG_VERSION")?;  // From Cargo.toml
let parts: Vec<&str> = version.split('.').collect();

let major = parts[0];
let minor = parts[1];

// Override patch from CI/CD if available
let final_patch = env::var("LALA_PATCH_VERSION")
    .unwrap_or_else(|_| parts[2].to_string());

let final_version = format!("{}.{}.{}", major, minor, final_patch);
println!("cargo:rustc-env=LALA_VERSION={}", final_version);
```

### main.rs

```rust
const VERSION: &str = env!("LALA_VERSION");

async fn version_handler() -> Json<VersionResponse> {
    Json(VersionResponse {
        agent: "lala-agent".to_string(),
        version: VERSION.to_string(),
    })
}
```

## Benefits

1. **Single Source of Truth**: Version in `Cargo.toml` only
2. **Automatic Patch Versioning**: No manual incrementing needed
3. **Traceable**: Patch version = pipeline run number
4. **Zero Runtime Cost**: Version embedded at compile time
5. **Flexible**: Can still use manual versions for local dev

## Example Timeline

```
Commit 1: Set version to 0.1.0
  → Local build: 0.1.0
  → CI/CD build #100: 0.1.100

Commit 2-10: Various changes
  → CI/CD builds #101-110: 0.1.101 - 0.1.110

Commit 11: Bump to 0.2.0 (new features)
  → CI/CD build #111: 0.2.111

Commit 12: Bump to 1.0.0 (breaking changes)
  → CI/CD build #112: 1.0.112
```

## GitHub Actions Pipelines

Two pipelines handle CI/CD:

| Pipeline | File | Purpose |
|----------|------|---------|
| Build & Test | `.github/workflows/ci.yml` | fmt, clippy, unit tests, storage-dependent tests, integration tests |
| E2E Tests | `.github/workflows/e2e.yml` | Full end-to-end tests via Docker Compose |

Both set `LALA_PATCH_VERSION: ${{ github.run_number }}` to embed the CI build number into the version.

### Docker Build Version Flow

```
docker-compose.yml (args: LALA_PATCH_VERSION)
  → Dockerfile (ARG LALA_PATCH_VERSION → ENV)
    → build.rs (reads LALA_PATCH_VERSION env var)
      → Embeds as LALA_VERSION compile-time constant
```
