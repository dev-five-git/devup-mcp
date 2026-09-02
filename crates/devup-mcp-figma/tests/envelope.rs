use base64::{Engine as _, engine::general_purpose::STANDARD};
use devup_mcp_figma::{
    FigmaTarget, UpstreamResult, decode_fast_multi_snapshot, decode_fast_snapshot,
    decode_fast_theme,
};
use serde_json::{Value, json};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[test]
fn valid_multichunk_envelope_round_trips() {
    let target = target();
    let envelope = complete_envelope();
    let result = upstream_result(envelope.clone(), 2);

    let decoded = decode_fast_snapshot(&result, &target).expect("valid envelope");

    assert_eq!(decoded.snapshot.file_key, "fileKey123");
    assert_eq!(decoded.snapshot.root_ids, ["1:1"]);
    assert_eq!(decoded.snapshot.nodes.len(), 2);
    assert_eq!(
        decoded.resources.raw["variables"].as_array().unwrap().len(),
        1
    );
    assert_eq!(decoded.resources.raw["styles"].as_array().unwrap().len(), 1);
    assert_eq!(decoded.stats.raw_bytes, envelope.len());
    assert!(decoded.stats.wire_bytes > envelope.len());
    assert_eq!(decoded.stats.chunk_count, 2);
}

#[test]
fn valid_multi_image_envelope_round_trips() {
    let target = target();
    let envelope = complete_envelope();
    let result = upstream_result_with_split_pngs(envelope.clone(), 2);

    let decoded = decode_fast_snapshot(&result, &target).expect("valid split envelope");

    assert_eq!(decoded.snapshot.nodes.len(), 2);
    assert_eq!(decoded.stats.raw_bytes, envelope.len());
    assert_eq!(decoded.stats.chunk_count, 2);
    assert!(decoded.stats.wire_bytes > envelope.len());
}

#[test]
fn valid_multi_root_envelope_requires_the_exact_ordered_root_set() {
    let envelope = mutate_envelope(|value| {
        value["source"]["rootId"] = json!("9:9");
        value["snapshot"]["rootIds"] = json!(["1:1", "1:2"]);
    });
    let mut result = upstream_result(envelope, 1);
    let mut descriptor: Value =
        serde_json::from_str(result.raw["content"][0]["text"].as_str().unwrap()).unwrap();
    descriptor["rootId"] = json!("9:9");
    result.raw["content"][0]["text"] = json!(descriptor.to_string());
    let section_target = FigmaTarget {
        node_id: Some("9:9".to_owned()),
        ..target()
    };

    let decoded = decode_fast_multi_snapshot(
        &result,
        &section_target,
        &["1:1".to_owned(), "1:2".to_owned()],
    )
    .expect("valid multi-root envelope");
    assert_eq!(decoded.snapshot.root_ids, ["1:1", "1:2"]);

    let error = decode_fast_multi_snapshot(
        &result,
        &section_target,
        &["1:2".to_owned(), "1:1".to_owned()],
    )
    .expect_err("ordered root mismatch");
    assert_eq!(error.details["category"], "targetMismatch");
}

#[test]
fn out_of_order_chunks_are_rejected() {
    let envelope = complete_envelope();
    let png = envelope_png_with_order(&envelope, &[1, 0]);
    let result = upstream_result_with_png(png, envelope.len(), 2);

    let error = decode_fast_snapshot(&result, &target()).expect_err("out of order chunks");

    assert_eq!(error.details["category"], "envelopeChunkSequence");
}

