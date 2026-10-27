mod asset_timeout;
mod assets;
mod webp;
pub use asset_timeout::asset_timeout;
pub use webp::{MAX_WEBP_DIMENSION, MAX_WEBP_PIXELS, validate_webp_options};
mod bridge;
mod collector;
mod credentials;
mod envelope;
mod errors;
mod explore;
mod large_values;
mod oauth;
mod payload;
mod plugin_scripts;
pub use plugin_scripts::{PLUGIN_PROTOCOL, PLUGIN_SCRIPTS_BUNDLE, PLUGIN_SCRIPTS_HASH};
mod resources;
mod search;
mod section;
mod snapshot;
mod source;
mod upstream;
mod url;
mod variables;

pub use collector::{
    CollectedParts, CollectionRequest, CollectionScope, CollectionStats, CollectorSession,
    CollectorStep, MAX_COLLECTED_ASSET_BYTES, PlannedCall, ReferencePng, ScreenFailure,
    SectionReadOptions,
};

pub use bridge::{
    AttachedFile, BridgeFigmaClient, BridgeIssue, BridgeJob, BridgeOptions, BridgePathSnapshot,
    BridgePeer, BridgeRole, BridgeServed, BridgeServer, BridgeState, DEFAULT_BRIDGE_PORT,
    FallbackUpstream, Meter, NodeRef, PluginContext, PortOwner, PreferredUpstream, READ_TIME_LIMIT,
};
pub use credentials::{
    ClientCredentialStore, ClientCredentials, CredentialStore, KeyringClientCredentialStore,
    KeyringCredentialStore, MemoryClientCredentialStore, MemoryCredentialStore,
    StoredAuthorization,
};
pub use envelope::{
    FastSnapshotPayload, FastThemePayload, FastTransportStats, ThemePage,
    decode_fast_multi_snapshot, decode_fast_snapshot, decode_fast_theme,
};
pub use errors::{DevupError, ErrorCode, figma_unreachable_hint, is_quota_refusal};
pub use explore::{
    ExploreBounds, ExploreCandidate, ExploreGroup, ExploreKind, ExploreNode, ExploreOptions,
    ExploreResult, TargetKind, classify_explore_node, classify_target, collect_section_notes,
    explore_snapshot,
};
pub use large_values::{
    LargeValueAssembler, LargeValueCursor, LargeValueDescriptor, LargeValueFragment,
    LargeValueReadOptions, LargeValueUnsupported, MAX_LARGE_VALUE_BYTES,
    MAX_LARGE_VALUE_CHUNK_BYTES,
};
pub use oauth::{
    AuthStatus, BrowserOpener, ClientCredentialSource, DEFAULT_CLIENT_NAME, DirectPathSnapshot,
    OAuthManager, SecretString, SystemBrowser, TokenState,
};
pub use payload::{
    CollectedPayload, PayloadCompleteness, PayloadCompletenessReport, PayloadStructure,
    ResourceAudit, validate_payload_context,
};
pub use resources::{
    ResourceKind, ResourceOccurrence, ResourceScope, UsedResourceRefs, collect_used_resource_refs,
};
pub use search::{
    SearchOptions, SearchResult, excluded_duplicate_warnings, node_excluded_reason, search_snapshot,
};
pub use section::{
    BatchLimits, SectionBatch, SectionCandidate, SectionIndex, SectionSummary, build_section_index,
    plan_batches, screen_owners,
};
pub use snapshot::{
    ChildCountMismatch, CompletenessState, Diagnostic, DiagnosticSeverity, FidelityImpact,
    FieldLocation, MissingChild, ParentMismatch, RawNode, SNAPSHOT_CURSOR_ID, Snapshot,
    SnapshotAudit, SnapshotChunk, SnapshotCursor, SnapshotCursorError, TypedNode, merge_chunks,
    read_snapshot_cursor, snapshot_chunk_from_result,
};
pub use source::{
    UpstreamFailureContext, UpstreamFailureKind, classify_upstream_failure, upstream_failure_error,
};
pub use upstream::{
    BatchBudget, BuiltinScript, ExploreReadOptions, FigmaUpstream, ReadToolCall, RemoteFigmaClient,
    SearchReadOptions, SnapshotReadOptions, UpstreamResult,
};
pub use url::{BRIDGE_CURRENT_KEY, BRIDGE_CURRENT_URL, FigmaTarget, is_bridge_only_key};
pub use variables::{ResourceBatch, ResourceStyleRef, UnresolvedResource};
mod asset_render_info;
mod metadata;
mod original_image;
mod reference_compose;
pub use asset_render_info::{annotate_assets, image_fill_audit, suggested_asset_filename};
pub use assets::{
    AssetExportDiagnostics, AssetExportOutcome, AssetExportStage, AssetFormat, AssetManifest,
    AssetManifestEntry, AssetRenderMode, AssetRequest, AssetSelection, AssetStatus, AssetVariant,
    MAX_ASSET_BYTES, PNG_EXPORT_FIELD, SVG_EXPORT_FIELD, asset_exclusion_reason,
    asset_export_from_result, discover_asset_manifest, discover_image_fill_assets,
    discover_web_ready_assets, exported_asset_from_bytes, resolve_asset_selections, source_kind_of,
    validate_asset_options, validate_asset_requests,
};
pub use original_image::{OriginalImage, original_image_from_result};
