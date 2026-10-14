use std::time::Duration;

/// Total lookup/render budget per asset, shared by the server, script and bridge.
/// Invalid settings retain the existing 90 second default.
pub fn asset_timeout() -> Duration {
    Duration::from_millis(parse_asset_timeout(
        std::env::var("DEVUP_ASSET_TIMEOUT_MS").ok().as_deref(),
    ))
}

fn parse_asset_timeout(value: Option<&str>) -> u64 {
    value
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|v| (1_000..=600_000).contains(v))
        .unwrap_or(90_000)
}

/// A transport must leave time for the script to return its diagnostic.
pub(crate) fn asset_transport_timeout() -> Duration {
    asset_timeout() + Duration::from_secs(5)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_is_bounded_and_backwards_compatible() {
        for value in [
            None,
            Some("0"),
            Some("999"),
            Some("600001"),
            Some("invalid"),
        ] {
            assert_eq!(parse_asset_timeout(value), 90_000);
        }
        assert_eq!(parse_asset_timeout(Some("180000")), 180_000);
        assert_eq!(parse_asset_timeout(Some("1000")), 1_000);
        assert_eq!(parse_asset_timeout(Some("600000")), 600_000);
    }
}