#[test]
fn noncanonical_png_header_is_rejected() {
    let envelope = complete_envelope();
    let png = envelope_png_with_ihdr(&envelope, &[0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    let result = upstream_result_with_png(png, envelope.len(), 1);

    let error = decode_fast_snapshot(&result, &target()).expect_err("noncanonical PNG");

    assert_eq!(error.details["category"], "pngIhdr");
}

#[test]
fn png_without_idat_is_rejected() {
    let envelope = complete_envelope();
    let mut png = PNG_SIGNATURE.to_vec();
    push_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    let mut data = Vec::with_capacity(envelope.len() + 8);
    data.extend_from_slice(&0_u32.to_be_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(&envelope);
    push_chunk(&mut png, b"duVp", &data);
    push_chunk(&mut png, b"IEND", &[]);

    assert_category(
        upstream_result_with_png(png, envelope.len(), 1),
        &target(),
        "pngIdat",
    );
}

#[test]
fn invalid_utf8_is_rejected_before_json_decode() {
    let bytes = vec![0xff, 0xfe, 0xfd];
    let result = upstream_result_with_png(envelope_png(&bytes, 1), bytes.len(), 1);

    let error = decode_fast_snapshot(&result, &target()).expect_err("invalid UTF-8");

    assert_eq!(error.details["category"], "envelopeUtf8");
}

#[test]
fn corrupt_transport_shapes_are_rejected_without_panicking() {
    let envelope = complete_envelope();

    let mut bad_signature = envelope_png(&envelope, 1);
    bad_signature[0] = 0;
    assert_category(
        upstream_result_with_png(bad_signature, envelope.len(), 1),
        &target(),
        "pngSignature",
    );

    let mut bad_crc = envelope_png(&envelope, 1);
    let marker = bad_crc
        .windows(4)
        .position(|window| window == b"duVp")
        .unwrap();
    bad_crc[marker + 12] ^= 1;
    assert_category(
        upstream_result_with_png(bad_crc, envelope.len(), 1),
        &target(),
        "pngCrc",
    );

    let mut truncated = envelope_png(&envelope, 1);
    truncated.pop();
    assert_category(
        upstream_result_with_png(truncated, envelope.len(), 1),
        &target(),
        "pngLength",
    );

    assert_category(
        upstream_result_with_png(
            envelope_png_with_order(&envelope, &[0, 0]),
            envelope.len(),
            2,
        ),
        &target(),
        "envelopeChunkSequence",
    );
}

#[test]
fn image_content_contract_is_strict() {
    let envelope = complete_envelope();

    let mut missing = upstream_result(envelope.clone(), 1);
    missing.raw["content"].as_array_mut().unwrap().truncate(1);
    assert_category(missing, &target(), "imageMissing");

    let mut wrong_mime = upstream_result(envelope.clone(), 1);
    wrong_mime.raw["content"][1]["mimeType"] = Value::from("image/jpeg");
    assert_category(wrong_mime, &target(), "imageMime");

    let mut duplicate = upstream_result_with_split_pngs(envelope.clone(), 2);
    let repeated = duplicate.raw["content"][1].clone();
    duplicate.raw["content"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    assert_category(duplicate, &target(), "imageMultiplicity");

    let oversized = vec![0_u8; 11 * 1024 * 1024 + 1];
    let error = decode_fast_snapshot(
        &upstream_result_with_png(oversized, envelope.len(), 1),
        &target(),
    )
    .expect_err("oversized PNG");
    assert_eq!(error.details["category"], "png");
}

#[test]
fn schema_target_graph_and_resource_integrity_are_validated() {
    let unsupported = mutate_envelope(|value| value["schemaVersion"] = Value::from(2));
    assert_category(upstream_result(unsupported, 1), &target(), "schemaVersion");

    let wrong_target = FigmaTarget {
        file_key: "otherFileKey".to_owned(),
        ..target()
    };
    assert_category(
        upstream_result(complete_envelope(), 1),
        &wrong_target,
        "targetMismatch",
    );

    let duplicate_node = mutate_envelope(|value| {
        let duplicate = value["snapshot"]["nodes"][1].clone();
        value["snapshot"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
    });
    assert_category(
        upstream_result(duplicate_node, 1),
        &target(),
        "duplicateNode",
    );

    let dangling_child = mutate_envelope(|value| {
        value["snapshot"]["nodes"][0]["fields"]["childrenIds"][0] = Value::from("9:9");
    });
    assert_category(
        upstream_result(dangling_child, 1),
        &target(),
        "danglingChild",
    );

    let missing_resource = mutate_envelope(|value| {
        value["resources"]["variables"] = json!([]);
    });
    assert_category(
        upstream_result(missing_resource, 1),
        &target(),
        "resourceMissing",
    );
}

#[test]
fn descriptor_must_match_the_binary_envelope() {
    let mut result = upstream_result(complete_envelope(), 2);
    let descriptor_text = result.raw["content"][0]["text"].as_str().unwrap();
    let mut descriptor: Value = serde_json::from_str(descriptor_text).unwrap();
    descriptor["nodeCount"] = Value::from(99);
    result.raw["content"][0]["text"] = Value::from(descriptor.to_string());

    assert_category(result, &target(), "nodeCount");
}

#[test]
fn valid_fast_theme_envelope_round_trips_and_validates_counts() {
    let envelope = theme_envelope();
    let result = theme_upstream_result(envelope.clone(), 1);

    let decoded = decode_fast_theme(&result, "fileKey123").expect("valid fast theme");

    assert_eq!(decoded.source_version, Some("v42".to_owned()));
    assert_eq!(
        decoded.resources.raw["collections"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        decoded.resources.raw["variables"].as_array().unwrap().len(),
        1
    );
    assert_eq!(decoded.resources.raw["styles"].as_array().unwrap().len(), 1);
    assert_eq!(decoded.resources.raw["localComplete"], true);
    assert_eq!(decoded.stats.raw_bytes, envelope.len());

    let mut bad = theme_upstream_result(envelope, 1);
    let descriptor = bad.raw["content"][0]["text"].as_str().unwrap();
    let mut descriptor: Value = serde_json::from_str(descriptor).unwrap();
    descriptor["variableCount"] = json!(2);
    bad.raw["content"][0]["text"] = json!(descriptor.to_string());
    let error = decode_fast_theme(&bad, "fileKey123").expect_err("count mismatch");
    assert_eq!(error.details["category"], "variableCount");
}

fn target() -> FigmaTarget {
    FigmaTarget {
        file_key: "fileKey123".to_owned(),
        node_id: Some("1:1".to_owned()),
        branch_key: None,
    }
}

fn complete_envelope() -> Vec<u8> {
    finalize_envelope(json!({
        "schemaVersion": 1,
        "source": {
            "fileKey": "fileKey123",
            "rootId": "1:1"
        },
        "snapshot": {
            "fileKey": "fileKey123",
            "version": null,
            "rootIds": ["1:1"],
            "nodes": [
                {
                    "id": "1:1",
                    "type": "FRAME",
                    "fields": {
                        "childrenIds": ["1:2"],
                        "boundVariables": {
                            "fills": {"type": "VARIABLE_ALIAS", "id": "VariableID:1:1"}
                        }
                    }
                },
                {
                    "id": "1:2",
                    "type": "TEXT",
                    "fields": {
                        "textStyleId": "S:style1",
                        "characters": "테스트"
                    }
                }
            ],
            "diagnostics": []
        },
        "resources": {
            "collections": [],
            "variables": [{"id": "VariableID:1:1", "name": "color/text"}],
            "styles": [{"id": "S:style1", "name": "body", "styleType": "TEXT"}],
            "usedRemoteVariables": [],
            "localComplete": false,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "integrity": {
            "nodeCount": 2,
            "variableRefCount": 1,
            "styleRefCount": 1,
            "utf8Bytes": 0
        }
    }))
}

fn theme_envelope() -> Vec<u8> {
    finalize_envelope(json!({
        "schemaVersion": 1,
        "source": {"fileKey": "fileKey123", "version": "v42"},
        "resources": {
            "collections": [{"id": "c", "name": "Theme"}],
            "variables": [{"id": "v", "name": "primary"}],
            "styles": [{"id": "s", "name": "body", "styleType": "TEXT"}],
            "usedRemoteVariables": [],
            "usedVariableIds": ["v"],
            "usedStyleIds": ["s"],
            "localComplete": true,
            "usedRemoteComplete": true,
            "unresolved": []
        },
        "integrity": {
            "collectionCount": 1,
            "variableCount": 1,
            "styleCount": 1,
            "unresolvedCount": 0,
            "utf8Bytes": 0
        }
    }))
}

fn theme_upstream_result(envelope: Vec<u8>, chunk_count: usize) -> UpstreamResult {
    let png = envelope_png(&envelope, chunk_count);
    let descriptor = json!({
        "kind": "devupFastThemeDescriptor",
        "schemaVersion": 1,
        "collectionCount": 1,
        "variableCount": 1,
        "styleCount": 1,
        "unresolvedCount": 0,
        "utf8Bytes": envelope.len(),
        "chunkCount": chunk_count
    });
    UpstreamResult {
        raw: json!({
            "content": [
                {"type": "text", "text": descriptor.to_string()},
                {"type": "image", "data": STANDARD.encode(png), "mimeType": "image/png"}
            ]
        }),
    }
}

fn mutate_envelope(mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(&complete_envelope()).unwrap();
    mutate(&mut value);
    finalize_envelope(value)
}

fn finalize_envelope(mut value: Value) -> Vec<u8> {
    for _ in 0..8 {
        let bytes = serde_json::to_vec(&value).unwrap();
        let length = bytes.len() as u64;
        if value["integrity"]["utf8Bytes"] == length {
            return bytes;
        }
        value["integrity"]["utf8Bytes"] = Value::from(length);
    }
    panic!("utf8Bytes did not converge");
}

fn assert_category(result: UpstreamResult, target: &FigmaTarget, expected: &str) {
    let error = decode_fast_snapshot(&result, target).expect_err(expected);
    assert_eq!(error.details["category"], expected);
}

fn upstream_result(envelope: Vec<u8>, chunk_count: usize) -> UpstreamResult {
    let png = envelope_png(&envelope, chunk_count);
    upstream_result_with_png(png, envelope.len(), chunk_count)
}

fn upstream_result_with_png(
    png: Vec<u8>,
    envelope_length: usize,
    chunk_count: usize,
) -> UpstreamResult {
    let descriptor = json!({
        "kind": "devupFastSnapshotDescriptor",
        "schemaVersion": 1,
        "rootId": "1:1",
        "nodeCount": 2,
        "variableRefCount": 1,
        "styleRefCount": 1,
        "utf8Bytes": envelope_length,
        "chunkCount": chunk_count
    });
    UpstreamResult {
        raw: json!({
            "content": [
                {"type": "text", "text": descriptor.to_string()},
                {"type": "image", "data": STANDARD.encode(png), "mimeType": "image/png"}
            ]
        }),
    }
}

fn upstream_result_with_split_pngs(envelope: Vec<u8>, chunk_count: usize) -> UpstreamResult {
    assert!(chunk_count > 0 && chunk_count <= envelope.len());
    let per_chunk = envelope.len().div_ceil(chunk_count);
    let payloads = envelope.chunks(per_chunk).collect::<Vec<_>>();
    assert_eq!(payloads.len(), chunk_count);
    let mut content = vec![json!({
        "type": "text",
        "text": json!({
            "kind": "devupFastSnapshotDescriptor",
            "schemaVersion": 1,
            "rootId": "1:1",
            "nodeCount": 2,
            "variableRefCount": 1,
            "styleRefCount": 1,
            "utf8Bytes": envelope.len(),
            "chunkCount": chunk_count
        }).to_string()
    })];
    for (sequence, payload) in payloads.into_iter().enumerate() {
        let png = envelope_png_for_chunk(payload, sequence, chunk_count);
        content.push(json!({
            "type": "image",
            "data": STANDARD.encode(png),
            "mimeType": "image/png"
        }));
    }
    UpstreamResult {
        raw: json!({"content": content}),
    }
}

fn envelope_png(envelope: &[u8], chunk_count: usize) -> Vec<u8> {
    assert!(chunk_count > 0 && chunk_count <= envelope.len());
    let order = (0..chunk_count).collect::<Vec<_>>();
    envelope_png_with_order(envelope, &order)
}

fn envelope_png_with_ihdr(envelope: &[u8], ihdr: &[u8; 13]) -> Vec<u8> {
    let mut png = PNG_SIGNATURE.to_vec();
    push_chunk(&mut png, b"IHDR", ihdr);
    let mut data = Vec::with_capacity(envelope.len() + 8);
    data.extend_from_slice(&0_u32.to_be_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(envelope);
    push_chunk(&mut png, b"duVp", &data);
    push_chunk(
        &mut png,
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x05, 0x00, 0xfa, 0xff, 0, 0, 0, 0, 0, 5, 0, 1,
        ],
    );
    push_chunk(&mut png, b"IEND", &[]);
    png
}

fn envelope_png_for_chunk(payload: &[u8], sequence: usize, total: usize) -> Vec<u8> {
    let mut png = PNG_SIGNATURE.to_vec();
    push_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    let mut data = Vec::with_capacity(payload.len() + 8);
    data.extend_from_slice(&(sequence as u32).to_be_bytes());
    data.extend_from_slice(&(total as u32).to_be_bytes());
    data.extend_from_slice(payload);
    push_chunk(&mut png, b"duVp", &data);
    push_chunk(
        &mut png,
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x05, 0x00, 0xfa, 0xff, 0, 0, 0, 0, 0, 5, 0, 1,
        ],
    );
    push_chunk(&mut png, b"IEND", &[]);
    png
}

fn envelope_png_with_order(envelope: &[u8], order: &[usize]) -> Vec<u8> {
    let chunk_count = order.len();
    assert!(chunk_count > 0 && chunk_count <= envelope.len());
    let mut png = PNG_SIGNATURE.to_vec();
    push_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);

    let per_chunk = envelope.len().div_ceil(chunk_count);
    let payloads = envelope.chunks(per_chunk).collect::<Vec<_>>();
    assert_eq!(payloads.len(), chunk_count);
    for &sequence in order {
        let payload = payloads[sequence];
        let mut data = Vec::with_capacity(payload.len() + 8);
        data.extend_from_slice(&(sequence as u32).to_be_bytes());
        data.extend_from_slice(&(chunk_count as u32).to_be_bytes());
        data.extend_from_slice(payload);
        push_chunk(&mut png, b"duVp", &data);
    }

    push_chunk(
        &mut png,
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x05, 0x00, 0xfa, 0xff, 0, 0, 0, 0, 0, 5, 0, 1,
        ],
    );
    push_chunk(&mut png, b"IEND", &[]);
    png
}

fn push_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
