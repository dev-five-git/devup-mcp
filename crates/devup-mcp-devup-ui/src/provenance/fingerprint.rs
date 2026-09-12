use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::sync::LazyLock;

use devup_mcp_figma::{RawNode, Snapshot};
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};

// These are the exact manifests substituted into snapshot.js and
// fast_snapshot.js by devup-mcp-figma's ScriptKind::source_with_inputs.
static NODE_FIELDS: LazyLock<BTreeSet<String>> = LazyLock::new(|| {
    let mut fields: BTreeSet<String> = serde_json::from_str(include_str!(
        "../../../devup-mcp-figma/src/plugin_api_manifest.json"
    ))
    .expect("checked-in node property manifest");
    // Collector-derived inputs read by TypedNode/codegen, rather than direct
    // Figma properties. Do not include exploration or transport metadata.
    fields.extend(
        [
            "parentId",
            "parentType",
            "childrenIds",
            "styledTextSegments",
        ]
        .map(str::to_owned),
    );
    fields
});

static SEGMENT_FIELDS: LazyLock<BTreeSet<String>> = LazyLock::new(|| {
    let mut fields: BTreeSet<String> = serde_json::from_str(include_str!(
        "../../../devup-mcp-figma/src/text_segment_manifest.json"
    ))
    .expect("checked-in text segment manifest");
    // getStyledTextSegments also returns these intrinsic run fields.
    fields.extend(["characters", "start", "end"].map(str::to_owned));
    fields
});

/// Per-node source fingerprints for an explicitly requested source map.
///
/// Domain: node type, the collector's property manifest and derived tree/text
/// fields. Values use the converter's fields-then-extra lookup. Missing, null,
/// and failed reads are distinct; error prose and capture metadata are excluded.
/// Text runs use the text segment manifest plus characters/start/end. Node IDs
/// identify results, but are not themselves hashed. This is a capture comparison,
/// not a hash of generated code, external variable definitions, or asset bytes.
///
/// v1 uses SHA-256 over a length-prefixed, type-tagged binary encoding. Object
/// keys are sorted, arrays retain order, and numbers are normalized exact binary
/// rationals (including integer/float equivalence and signed-zero equivalence).
/// Bump the prefix if the encoding changes; no public source offsets are emitted.
pub fn design_fingerprints(snapshot: &Snapshot) -> BTreeMap<String, String> {
    snapshot
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), fingerprint(node)))
        .collect()
}

fn fingerprint(node: &RawNode) -> String {
    let mut hash = Sha256::new();
    hash.update(b"devup-design-fingerprint\0v1\0");
    string(&mut hash, &node.node_type);
    let view = node.typed_view();
    for field in NODE_FIELDS.iter() {
        string(&mut hash, field);
        hash.update([u8::from(node.field_errors.contains_key(field))]);
        match view.value(field) {
            None => hash.update([0]),
            Some(value) => {
                hash.update([1]);
                if field == "styledTextSegments" {
                    text_segments(&mut hash, value);
                } else {
                    canonical_value(&mut hash, value);
                }
            }
        }
    }
    let mut result = String::from("v1:sha256:");
    for byte in hash.finalize() {
        write!(result, "{byte:02x}").expect("writing to a String");
    }
    result
}

fn length(hash: &mut Sha256, value: usize) {
    hash.update((value as u64).to_be_bytes());
}

fn string(hash: &mut Sha256, value: &str) {
    hash.update(b"s");
    length(hash, value.len());
    hash.update(value.as_bytes());
}

fn text_segments(hash: &mut Sha256, value: &Value) {
    let Some(segments) = value.as_array() else {
        canonical_value(hash, value);
        return;
    };
    hash.update(b"a");
    length(hash, segments.len());
    for segment in segments {
        if let Some(object) = segment.as_object() {
            let selected = object
                .iter()
                .filter(|(key, _)| SEGMENT_FIELDS.contains(*key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            canonical_value(hash, &Value::Object(selected));
        } else {
            canonical_value(hash, segment);
        }
    }
}

fn canonical_value(hash: &mut Sha256, value: &Value) {
    match value {
        Value::Null => hash.update(b"n"),
        Value::Bool(false) => hash.update(b"f"),
        Value::Bool(true) => hash.update(b"t"),
        Value::String(value) => string(hash, value),
        Value::Number(value) => number(hash, value),
        Value::Array(values) => {
            hash.update(b"a");
            length(hash, values.len());
            for value in values {
                canonical_value(hash, value);
            }
        }
        Value::Object(values) => {
            hash.update(b"o");
            length(hash, values.len());
            for (key, value) in values.iter().collect::<BTreeMap<_, _>>() {
                string(hash, key);
                canonical_value(hash, value);
            }
        }
    }
}

fn number(hash: &mut Sha256, value: &Number) {
    // Represent every finite JSON number as sign * significand * 2^exponent,
    // without decimal formatting or rounding large integer JSON values to f64.
    let (mut negative, mut significand, mut exponent) = if let Some(value) = value.as_u64() {
        (false, value, 0_i32)
    } else if let Some(value) = value.as_i64() {
        (value < 0, value.unsigned_abs(), 0)
    } else {
        let bits = value.as_f64().expect("finite JSON number").to_bits();
        let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
        let fraction = bits & ((1_u64 << 52) - 1);
        if exponent_bits == 0 {
            (bits >> 63 != 0, fraction, -1074)
        } else {
            (
                bits >> 63 != 0,
                fraction | (1_u64 << 52),
                exponent_bits - 1075,
            )
        }
    };
    if significand == 0 {
        negative = false;
        exponent = 0;
    } else {
        let zeros = significand.trailing_zeros();
        significand >>= zeros;
        exponent += zeros as i32;
    }
    hash.update(b"d");
    hash.update([u8::from(negative)]);
    hash.update(significand.to_be_bytes());
    hash.update(exponent.to_be_bytes());
}
