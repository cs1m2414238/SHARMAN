# Sharman

Sharman is a local-first Windows assistant. It opens with one question—**What do you want to do?**—and can route typed or spoken requests to local file search, project progress, media, websites, and optional integrations. A Git repository is no longer required to enter or use the app.

AgentRelay remains inside Sharman as its specialist development layer: shared coding-agent memory, planning, safe execution, verification, project graphs, and handoffs. Project setup is optional and appears only when the user chooses **Add project**.

Version 0.22.1-alpha.1 keeps the hardened local workflow and adds the first personal-data integration: read-only Gmail Inbox Intelligence with durable incremental checkpoints. The separate v0.23 Amazon demo packages add a web console and Streamable HTTP MCP bridge without replacing or weakening the native Windows runtime.

## Live judging demo — updated 22 October 2026

[Open SHARMAN](https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws/) · [Alexa+ simulation](https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws/alexa) · [MCP](https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws/mcp) · [Health](https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws/health).

[Tasks](https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws/tasks) now starts clean, without fake video/project to-dos. The updated UI adds real task creation, clearer navigation, prominent confirmed execution receipts and an explicit required-permission vs agent-ceiling comparison in [Home](https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws/home). [Polish verification](docs/UI_RECORDING_POLISH.md): 17 backend tests, six Tasks and eleven Home browser/accessibility states, container restart and actual public MCP/denial acceptance pass.

Hosted on AWS ECS Express Mode in Sydney with one small non-root task. The public console uses **shared disposable sample data**: do not enter private information. It does not connect to personal Gmail, the Windows database or desktop notifications. The MCP protocol, bounded supervisor, permission checks, reminder records and audit trail are real. App Runner is blocked by the account's organization policy, so the owner-approved ECS fallback is used.

Feature scope and architecture are frozen. [Deployment evidence and maintenance](docs/AWS_JUDGING_DEPLOYMENT.md) records hosting, image scans, the fixed concurrent-start bug and live checks. The [public Ring presentation update](docs/RING_PUBLIC_DEPLOYMENT.md) separates dated local API validation from public connection health and safety simulations. [Submission readiness](docs/SUBMISSION_READINESS.md) tracks Gmail recovery/deadline acceptance, the final RC, three qualified rehearsals and recording. A visible Windows reminder is now owner-observed PASS. Public hosting does not close the remaining native/release gates; Bedrock stays disabled and optional.

**20 October local AWS validation:** Nova 2 Lite (`global.amazon.nova-2-lite-v1:0`, source Region `ap-southeast-2`) now passes real SDK JSON and SHARMAN REST/MCP inference with temporary credentials. Local Only exclusion, denied-lock preservation, output rejection and credential/error fallback pass. Connections only says Connected after a validated call in that process. This opt-in capability is disabled by default and has **not** been enabled or redeployed on the shared public service. [Live evidence and reproduction](docs/AWS_BEDROCK_INTEGRATION.md).

The public inbox provides explicitly requested sample data independently of native Gmail. Official Ring Developer Playground discovery, device status and event-history access passed live local validation. The public Home panel shows current connection health separately from the safety scenarios. Historical validation evidence stays in documentation; live device data requires a configured local connection. Three SHARMAN safety scenarios and Alexa+ speech remain disclosed simulations. Live Ring Motion webhook delivery, physical hardware and Alexa speech are unverified. See the [Ring adapter](docs/RING_ADAPTER.md), [public Ring update](docs/RING_PUBLIC_DEPLOYMENT.md) and [2:50 recording plan](docs/DEMO_SCRIPT.md).

## Architecture

Submission checks: `npm run validate:amazon` runs the automated release preflight.
`npm run rehearse:amazon -- --runs 3` performs three fresh isolated API/MCP smoke
runs without private Gmail, Windows reminder or public-demo writes. Strict RC
version checks and optional live Bedrock validation are documented in
[judge rehearsal tooling](docs/JUDGE_REHEARSAL.md). Automated success is not a
claim that the remaining live-mailbox, installer and owner/UI gates passed.

