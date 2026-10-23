#[path = "asset_policy.rs"]
mod asset_policy;
#[path = "exclude_hidden_asset_outputs.rs"]
mod exclude_hidden_asset_outputs;
#[path = "finalize_export_delivery.rs"]
mod finalize_export_delivery;
#[path = "image_fill_audit.rs"]
mod image_fill_audit;
#[path = "../page_scaffold.rs"]
pub(crate) mod page_scaffold;
#[path = "prepare_asset_manifest.rs"]
mod prepare_asset_manifest;
#[path = "project_node_and_theme_outputs.rs"]
mod project_node_and_theme_outputs;
#[path = "project_responsive_output.rs"]
mod project_responsive_output;
#[path = "project_section_frames.rs"]
mod project_section_frames;
#[path = "responsive_groups.rs"]
pub(super) mod responsive_groups;
#[path = "section_selection_reply.rs"]
mod section_selection_reply;
#[path = "summarize_projection_diagnostics.rs"]
mod summarize_projection_diagnostics;
#[path = "theme_export.rs"]
pub(crate) mod theme_export;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::result_contract::{
    attach_review_and_deliverable, failure_issue, output_result, scope_output,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_devup_ui::{
    codegen::{
        CodegenOptions, generate_component, normalize_component_name, responsive::merge_breakpoints,
    },
    theme::{generate_devup_json, variable_snapshot_from_result},
};
use devup_mcp_figma::{
    AssetManifest, AssetStatus, CollectedPayload, CollectionStats, DevupError, Diagnostic,
    DiagnosticSeverity, ErrorCode, ExploreOptions, SearchOptions, TargetKind, classify_target,
    explore_snapshot, search_snapshot,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{
    artifacts::{ArtifactLookup, ArtifactStore, OutputReservation},
    delivery::{DeliveryMode, ProjectedOutput, choose_delivery_for_result},
    format_epoch_rfc3339,
    operation::PendingOperation,
    output::{OutputPolicy, OutputTransaction},
    parse_scope,
    quality::{
        AcquisitionQuality, OutputQuality, ProjectionQuality, acquisition_quality, assets_quality,
        projection_quality, theme_quality,
    },
    section_candidate_as_explore, section_index_from_payload,
};
