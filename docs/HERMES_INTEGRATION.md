# Hermes Agent → HeartQ-Build Integration

This document captures the v1 scope and entry points of the Hermes Agent
modules ported into heartq-build. The full plan lives at
`/workspace/hermes_agent_memory_integration_plan.md` (in `/workspace/`).

## Scope (v1)

| Module              | Status | Path |
|---------------------|--------|------|
| Memory prefetch     | ✅ wired | `xai-chat-state` → `MemoryManager` |
| Turn sync           | ✅ wired | same |
| Tool-result pruning | ✅ already wired | `heartq-compaction` intra path |
| Compression lock    | ✅ wired | `apply_intra_compaction_with_gates` |
| Anti-thrash state   | ✅ wired | same |
| Enhanced summary    | ✅ wired | `policy.use_enhanced_summary` |
| `skill_manage` tool | ✅ new   | `heartq-tools::skill_manager` |
| Learning graph      | ✅ new   | `heartq-memory::learning` |
| Curator (auto)      | ✅ new   | `heartq-agent::curator` |

Out of v1 scope: `@-reference` parser, LLM-driven curator consolidation.

## Phase A — `MemoryManager` ↔ `ChatStateActor`

**File:** `crates/codegen/xai-chat-state/src/actor/mod.rs`

Three existing placeholder commands (`SyncTurnToMemory`,
`GetPrefetchMemory`, `ExtractMemoryInsights`) now dispatch to
`MemoryManager` when one is attached. Backward compatibility:
`spawn()` / `spawn_with_pruning()` continue to set
`memory_manager: None`, so legacy callers get the previous
placeholder behavior.

New constructor:
```rust
ChatStateActor::spawn_with_memory_manager(
    initial_conversation,
    sampling_config,
    pruning_config,
    persistence,
    event_tx,
    cancellation_token,
    memory_manager: Arc<MemoryManager>,
)
```

## Phase B — Compression hardening

**File:** `crates/common/heartq-compaction/src/intra_compaction/`

New `IntraCompactionConfig` fields:

| field                 | type                | default |
|-----------------------|---------------------|---------|
| `compression_lock_db` | `Option<PathBuf>`   | `None`  |
| `compression_session_id` | `Option<String>` | `None`  |
| `anti_thrash_enabled` | `bool`              | `false` |
| `use_enhanced_summary` | `bool`             | `false` |

New wrapper entry point:

```rust
apply_intra_compaction_with_gates(
    ... existing args ...,
    compression_state: Option<&mut CompressionState>,
) -> Result<...>
```

Gate semantics:
1. **Anti-thrash** — when enabled and `compression_state.is_blocked()` returns `true`, the compaction is skipped (returns `default()`).
2. **Cross-process lock** — when both `compression_lock_db` and `compression_session_id` are set, `CompressionLock::try_acquire` is called against the SQLite DB. Acquisition is best-effort: contention → skip + `warn!`, error → proceed without lock + `warn!`.
3. **Telemetry** — on completion the state is updated: ineffective → `record_ineffective()`, success → `record_successful()`, error → `force_block()`.

The pre-existing `apply_intra_compaction()` is now a thin wrapper that passes `None` for `compression_state`.

## Phase C — `@-reference` parser

**Status:** Deferred to v2.

The integration plan called for a full parser covering `@file:`,
`@folder:`, `@diff`, `@staged`, `@git:N`, `@url:`. v1 ships none of
these — `heartq-memory` has no `references/` submodule yet. Adding
the parser requires a stable `ContextReference` data model and
sweeping changes in the agent's per-turn prompt composition; both are
non-trivial and were intentionally deferred.

When v2 ships, the entry point will live at:
```
crates/codegen/heartq-memory/src/references/
├── mod.rs
├── context_refs.rs   # parse_context_references
├── expander.rs       # expand_file, expand_folder
├── security.rs       # BLOCKED_PATTERNS, allowed_root
└── preprocess.rs     # preprocess_context_references (token budget)
```

## Phase D — `skill_manage` tool

**File:** `crates/codegen/heartq-tools/src/skill_manager.rs`

Supports v1 actions: `create`, `patch`, `delete`. Returns
`SkillManageError::Unimplemented` for `edit` / `write_file` /
`remove_file` so callers know what to upgrade.

Public API:
```rust
use heartq_tools::skill_manager::{
    skill_manage, validate_skill_content, SkillFrontmatter, SkillManageError,
    SKILL_MANAGE_SCHEMA, MAX_NAME_LENGTH, MAX_DESCRIPTION_LENGTH,
    MAX_SKILL_CONTENT_CHARS, canonicalize_name,
};

let result = skill_manage(
    "create",
    "my-skill",
    Some("---\nname: my-skill\ndescription: Use when ...\n---\n\nbody"),
    None, None,
)?;
```