```text
React + TypeScript UI
          |
      Tauri IPC
          |
Sharman assistant router
  +-- local supervisor + typed tool registry
  +-- local filename/date search
  +-- validated Windows actions
  +-- explicit YouTube search/play actions
  +-- persistent local chat + opt-in personal memory
  +-- personal tasks + background reminder scheduler
  +-- project portfolio summaries
  +-- adaptive local-model profile
          |
AgentRelay specialist core
  +-- read-only Git scanner
  +-- SQLite project memory
  +-- relevance + freshness compiler
  +-- task router + dependency graph
  +-- goal planner + versioned task DAGs
  +-- assignment + soft-lock coordinator
  +-- isolated worktree + merge coordinator
  +-- independent verification + quality gates
  +-- reproducible environment manager
  +-- read-only event replay + provenance engine
  +-- structured explanation + immutable audit ledger
  +-- project graph + impact/risk engine
  +-- source-control sync + external links
  +-- CI normalization + blocker handling
  +-- provider/plugin registry
  +-- normalized integration event gateway
  +-- grounded compression port
  +-- permission policy + action ledger
  +-- sandbox port
          |
  +-------+------------------+
  |                          |
Nebius adapter          Amazon adapter
Nemotron                Alexa+ MCP
Token Factory           Agent Skill / MCP
```

The core is a standalone Rust library. Tauri is an adapter, not the application architecture, so the same library can later power a background daemon, CLI, VS Code bridge and MCP server.

See [docs/SHARMAN_ASSISTANT.md](docs/SHARMAN_ASSISTANT.md) for the assistant capability and trust boundary, and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for AgentRelay dependency rules and milestone history.

See [docs/SHARMAN_SUPERVISOR.md](docs/SHARMAN_SUPERVISOR.md) for the supervisor, tool registry, personal-task database, reminder scheduler, and current MVP command contract.

See [docs/INBOX_INTELLIGENCE.md](docs/INBOX_INTELLIGENCE.md) for the read-only Gmail OAuth flow, Windows Credential Manager boundary, deterministic email action extraction, and Action Inbox audit model.

See [docs/AMAZON_ALEXA_SUBMISSION.md](docs/AMAZON_ALEXA_SUBMISSION.md) for the hackathon web console, Streamable HTTP MCP bridge, exact demo flow, and remaining submission work.

See [docs/SHARMAN_HOME_ARCHITECTURE.md](docs/SHARMAN_HOME_ARCHITECTURE.md), [docs/AWS_BEDROCK_INTEGRATION.md](docs/AWS_BEDROCK_INTEGRATION.md), [docs/HACKATHON_CHANGELOG.md](docs/HACKATHON_CHANGELOG.md), [docs/FRICTION_LOG.md](docs/FRICTION_LOG.md), and [docs/DEMO_SCRIPT.md](docs/DEMO_SCRIPT.md) for **SHARMAN Home** (Ring event simulator + constrained `VisitorAgent` + simulated Alexa+ care workflow + opt-in Amazon Bedrock `ConverseCommand` accessibility explanations with deterministic local fallback), the explicit boundary separating the real SHARMAN runtime from simulated Ring/Alexa+ surfaces, and the submission provenance ledger.

## Included now

