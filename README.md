# Lys Ecosystem

A unified build hub that takes source code, runs developer hooks, and produces
executables and installers for Windows, macOS, Linux, BSD, and more.

This ecosystem is split into five focused projects that work together:

## Projects

- **hub**: The main hub service/CLI. Coordinates builds, queues, and workflow.
- **uvd**: The core execution engine. Runs hooks and produces cross-platform
  artifacts.
- **lys (vcs)**: Local-first version control for source and build metadata.
  Stores hooks and project configuration.
- **syl (api)**: REST API layer. Manages auth, builds, artifacts, logs, and
  integrations.
- **silex (sdk)**: Client libraries that wrap the Syl API with typed, ergonomic
  calls for apps, CLIs, and services.

## Flow

```
source code
  -> lys (vcs)
  -> hub (orchestrator)
  -> uvd (engine) runs hooks and builds
  -> artifacts (executables + installers)

clients
  -> silex (sdk)
  -> syl (api)
  -> hub
  -> uvd
```

## Repo layout (this workspace)

- `uvd/`  - engine implementation
- `lys/`  - VCS implementation
- `syl/`  - API implementation

Hub and Silex may live in separate repositories or packages, depending on
deployment and language targets.

## Scope

The long-term goal is a single developer hub that can:

- ingest source code (from Lys or other VCS adapters)
- execute hooks deterministically
- build and package for multiple platforms
- expose a stable API and SDK for automation

If you want me to extend this README with install steps, API endpoints, or SDK
usage examples, tell me which parts you want first.
