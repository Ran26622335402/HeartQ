//! Shared, transport-agnostic compaction engine.
//!
//! # Modules
//!
//! This crate is organized into several module groups:
//!
//! ## Compaction Strategies
//! - [`code_compaction`] - Full session replacement (heartq-build)
//! - [`intra_compaction`] - Tail-keep per-step pass (HeartQ chat)
//! - [`inter_compaction`] - Chunked between-turn pass (HeartQ chat)
//!
//! ## Precompaction
//! - [`precompaction`] - Tool result pruning before compression
//!
//! ## Obligations (Phase 1 - OpenSquilla Integration)
//! - [`obligations`] - Obligation extraction system (13 types of high-signal facts)
//!
//! This crate is the `compaction-core`: shared policy, prompts, selection,
//! and assembly. Host-specific trigger wiring, transport, persistence /
//! replay / rewind, state commit, metrics backends, and prompt-variant forks
//! stay in each product host (for example `heartq-shell`).
//!
//! The crate depends on **neither** a conversation-type crate nor
//! `heartq-sampling-types`. It is decoupled from both HeartQ chat and
//! heartq-build hosts through a small set of trait seams:
//!
//! - [`CompactionItem`] / [`CompactionRole`] / [`CompactionItemBuilder`] —
//!   abstracts a single turn and its reconstruction.
//! - [`ItemTokenCounter`] — trusted token counting per host.
//! - [`CompactionSampler`] — the LLM call.
//! - [`CompactionStreamProc`](intra_compaction::CompactionStreamProc) —
//!   state commit for intra-compaction.
//! - [`IntraCompactionObserver`](intra_compaction::IntraCompactionObserver) /
//!   [`InterCompactionObserver`](inter_compaction::InterCompactionObserver)
//!   — host metrics.
//!
//! Compaction styles live in their own modules:
//!
//! - [`code_compaction`] — heartq-build's whole-session **full-replace**
//!   subsystem (prompt/summary/failure/config, assemble, orchestration).
//! - [`intra_compaction`] — HeartQ chat's tail-keep, per-step pass.
//! - [`inter_compaction`] — HeartQ chat's chunked, between-turn pass.
//!
//! Compaction-type content (parallel subfolders): [`steps`] (the step-level
//! prompt) and [`history`] (filtering, history prompts, validation +
//! user-query preservation).
//!
//! Shared seams/primitives: [`item`], [`token`], [`sampler`],
//! [`prompt::CompactionPrompt`], [`select`] (tool-pair-safe tail-keep
//! selection — shared by the intra `Steps` and `History` targets, so it stays
//! neutral at the crate root rather than under `steps`), and [`reminder`]
//! (active-agent-state `<system-reminder>` formatting shared by HeartQ chat and
//! heartq-build; hosts still own snapshotting and host-only sections).

pub mod code_compaction;
pub mod continuation; // Phase 4: OpenSquilla continuation decision
pub mod history;
pub mod inter_compaction;
pub mod intra_compaction;
pub mod item;
pub mod lock;
pub mod obligations; // Phase 1: OpenSquilla obligation extraction
pub mod precompaction;
pub mod prompt;
pub mod reminder;
pub mod sampler;
pub mod safety; // Phase 3: OpenSquilla safety mode system
pub mod select;
pub mod state;
pub mod steps;
pub mod structured_summary; // Phase 2: OpenSquilla structured summary
pub mod token;

/// Shared code default for the dedicated compaction model name.
///
/// Override order (highest first):
/// 1. the agent's `compaction_model_name` setting (non-blank)
/// 2. service / harness config YAML
/// 3. this constant
pub use intra_compaction::DEFAULT_COMPACTION_MODEL_NAME;

// heartq-build's full-replace subsystem now lives under `code_compaction`;
// re-exported at the crate root so consumers keep a stable public API.
pub use code_compaction::{
    CompactedHistoryParts, DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT, FailureKind,
    FullReplaceAttemptOutcome, FullReplaceConfig, FullReplaceContext, FullReplaceError,
    FullReplaceObserver, FullReplaceOutput, FullReplaceSummary, MIN_SUMMARY_SEED_CHARS,
    SELF_SUMMARIZATION_PROMPT, SummaryPromptKind, apply_full_replace_compaction,
    assemble_compacted_history, build_summary_prompt, build_summary_prompt_kind,
    classify_http_status, classify_stream_event_error, format_compact_summary,
    format_compact_summary_content, is_context_length_error, is_degenerate_summary,
    sample_full_replace_summary, wrap_user_query,
};
pub use item::{
    CompactionFileRef, CompactionItem, CompactionItemBuilder, CompactionItemFactory, CompactionRole,
};
pub use prompt::CompactionPrompt;
// Reminder types/formatters: import from `reminder::` (borrowed views).
// Only the summary-injection helper is re-exported at the crate root — both
// intra FullReplace and inter already use it by this name.
pub use reminder::append_reminder_block;
pub use sampler::{CompactionSampleError, CompactionSampler, LlmCompactionOutput};
pub use select::{SplitPlan, select_turns_to_compact};
pub use steps::format_compaction_prompt;
pub use token::ItemTokenCounter;

// Phase 2.6: Precompaction, lock, and state module re-exports
pub use precompaction::{ToolPrunerConfig, ToolResultPruner, PruneStats};
pub use lock::{CompressionLock, CompressionLockError, init_lock_table, LockRefresher};
pub use state::CompressionState;

// Phase 1: Obligations module re-exports
pub use obligations::{
    CompactionEntry, CompactionObligation, ObligationExtractor, ObligationKind, CoverageResult,
    CoverageStatus, SimpleEntry, ToolCallInfo, verify_summary_coverage, MAX_OBLIGATION_VALUE_CHARS,
    MAX_COMMAND_CHARS, MAX_CRITICAL_CARRY_FORWARD,
};

// Phase 2: Structured Summary module re-exports
pub use structured_summary::{
    StructuredCompactionSummary, StructuredSummaryBuilder, FileOrArtifact, ToolResultRef,
    DecisionOrRationale, KnownFailure, render_structured_summary,
};

// Phase 3: Safety module re-exports
pub use safety::{SafetyMode, CompactionReceipt, SafetyGuard, SafetyDecision, CompactionDecision};

// Phase 4: Continuation module re-exports
pub use continuation::{ContinuationAction, CompactionContinuationDecision, decide_compaction_continuation};
