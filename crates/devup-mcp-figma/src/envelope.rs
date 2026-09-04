use std::{borrow::Cow, collections::BTreeSet};

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    DevupError, ErrorCode, FigmaTarget, ResourceKind, SnapshotChunk, UpstreamResult,
    collect_used_resource_refs, read_snapshot_cursor,
};

/// Decoder-side ceiling on a single text envelope. Deliberately larger than
/// the 15 KiB the producing script budgets itself to: a relay that
/// re-serializes the JSON (pretty-printing, different escaping) inflates the
/// payload without changing its content, and rejecting that as `too_large`
/// would fail a perfectly valid envelope. Still bounded, so a hostile or
/// runaway response cannot be buffered without limit.
const MAX_TEXT_ENVELOPE_BYTES: usize = 64 * 1024;
const MAX_STRINGIFIED_RESULT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FastTransportStats {
    pub transport: &'static str,
    pub raw_bytes: usize,
    pub wire_bytes: usize,
    pub chunk_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FastSnapshotPayload {
    pub snapshot: SnapshotChunk,
    pub resources: UpstreamResult,
    pub stats: FastTransportStats,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FastThemePayload {
    pub resources: UpstreamResult,
    pub source_version: Option<String>,
    pub stats: FastTransportStats,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    #[serde(default)]
    kind: Option<String>,
    schema_version: u32,
    source: EnvelopeSource,
    snapshot: SnapshotChunk,
    resources: Value,
    integrity: EnvelopeIntegrity,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvelopeSource {
    file_key: String,
    root_id: String,
}

/// The producer also emits `utf8Bytes` here. It is deliberately absent: it is
/// the producer's measurement of its own serialized form, so comparing it
/// against what arrived only rejected relays that re-serialize the JSON.
/// Corruption is caught by the counts below plus `validate_resources`, which
/// read the content itself. Serde ignores the extra key on the wire.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvelopeIntegrity {
    node_count: usize,
    variable_ref_count: usize,
    style_ref_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeEnvelope {
    #[serde(default)]
    kind: Option<String>,
    schema_version: u32,
    source: ThemeEnvelopeSource,
    resources: Value,
    integrity: ThemeEnvelopeIntegrity,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeEnvelopeSource {
    file_key: String,
    version: Option<String>,
}

/// `utf8Bytes` is omitted for the same reason as [`EnvelopeIntegrity`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeEnvelopeIntegrity {
    collection_count: usize,
    variable_count: usize,
    style_count: usize,
    unresolved_count: usize,
}

pub fn decode_fast_snapshot(
    result: &UpstreamResult,
    target: &FigmaTarget,
) -> Result<FastSnapshotPayload, DevupError> {
    let target_root = target
        .node_id
        .as_ref()
        .ok_or_else(|| invalid("targetRootMissing"))?;
    decode_fast_snapshot_for_roots(result, target, std::slice::from_ref(target_root))
}

pub fn decode_fast_multi_snapshot(
    result: &UpstreamResult,
    target: &FigmaTarget,
    expected_root_ids: &[String],
) -> Result<FastSnapshotPayload, DevupError> {
    if expected_root_ids.is_empty()
        || expected_root_ids.iter().collect::<BTreeSet<_>>().len() != expected_root_ids.len()
    {
        return Err(invalid("targetRootsInvalid"));
    }
    decode_fast_snapshot_for_roots(result, target, expected_root_ids)
}

/// Fast node snapshots are always delivered as text now (no PNG-chunked
/// binary transport exists any more — real-world hosts silently discarded
/// those image attachments, so it never actually worked). A single round may
/// legitimately cover only *part* of the target subtree; `peek_page_cursor`
/// reports whether this is the case so `validate_envelope` can relax the
/// root-containment and dangling-child checks that only hold for a complete,
/// self-contained envelope.
fn decode_fast_snapshot_for_roots(
    result: &UpstreamResult,
    target: &FigmaTarget,
    expected_root_ids: &[String],
) -> Result<FastSnapshotPayload, DevupError> {
    let raw = normalize_upstream_result(&result.raw)?;
    let Some((envelope, utf8_bytes)) =
        find_tagged_text::<Envelope>(&raw, "devupFastSnapshotEnvelope")?
    else {
        return Err(invalid("textEnvelopeMissing"));
    };
    let page = peek_page_cursor(&envelope.snapshot)?;
    validate_envelope(&envelope, target, expected_root_ids, page)?;
    Ok(FastSnapshotPayload {
        snapshot: envelope.snapshot,
        resources: UpstreamResult {
            raw: envelope.resources,
        },
        stats: FastTransportStats {
            transport: "text",
            raw_bytes: utf8_bytes,
            wire_bytes: utf8_bytes,
            chunk_count: 0,
        },
    })
}

pub fn decode_fast_theme(
    result: &UpstreamResult,
    expected_file_key: &str,
) -> Result<FastThemePayload, DevupError> {
    let raw = normalize_upstream_result(&result.raw)?;
    let Some((envelope, utf8_bytes)) =
        find_tagged_text::<ThemeEnvelope>(&raw, "devupFastThemeEnvelope")?
    else {
        return Err(invalid("textEnvelopeMissing"));
    };
    validate_theme_envelope(&envelope, expected_file_key)?;
    Ok(FastThemePayload {
        resources: UpstreamResult {
            raw: envelope.resources,
        },
        source_version: envelope.source.version,
        stats: FastTransportStats {
            transport: "text",
            raw_bytes: utf8_bytes,
            wire_bytes: utf8_bytes,
            chunk_count: 0,
        },
    })
}

/// Whether an envelope's node list is a partial page of a larger, paginated
/// fetch. Derived from the shared `__DEVUP_SNAPSHOT_CURSOR__` reader so the
/// marker is only ever parsed against one field list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PageCursor {
    is_first_page: bool,
    is_final_page: bool,
}

fn peek_page_cursor(chunk: &SnapshotChunk) -> Result<PageCursor, DevupError> {
    match read_snapshot_cursor(&chunk.nodes).map_err(|error| invalid(error.category()))? {
        Some(cursor) => Ok(PageCursor {
            is_first_page: cursor.offset == 0,
            is_final_page: cursor.complete,
        }),
        // No cursor marker at all: treat as a single, complete, self-contained
        // envelope (the shape every fast snapshot had before pagination).
        // Real script output always includes the marker; this only matters for
        // hand-built payloads (tests, older fixtures).
        None => Ok(PageCursor {
            is_first_page: true,
            is_final_page: true,
        }),
    }
}

fn normalize_upstream_result(value: &Value) -> Result<Cow<'_, Value>, DevupError> {
    match value {
        Value::String(text) => {
            if text.len() > MAX_STRINGIFIED_RESULT_BYTES {
                return Err(too_large("upstreamResultJson"));
            }
            serde_json::from_str(text)
                .map(Cow::Owned)
                .map_err(|_| invalid("upstreamResultJson"))
        }
        _ => Ok(Cow::Borrowed(value)),
    }
}

fn find_tagged_text<T: DeserializeOwned>(
    value: &Value,
    expected_kind: &str,
) -> Result<Option<(T, usize)>, DevupError> {
    fn collect<'a>(value: &'a Value, expected_kind: &str, found: &mut Vec<&'a str>) {
        match value {
            Value::Object(object) => {
                if let Some(text) = object.get("text").and_then(Value::as_str)
                    && serde_json::from_str::<Value>(text)
                        .ok()
                        .and_then(|value| {
                            value.get("kind").and_then(Value::as_str).map(str::to_owned)
                        })
                        .as_deref()
                        == Some(expected_kind)
                {
                    found.push(text);
                }
                for child in object.values() {
                    collect(child, expected_kind, found);
                }
            }
            Value::Array(values) => {
                for child in values {
                    collect(child, expected_kind, found);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    let mut found = Vec::new();
    collect(value, expected_kind, &mut found);
    match found.as_slice() {
        [] => Ok(None),
        [text] => {
            if text.len() > MAX_TEXT_ENVELOPE_BYTES {
                return Err(too_large("textEnvelope"));
            }
            serde_json::from_str(text)
                .map(|envelope| Some((envelope, text.len())))
                .map_err(|_| invalid("envelopeJson"))
        }
        _ => Err(invalid("textEnvelopeMultiplicity")),
    }
}

fn validate_envelope(
    envelope: &Envelope,
    target: &FigmaTarget,
    expected_root_ids: &[String],
    page: PageCursor,
) -> Result<(), DevupError> {
    if envelope.schema_version != 1
        || envelope
            .kind
            .as_deref()
            .is_some_and(|kind| kind != "devupFastSnapshotEnvelope")
    {
        return Err(invalid("schemaVersion"));
    }
    let target_root = target
        .node_id
        .as_deref()
        .ok_or_else(|| invalid("targetRootMissing"))?;
    if envelope.source.file_key != target.file_key
        || envelope.snapshot.file_key != target.file_key
        || envelope.source.root_id != target_root
        || envelope.snapshot.root_ids != expected_root_ids
    {
        return Err(invalid("targetMismatch"));
    }
    let mut node_ids = BTreeSet::new();
    for node in &envelope.snapshot.nodes {
        if !node_ids.insert(node.id.as_str()) {
            return Err(invalid("duplicateNode"));
        }
    }
    // The root is only guaranteed present on the first page of a paginated
    // fetch (BFS traversal always visits it at index 0); later pages cover
    // only a later slice of the same subtree.
    if envelope.integrity.node_count != node_ids.len()
        || (page.is_first_page
            && !expected_root_ids
                .iter()
                .all(|root_id| node_ids.contains(root_id.as_str())))
    {
        return Err(invalid("nodeCount"));
    }
    // A child referenced by a node in this page may legitimately live in a
    // later page while pagination is still in progress. Once the fetch is
    // complete (this is the final page), every remaining node has already
    // been sent, so full containment is enforced again.
    if page.is_final_page {
        for node in &envelope.snapshot.nodes {
            for child_id in node.typed_view().child_ids() {
                if !node_ids.contains(child_id) {
                    return Err(invalid("danglingChild"));
                }
            }
        }
    }

    let refs = collect_used_resource_refs(std::slice::from_ref(&envelope.snapshot));
    if envelope.integrity.variable_ref_count != refs.variable_ids.len()
        || envelope.integrity.style_ref_count != refs.styles.len()
    {
        return Err(invalid("resourceRefCount"));
    }
    validate_resources(&envelope.resources, &refs.variable_ids, &refs.styles)?;
    Ok(())
}

fn validate_theme_envelope(
    envelope: &ThemeEnvelope,
    expected_file_key: &str,
) -> Result<(), DevupError> {
    if envelope.schema_version != 1
        || envelope
            .kind
            .as_deref()
            .is_some_and(|kind| kind != "devupFastThemeEnvelope")
    {
        return Err(invalid("schemaVersion"));
    }
    if envelope.source.file_key != expected_file_key {
        return Err(invalid("targetMismatch"));
    }
    let resources = envelope
        .resources
        .as_object()
        .ok_or_else(|| invalid("resourcesShape"))?;
    let collections = resource_ids(resources.get("collections"), "collections")?;
    let variables = resource_ids(resources.get("variables"), "variables")?;
    let styles = resource_ids(resources.get("styles"), "styles")?;
    let unresolved = resources
        .get("unresolved")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("unresolvedShape"))?;
    validate_theme_count(
        envelope.integrity.collection_count,
        collections.len(),
        "collectionCount",
    )?;
    validate_theme_count(
        envelope.integrity.variable_count,
        variables.len(),
        "variableCount",
    )?;
    validate_theme_count(envelope.integrity.style_count, styles.len(), "styleCount")?;
    validate_theme_count(
        envelope.integrity.unresolved_count,
        unresolved.len(),
        "unresolvedCount",
    )?;
    if resources.get("localComplete").and_then(Value::as_bool) != Some(true) {
        return Err(invalid("localComplete"));
    }
    for value in unresolved {
        if value.get("id").and_then(Value::as_str).is_none()
            || serde_json::from_value::<ResourceKind>(
                value
                    .get("kind")
                    .cloned()
                    .ok_or_else(|| invalid("unresolvedShape"))?,
            )
            .is_err()
        {
            return Err(invalid("unresolvedShape"));
        }
    }
    Ok(())
}

fn validate_theme_count(
    envelope_count: usize,
    observed_count: usize,
    category: &'static str,
) -> Result<(), DevupError> {
    if envelope_count != observed_count {
        Err(invalid(category))
    } else {
        Ok(())
    }
}

fn validate_resources(
    resources: &Value,
    variable_ids: &[String],
    style_refs: &[crate::ResourceStyleRef],
) -> Result<(), DevupError> {
    let object = resources
        .as_object()
        .ok_or_else(|| invalid("resourcesShape"))?;
    let resolved_variables = resource_ids(object.get("variables"), "variables")?;
    let resolved_styles = resource_ids(object.get("styles"), "styles")?;
    let mut unresolved_variables = BTreeSet::new();
    let mut unresolved_styles = BTreeSet::new();
    for value in object
        .get("unresolved")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("unresolvedShape"))?
    {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("unresolvedShape"))?;
        match serde_json::from_value::<ResourceKind>(
            value
                .get("kind")
                .cloned()
                .ok_or_else(|| invalid("unresolvedShape"))?,
        )
        .map_err(|_| invalid("unresolvedShape"))?
        {
            ResourceKind::Variable => {
                unresolved_variables.insert(id);
            }
            ResourceKind::Style => {
                unresolved_styles.insert(id);
            }
        }
    }
    if variable_ids.iter().any(|id| {
        !resolved_variables.contains(id.as_str()) && !unresolved_variables.contains(id.as_str())
    }) || style_refs.iter().any(|style| {
        !resolved_styles.contains(style.id.as_str())
            && !unresolved_styles.contains(style.id.as_str())
    }) {
        return Err(invalid("resourceMissing"));
    }
    Ok(())
}

fn resource_ids<'a>(
    value: Option<&'a Value>,
    category: &'static str,
) -> Result<BTreeSet<&'a str>, DevupError> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(category))?;
    values
        .iter()
        .map(|value| {
            value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(category))
        })
        .collect()
}

fn invalid(category: &'static str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupSnapshotUnsupported,
        "Figma fast snapshot envelope validation failed.",
        false,
        json!({"category": category}),
    )
}

fn too_large(category: &'static str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupFigmaResponseTooLarge,
        "Figma fast snapshot envelope exceeded the safe size limit.",
        false,
        json!({"category": category}),
    )
}