- Assistant-first home instead of mandatory repository onboarding
- Typed commands plus optional Windows webview speech input
- Private filename and date search across Downloads, Desktop, Documents and Pictures
- Local audio discovery and playback through the default Windows application
- Explicit “play … on YouTube” commands that resolve and open the best matching result
- Persistent local conversations and inspectable opt-in personal memories in `sharman-memory.db`
- A single local supervisor that emits typed, auditable tool calls rather than giving the model direct OS access
- Persistent personal tasks and one-time reminders with background Windows notifications
- Trusted LeetCode commands, including structured Daily Challenge resolution with a safe fallback
- Read-only Gmail Inbox Intelligence with PKCE OAuth, deterministic action/deadline extraction, one-click local reminders, and a bounded execution audit
- Incremental Gmail history checkpoint, stable action identities, sync recovery state, and explicit-time/relative-window deadline tests
- Optional project onboarding with **Skip for now**, × close, and Escape; SHARMAN never traps a non-project user in AgentRelay setup
- Separate SHARMAN web console with Assistant, **SHARMAN Home** (`/home`), Action Inbox, Tasks, Activity, Connections, and Alexa+ (`/alexa`) simulation surfaces
- **SHARMAN Home** Ring simulator with three deterministic care scenarios (`expected_delivery`, `repeated_unexpected`, `blocked_unlock`), ephemeral `VisitorAgent` permission boundaries (`door.unlock` permanently forbidden), and opt-in Amazon Bedrock (`global.amazon.nova-2-lite-v1:0`) care explanations with deterministic local fallback
- Real Streamable HTTP MCP bridge negotiating MCP `2025-11-25` with eleven permission-annotated tools for supervisor requests, tasks, inbox, reminders, allowed URLs and Home workflows/explanations
- Validated `http`/`https` website actions with explicit confirmation
- Cross-project task progress calculated from SQLite truth, never model guesses
- RAM/CPU-based local model recommendation from 1–3B through 20–32B quantized tiers
- Optional Ollama discovery and local-only conversational fallback; deterministic code remains in control of every action
- Honest capability states for email, voice and local-model runtime setup
- Tauri 2 desktop shell
- Guided first-run and repeatable project onboarding with native repository selection
- Git/Rust/Node/Tauri/agent discovery, stack detection, context import, memory initialization and health check
- General, Agents, Projects, Permissions, Context, Verification, Integrations, Environment, Notifications, Backups and Advanced settings
- Project health dashboard for repository, context, agents, conflicts, startup recovery, verification, environment, backups and GitHub
- Sanitized diagnostics ZIP export that excludes secrets, source code, SQLite data and command output by default
- Consistent SQLite memory backups stored outside tracked repositories
- Startup integrity check and fail-closed crash recovery for in-flight sessions, actions, verifier runs, missing worktrees, and stale caches
- Pre-migration SQLite snapshots with automatic rollback and preservation of the failed database image
- Repeatable RC qualification command with structured evidence, performance baseline, and optional isolated installer execution
- NSIS per-user Windows installer with Start menu integration, optional desktop/startup shortcuts and clean uninstall
- Windows system tray lifecycle with live agent/task/project counts and persistent pause/resume controls
- Windows process inspection through a provider-neutral `AgentDetector` contract
- Manifest-driven detection for Claude, Codex, Gemini, Cursor and GitHub Copilot
- Explicit per-project background-tracking toggle
- Native recursive repository watcher with generated/build-directory filtering
- Automatic session start from process working directory or command-line project paths
- Automatic session end on process exit or configurable 1–240 minute idle timeout
- Native notifications for detected sessions, ready handoffs, stale context, conflicts and blockers
- React + TypeScript Repository, Goals, Plan, Orchestration, Execution, Environment, Impact, Integrations, Activity, Timeline, Audit, Handoff, Conflicts, Context and Actions views
- Rust Git scanner using fixed read-only commands
- Command timeout and hidden Git child processes on Windows
- Branch, modified/added/deleted files, diff statistics and recent commits
- UUID-backed projects, goals, tasks, decisions, events and agent sessions
- Append-only event ledger protected by SQLite triggers
- Git scans persisted as repository, file-change and commit events
- Editable/archivable goals and completable provider-neutral tasks
- Structured decisions with rationale, alternatives and affected components
- Deterministic `AGENT_CONTEXT.md` preview and generation from SQLite
- Relevance scoring from active task, profile, recency, touched files, decision dependencies and freshness
- Critical, Active, Recent, Historical and Archived context tiers
- Compact (2k), Normal (6k) and Deep (12k) token-budget presets
- Provider-neutral architecture, implementation, product and balanced context profiles
- Verifiable source references for decisions, tasks, events, commits and files
- Click-to-explain “Why is this in my context?” UI
- Superseded and invalidated statements excluded from normal context; explicitly labeled in Deep history
- Optional NVIDIA Nemotron compression through Nebius Token Factory's OpenAI-compatible endpoint
- Structured model-output validation with deterministic fallback on network, format, citation or budget failure
- Manual provider-neutral agent sessions with captured starting and final Git state
- Session-linked file, commit, task, decision, idle, and lifecycle events
- Persisted deterministic handoffs with completed work, remaining tasks, decisions and blockers
- Automatic context regeneration when a session ends
- Durable conflict records with open, acknowledged and resolved lifecycle states
- Freshness projections for context, decisions, handoffs and completed tasks
- Cross-agent file overlap detection across retained handoffs
- Explicit decision supersession with stale-decision exclusion from generated context
- Completed-task revalidation warnings when later sessions revisit the same files
- Stale-context detection from newer project events or a mismatched Git HEAD
- Conflict warnings embedded in generated handoff context without using an LLM as truth
- Last selected repository and full memory persistence across restarts
- Provider-neutral `ContextCompressor` and `ExecutionSandbox` interfaces
- Per-project capability policy with explicit Allow, Ask and Deny modes; new projects default tests and builds to Ask
- Locked source-write, Git commit, Git push and file-delete permissions
- Trusted test/build command resolution for Cargo, npm, Python, Go, Maven and Gradle projects
- Nebius Token Factory Sandbox adapter through the official ConTree CLI
- Disposable remote execution with explicit argv, timeouts and bounded captured output
- Durable action request, denial, start, success and failure events
- Sandbox results promoted into relevance-ranked agent context
- Safe unavailable state when Token Factory is not configured; there is no local-shell fallback
- Configurable per-project capability profiles for Claude, Codex, Gemini, Cursor and Copilot
- Deterministic task routing from task capabilities, profile strengths, allowed actions and context limits
- Explicit suggested, assigned, running, waiting, blocked and completed assignment states
- Dependency-cycle validation for `depends_on` and `blocked_by` relationships
- `produces` and `consumes` artifact-flow metadata
- Parallel-safety assessments over dependencies, artifact flow and workspace paths
- User-approved soft locks for repository-relative path patterns
- Lock-overlap prevention before assignments are approved
- Explicit routed-session start; AgentRelay never launches an agent autonomously
- Automatic next-agent recommendation and reasoning in completed handoffs
- Orchestration delivery graph, profile editor, lock list and parallel-safety UI
- Open, validated `ProviderId` values instead of a closed provider enum
- Versioned Provider/Plugin SDK contracts for detection, events, actions, context and handoffs
- Validated TOML manifests with duplicate-ID and SDK-version rejection
- Runtime manifest directory plus built-in manifests compiled into the core
- Integration registry grouped into providers, developer tools and automation
- Generic normalized event gateway with auditable integration and event-type provenance
- Built-in GitHub issue/PR, local Git, CI/check/build and VS Code event adapters
- Plugin action proposals routed through the same deny-by-default policy and remote sandbox
- Manifest-driven Windows detection with no provider switches in the desktop shell
- On-demand graph projection across goals, tasks, files, modules, tests, decisions and agent sessions
- Typed `Contains`, `Touches`, `BelongsTo`, `DependsOn`, `Validates`, `Affects`, `ModifiedBy` and `Blocks` relationships
- Bounded Rust, TypeScript/JavaScript and Python source-import inspection with no code execution
- Transitive reverse-dependency impact analysis for repository files
- Mapped affected modules, tasks, tests, active decisions, sessions and overlapping conflicts
- Deterministic 0–100 change-risk score with visible factor-by-factor point contributions
- Pre-edit warning and evidence explorer in the Impact view
- Provider-neutral `SourceControlProvider` contract with a read-only GitHub implementation
- GitHub repository discovery from `origin` plus an explicit `owner/repository` override
- GitHub issues imported idempotently as linked AgentRelay tasks with labels, assignees, state and inferred branch
- Pull-request snapshots with changed files, commits, reviewers, merge state and check runs
- Bidirectional task/session links for issues, pull requests, review comments and CI checks
- Review comments and change-request reviews converted once into durable actionable tasks
- Deterministic `BuildStarted`, `BuildPassed`, `BuildFailed`, `TestFailed`, `LintFailed`, `SecurityCheckFailed`, `PrReady` and `PrMerged` events
- Automatic CI blockers for linked active tasks, with recovery and merge completion handling
- Active GitHub CI failures included in generated session handoffs
- Pull Requests and CI views with tasks, agent sessions, decisions, checks and blockers
- AgentRelay-owned Git worktrees and branches for approved routed tasks
- Durable task-to-worktree bindings with provider, branch, path, base/head revisions, session and lifecycle state
- Parallel admission checks over dependencies, produced/consumed artifacts, claimed paths and active architectural decisions
- Isolated routed sessions recorded under the original project instead of creating duplicate project memory
- Deterministic clean-tree, changed-file, ancestry, parallel-safety and mergeability validation checks
- Optional permission-gated test/build validation in the existing remote execution sandbox
- Non-mutating merge preparation through `git merge-tree`
- Explicit user-confirmed merge with a clean-main-worktree guard
- Post-merge worktree cleanup that retains the AgentRelay branch and all SQLite history
- `WorktreeReady`, `ValidationPassed`, `ValidationFailed`, `MergeReady`, `MergeConflictDetected`, `MergeCompleted`, and `WorktreeArchived` events
- Deterministic goal decomposition into architecture, implementation, persistence, configuration/migration, testing, and documentation tasks
- Versioned task DAGs with expected files, affected modules, dependency counts, explainable conflict risk, and parallel stages
- Planner validation for dependency cycles, missing artifacts/decisions, critical overlaps, stale context, critical conflicts, and missing capable profiles
- Provider recommendations delegated to the existing configurable capability router rather than provider-name rules
- Explicit plan approval that materializes ordinary AgentRelay tasks without assigning or launching agents
- Automatic recovery-plan proposals after an approved worktree fails deterministic validation
- Downstream blocking, recovery-task insertion, dependency rewiring, changed-task/dependency audit data, and retained version history
- Plan view with validation findings, routing/risk evidence, approval boundary, failure reporting, and version history
- Independent verification suites bound to the exact task, worktree, branch, commit, and environment fingerprint
- Required `format`, `lint`, `test`, `build`, frontend-build, desktop-build, architecture, acceptance, and environment gates inferred from a checked-in manifest
- Durable `NOT_RUN`, `RUNNING`, `PASSED`, `FAILED`, and `WAIVED` gate evidence with argv, exit code, duration, bounded stdout/stderr summaries, and timestamps
- Automatic recovery-plan proposals or repair tasks after verification failure
- Merge preparation blocked until the current workspace head has a passing or explicitly waived verification suite
- TOML environment manifests for Native Windows, sandboxed-local, and container profiles
- SHA-256 environment fingerprints over runtime/tool observations, lockfiles, dependency manifests, public configuration, secret presence, OS, and architecture
- Secret-reference-only persistence: values never enter SQLite, events, evidence, or `AGENT_CONTEXT.md`
- Fingerprint-scoped Cargo/npm/build caches with invalidation and disposable verifier cleanup
- Environment drift checks against required versions and last verified provenance
- Environment view with runtime status, drift evidence, configured secret references, cache provenance, and last verification
- Strictly read-only replay projections selected by event, session, or timestamp; replay never invokes Git or mutates SQLite
- Durable context versions with SHA-256 content hash, session/task binding, commit, branch, plan version, and environment fingerprint
- Event-bounded reconstruction that preserves SQLite insertion order even when several events share one timestamp
- Historical goal/task/decision/session state plus Git dirty state, changed files, blockers, plans, environments, and verification gates
- Explicit `Exact` versus `LegacyBestEffort` replay accuracy with warnings for pre-v0.16 compact event payloads
- Point-to-point comparison for new/superseded decisions, completed tasks, changed files, blockers, context, plan, and environment
- “Why did this change?” provenance over entity-linked events, sessions, tasks, and verification evidence
- Timeline UI with exact historical `AGENT_CONTEXT.md`, project-state panels, comparisons, and provenance explorer
- Immutable explanation records protected from update and deletion by SQLite triggers
- Structured reason codes, human-readable details, optional score contributions, and typed evidence references
- Deterministic audit coverage for planning, assignment, parallel safety, risk, context, conflict, verification, permissions, replanning, merge readiness, freshness, and decisions
- Plan approval and action-policy evidence that preserves the user-control boundary
- Audit view with category/search filters, subject-focused deep links, outcomes, reason scores, and evidence identifiers
- Contextual **Why?** controls on plans, routed tasks, blocked task pairs, impact risk, verification suites/gates, decisions, permissions, actions, conflicts, and compiled context items
- Separate Nebius and Amazon integration modules
- Git-backed and SQLite-backed Rust tests