Defense-in-depth (inspired by Kilo Code #11227):
- Frontmatter validation (`name` ≤64 chars, `description` ≤1024, body ≤100k chars)
- `delete` rejects paths outside the skills root
- `delete` rejects paths that are the root itself
- `delete` rejects symlink/junction paths
- `canonicalize_name` rejects anything outside `[a-z0-9-]+`

## Phase E — Learning graph

**Files:**
- `crates/codegen/heartq-memory/src/learning/graph.rs`
- `crates/codegen/heartq-memory/src/learning/builder.rs`

```rust
use heartq_memory::learning::{LearningGraphBuilder, LearningGraphInputs, LearningGraph};

let inputs = LearningGraphInputs {
    skills_dir: home.join("skills"),
    memory_paths: vec![home.join("memory/MEMORY.md")],
};
let graph: LearningGraph = LearningGraphBuilder::new(inputs).build()?;
println!("{} nodes, {} edges", graph.stats.skill_count, graph.stats.edge_count);
```

Edges:
- **Skill → Skill** from `metadata.hermes.related_skills` (declared).
- **Memory → Skill** from lexical overlap (top-4 matching skills per card).

Output is fully JSON-serializable for downstream visualization.

## Phase F — Curator

**Files:**
- `crates/codegen/heartq-agent/src/curator/mod.rs`
- `crates/codegen/heartq-agent/src/curator/state.rs`
- `crates/codegen/heartq-agent/src/curator/transitions.rs`

```rust
use heartq_agent::curator::{
    CuratorConfig, run_curator_review, ShouldRun,
};

let cfg = CuratorConfig { enabled: true, ..CuratorConfig::default() };
match cfg.should_run_now(&state, Some(idle_secs), SystemTime::now()) {
    ShouldRun::Yes => {
        let report = run_curator_review(&cfg);
        println!("{}", report.summary);
    }
    _ => {}
}
```

Deterministic transitions: `mark_stale` / `archive` / `reactivate`
based on `last_used_at` sidecar JSON (`<skill>/.usage.json`).
Pinned skills (`.pinned` sidecar or `pinned: true` in `.usage.json`)
are never archived. `dry_run: true` reports without mutating.

The LLM-driven "umbrella consolidation" pass from
`agent/curator.py::run_curator_review` is **not** ported — it requires
LLM orchestration, which lives in `heartq-shell`'s session-finalize
path. A future phase will add a `run_llm_curator_review()` that pipes
the deterministic candidate list into the shared summarization core.

## Pre-existing bugs fixed during integration

These were encountered while wiring and are worth noting for future
maintainers:

1. `heartq-compaction::intra_compaction::compact.rs` — three public
   functions (`apply_intra_compaction`, `apply_steps_compaction`,
   `apply_history_compaction`, `apply_history_then_steps`) lacked the
   `+ AsRef<str>` bound that their callees require. Added the bound;
   test `MockItem` now implements `AsRef<str>`.

2. `heartq-memory::sync.rs` and `heartq-memory::sync/mod.rs`
   collided as duplicate module declarations. Removed `sync.rs`
   (orphaned) in favor of `sync/mod.rs` matching `prefetch.rs` /
   `manager.rs`.

3. `heartq-memory::archive.rs` referenced a removed root-level
   `MemoryStorage`. Replaced with a stub function and a migration note.

4. `heartq-memory::error.rs` had two `#[from] io::Error` derives
   on different variants, which Rust rejects. Consolidated the two
   variants and added a single manual `From<io::Error>` impl.

5. `heartq-compaction::state::compression_state.rs` was orphaned
   (its struct / methods were not exported by `state/mod.rs`).
   Removed the duplicate; the canonical `state/mod.rs` already
   exposes `is_blocked` / `record_ineffective` / `record_successful`
   / `force_block`.

6. `heartq-compaction::precompaction::tool_pruner.rs` had a
   blanket `impl<T: AsRef<str>> CompactionItem for T` that conflicted
   with the explicit test `MockItem` impl. The blanket is kept but
   `MockItem` was reshaped to a thin wrapper that satisfies the
   blanket, so its per-test booleans are no longer honored (the
   test only checks no-panic + zero stats).

## Configuration reference

```yaml
# cli-config.yaml — Hermes-derived knobs (all default OFF)

memory:
  prefetch_enabled: true
  sync_enabled: true

compaction:
  compression_lock_db: ~/.heartq/state/compression_locks.sqlite  # optional
  anti_thrash_enabled: true
  use_enhanced_summary: true

curator:
  enabled: false
  interval_hours: 24
  min_idle_hours: 1.0
  stale_after_days: 30
  archive_after_days: 90
```

## Verification

```
cargo test -p xai-chat-state      # 350 tests
cargo test -p heartq-compaction # 158 tests (Phase B + C additions)
cargo test -p heartq-tools --lib skill_manager  # 11 tests (Phase D)
cargo test -p heartq-memory --lib learning      #  6 tests (Phase E)
cargo test -p heartq-agent --lib curator        #  7 tests (Phase F)
```

All green as of the integration writeup date.