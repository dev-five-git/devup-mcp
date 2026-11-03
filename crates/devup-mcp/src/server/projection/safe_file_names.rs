/// Bound semantic filenames without putting source IDs or hashes into public paths.
/// Unicode names remain readable on the supported filesystems. Generated folders are short; output roots remain
/// subject to OutputPolicy and the host filesystem's own path limits.
fn host_safe_file_name(name: &str) -> String {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut slug = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '@') {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug = slug.trim_matches('-').to_owned();
    let upper = slug.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    if slug.is_empty() || reserved {
        slug.insert_str(0, "asset-");
    }
    slug = slug
        .split('-')
        .filter(|part| !(part.len() >= 24 && part.chars().all(|c| c.is_ascii_hexdigit())))
        .collect::<Vec<_>>()
        .join("-");
    while slug.len() > 120 {
        slug.pop();
    }
    if slug.is_empty() {
        slug = "image".into();
    }
    let extension = extension
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(10)
        .collect::<String>();
    if !extension.is_empty() {
        slug.push('.');
        slug.push_str(&extension);
    }
    slug
}

/// Preserve semantic subdirectories and Unicode while sanitizing unsafe path characters.
fn host_safe_asset_path(path: &str) -> String {
    for prefix in ["/icons/", "/images/"] {
        if let Some(name) = path.strip_prefix(prefix) {
            return format!(
                "{prefix}{}",
                name.split('/')
                    .map(host_safe_file_name)
                    .collect::<Vec<_>>()
                    .join("/")
            );
        }
    }
    match path.rsplit_once('/') {
        Some((directory, name)) => format!("{directory}/{}", host_safe_file_name(name)),
        None => host_safe_file_name(path),
    }
}