Run `./scripts/Invoke-RcQualification.ps1` from PowerShell 7 and review [the RC criteria](docs/RELEASE_CANDIDATE.md), [compatibility matrix](docs/COMPATIBILITY.md), and [privacy policy](docs/PRIVACY_AND_TELEMETRY.md). The local unsigned NSIS build is test evidence, not a public v1.0 installer. General agent-controlled source-changing actions remain outside the local trust boundary; the MCP bridge exposes a small, permission-annotated personal-productivity tool set instead.

See [docs/PLUGIN_SDK.md](docs/PLUGIN_SDK.md) for the manifest format, compatibility policy, and extension boundaries.

## Repository structure

```text
agentrelay/
├── Cargo.toml
├── crates/
│   └── agentrelay-core/       Reusable local domain/application core
│       ├── manifests/         Built-in provider/tool/automation manifests
│       └── src/
│           ├── sdk.rs         Stable plugin contracts and integration registry
│           ├── git.rs         Git process execution and parsing
│           ├── domain/        Project-memory domain types
│           │   └── audit.rs   Structured explanation and evidence contract
│           ├── memory.rs      SQLite storage and schema migrations
│           ├── replay.rs      Read-only event replay, comparison and provenance
│           ├── conflict.rs    Deterministic conflict/freshness rules
│           ├── orchestration.rs Capability routing and parallel-safety rules
│           ├── worktree.rs    Managed worktree and approved-merge boundary
│           ├── graph.rs       Project graph, impact analysis and risk scoring
│           ├── source_control.rs Provider-neutral source-control boundary
│           ├── detection.rs   Provider-neutral process-detection contract
│           ├── context/       Relevance compiler, validation and renderer
│           ├── ports.rs       AI and sandbox boundaries
│           └── integrations/  GitHub, Nebius and Amazon adapters
├── frontend/
│   ├── src/                   React UI, onboarding/settings and typed IPC client
│   └── src-tauri/             Tray, watcher, diagnostics, installer and native shell
├── sharman-web/               Amazon demo-first React console and Alexa+ simulation
├── sharman-mcp/               Streamable HTTP MCP bridge plus localhost demo API
├── docs/
└── docker/                    Reserved packaging notes
```

