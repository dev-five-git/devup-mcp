mod collector;
mod credentials;
mod errors;
mod explore;
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
pub use errors::{DevupError, ErrorCode};
pub use explore::{
    ExploreBounds, ExploreCandidate, ExploreGroup, ExploreKind, ExploreNode, ExploreOptions,
    ExploreResult, classify_explore_node, explore_snapshot,
};
pub use oauth::{AuthStatus, BrowserOpener, OAuthManager, SecretString, SystemBrowser};
pub use payload::{
    CollectedPayload, PayloadCompleteness, PayloadStructure, validate_payload_context,
};
pub use resources::{
    ResourceKind, ResourceOccurrence, ResourceScope, UsedResourceRefs, collect_used_resource_refs,
};
pub use search::{SearchOptions, SearchResult, search_snapshot};
pub use snapshot::{
    Diagnostic, RawNode, Snapshot, SnapshotChunk, TypedNode, merge_chunks,
    snapshot_chunk_from_result,
};
pub use source::{
    SelectedSource, SourcePolicy, UpstreamFailureContext, UpstreamFailureKind,
    classify_upstream_failure, fallback_allowed, fallback_allowed_for_error,
    upstream_failure_error,
};
pub use upstream::{
    BuiltinScript, ExploreReadOptions, FigmaUpstream, ReadToolCall, RemoteFigmaClient,
    SearchReadOptions, UpstreamResult,
};
pub use url::FigmaTarget;
pub use variables::{ResourceBatch, ResourceStyleRef, UnresolvedResource};
mod metadata;
pub use collector::{
    CollectedParts, CollectionRequest, CollectionScope, CollectorSession, CollectorStep,
    PlannedCall,
};
