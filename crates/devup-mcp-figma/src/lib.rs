mod assets;
mod collector;
mod credentials;
mod envelope;
mod errors;
mod explore;
mod large_values;
mod oauth;
mod payload;
mod resources;
mod search;
mod snapshot;
mod source;
mod upstream;
mod url;
mod variables;

pub use credentials::{
    CredentialStore, KeyringCredentialStore, MemoryCredentialStore, StoredAuthorization,
};
pub use envelope::{
    FastSnapshotPayload, FastThemePayload, FastTransportStats, decode_fast_snapshot,
    decode_fast_theme,
};
pub use errors::{DevupError, ErrorCode};
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
pub use oauth::{AuthStatus, BrowserOpener, OAuthManager, SecretString, SystemBrowser};
pub use payload::{
    CollectedPayload, PayloadCompleteness, PayloadCompletenessReport, PayloadStructure,
    ResourceAudit, validate_payload_context,
};
pub use resources::{
    ResourceKind, ResourceOccurrence, ResourceScope, UsedResourceRefs, collect_used_resource_refs,
};
pub use search::{SearchOptions, SearchResult, search_snapshot};
pub use snapshot::{
    ChildCountMismatch, CompletenessState, Diagnostic, DiagnosticSeverity, FieldLocation,
    MissingChild, ParentMismatch, RawNode, Snapshot, SnapshotAudit, SnapshotChunk, TypedNode,
    merge_chunks, snapshot_chunk_from_result,
};
pub use source::{
    SelectedSource, SourcePolicy, UpstreamFailureContext, UpstreamFailureKind,
    classify_upstream_failure, fallback_allowed, fallback_allowed_for_error,
    upstream_failure_error,
};
pub use upstream::{
    BuiltinScript, ExploreReadOptions, FigmaUpstream, ReadToolCall, RemoteFigmaClient,
    SearchReadOptions, SnapshotReadOptions, UpstreamResult,
};
pub use url::FigmaTarget;
pub use variables::{ResourceBatch, ResourceStyleRef, UnresolvedResource};
mod metadata;
pub use assets::{
    AssetFormat, AssetManifest, AssetManifestEntry, AssetRequest, AssetSelection, AssetStatus,
    MAX_ASSET_BYTES, asset_export_from_result, discover_asset_manifest, resolve_asset_selections,
    validate_asset_requests,
};
pub use collector::{
    CollectedParts, CollectionRequest, CollectionScope, CollectionStats, CollectorSession,
    CollectorStep, PlannedCall,
};