## Windows prerequisites

- Node.js 20.19+ or 22.12+
- Git on `PATH`
- Rust stable with the MSVC host toolchain
- Microsoft C++ Build Tools with **Desktop development with C++**
- Microsoft Edge WebView2 (normally already present on Windows 10/11)

Install rustup:

```powershell
winget install --id Rustlang.Rustup --source winget
rustup default stable-msvc
```

## Run the desktop app

```powershell
cd agentrelay/frontend
npm install
npm run tauri dev
```

Sharman opens directly to the assistant home. No repository is required. Choose **Add project** only when you want AgentRelay project memory and coding-agent coordination. Closing the window hides Sharman to the system tray; use **Exit** from the tray menu to stop it.

## Build the Windows installer

```powershell
cd agentrelay/frontend
npm run tauri build -- --bundles nsis
```

The per-user installer is written to `target/release/bundle/nsis/`. It adds a Start menu entry and asks whether to create a desktop shortcut or launch at sign-in. Uninstall removes those registrations and asks whether local project memory/settings/backups should also be deleted. Release artifacts are unsigned unless a Windows code-signing certificate is supplied to the build environment.

## Connect GitHub delivery

Open **Integrations**, optionally enter `owner/repository`, and choose **Sync GitHub**. Without an
override, AgentRelay reads the repository's `origin` URL. Public repositories can be read without a
token. Set `GITHUB_TOKEN` for private repositories or higher API limits; the token is read from the
process environment and is never written to SQLite.

