fn projection_key(outputs: &[ProjectedOutput]) -> String {
    let mut hasher = Sha256::new();
    for output in outputs {
        hasher.update(output.name.as_bytes());
        hasher.update([0]);
        hasher.update(output.mime_type.as_bytes());
        hasher.update([u8::from(output.is_binary)]);
        hasher.update((output.bytes.len() as u64).to_le_bytes());
        hasher.update(&output.bytes);
    }
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

pub(super) fn artifact_metadata(artifact: &ArtifactLookup) -> Value {
    let origin_collection = &artifact.payload.stats;
    json!({
        "artifactId": artifact.artifact_id,
        "contentHash": artifact.content_hash,
        "cacheHit": artifact.cache_hit,
        "reuseKind": artifact.reuse_kind,
        "ageSeconds": artifact.age_seconds,
        "remainingTtlSeconds": artifact.remaining_ttl_seconds,
        "avoidedFigmaToolCalls": if artifact.cache_hit { origin_collection.figma_tool_calls } else { 0 },
        "originCollection": origin_collection,
        "capabilities": artifact.capabilities,
        "reuse": reuse_summary(&artifact.capabilities),
        "sizeBytes": artifact.size_bytes,
        "acquiredAt": format_epoch_rfc3339(artifact.created_at_epoch_seconds),
        "expiresAt": format_epoch_rfc3339(artifact.expires_at_epoch_seconds)
    })
}