```powershell
$env:GITHUB_TOKEN = "your-fine-grained-read-token"
npm run tauri dev
```

The adapter uses read-only issue, pull-request, file, commit, review-comment, review, and check-run
endpoints. `AGENTRELAY_GITHUB_API_URL` and `AGENTRELAY_GITHUB_WEB_URL` support GitHub Enterprise;
`AGENTRELAY_GITHUB_API_VERSION` overrides the default API version. A manual sync is intentionally
explicit in v0.11; background webhooks/polling and GitHub write-back are not enabled. To stay within
anonymous REST limits, an unauthenticated sync enriches the six most recently updated PRs; token-backed
syncs enrich twenty.

## Run collaborative execution

In **Orchestration**, define task dependencies, artifact flow, claimed paths, and an approved agent.
Choose **Prepare workspace** to run the parallel-safety gate and create a branch under
`agentrelay/<provider>/...` with a sibling worktree under `.agentrelay-worktrees/`. Continue in the
**Execution** view:

1. Start the isolated session and direct the assigned agent to the displayed worktree path.
2. Finish the session after the agent commits its branch changes.
3. Validate Git ancestry, cleanliness, changed files, overlap, mergeability, and optionally tests/builds.
4. Prepare the merge without modifying the project checkout.
5. Explicitly approve the merge; AgentRelay refuses while the main checkout is dirty.
6. Clean up the merged worktree. Its branch and all recorded evidence remain available.

AgentRelay coordinates and records external agents; it does not silently launch them. Worktree paths
are derived internally, constrained to AgentRelay's dedicated sibling directory, and never accepted
as arbitrary cleanup targets from the UI.

## Enable Nemotron compression

Deterministic context compilation works without credentials. To enable remote compression, set both variables before starting AgentRelay:

```powershell
$env:NEBIUS_API_KEY = "your-token-factory-key"
$env:AGENTRELAY_NEMOTRON_MODEL = "the-current-NVIDIA-Nemotron-model-id"
npm run tauri dev
```

`NEBIUS_BASE_URL` can override the default `https://api.tokenfactory.nebius.com/v1` endpoint. AgentRelay refuses non-Nemotron model IDs for this adapter, never persists the API key, and keeps the deterministic selected facts when remote compression or validation fails.

## Enable controlled sandbox actions

Install and authenticate the official ConTree CLI, then choose an OCI image containing the tools your project needs:

```powershell
$env:CONTREE_TOKEN = "your-token-factory-token"
$env:CONTREE_PROJECT = "your-nebius-project-id"
contree auth -y --type jwt

$env:AGENTRELAY_SANDBOX_IMAGE = "your-sandbox-image"
# Optional when contree is not on PATH:
$env:AGENTRELAY_CONTREE_CLI = "C:\Tools\contree.exe"
npm run tauri dev
```

Enable **Run tests** or **Run build** in the Actions view for each project. The core accepts only an `ActionKind`; it derives a separate executable-and-arguments array from `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `pom.xml`, or `gradlew`. The adapter uploads the repository to `/workspace`, uses a disposable sandbox, and records the remote result. Without a configured image or authenticated CLI, the request is recorded as unavailable and nothing runs locally.

## Verify

From the `agentrelay` directory:

```powershell
cargo fmt --all --check
cargo test -p agentrelay-core
cargo clippy --workspace --all-targets -- -D warnings

cd frontend
npm run build
npm run tauri build -- --no-bundle
```

`npm run dev` starts a browser-only visual preview. Git scanning is intentionally unavailable there because the application has no HTTP backend; native operations are reachable only through Tauri IPC.

## Security boundary

The core does not accept arbitrary command text from the UI, an agent, or a plugin. It accepts a named action, applies the persisted project policy, derives a fixed argv from trusted project manifests, and invokes the `ExecutionSandbox` port. Plugin `ActionAdapter::execute` returns an `ActionProposal`; it cannot execute a process. The GitHub adapter only returns normalized records and cannot write SQLite or mutate GitHub. Test/build execution has no host fallback. General agent-requested source modification, commit, push and delete capabilities remain disabled in v0.13. Worktree creation/removal and the confirmation-gated merge are separate coordinator operations with internally derived Git arguments, owned-branch validation, clean-tree checks, and managed-path containment.
