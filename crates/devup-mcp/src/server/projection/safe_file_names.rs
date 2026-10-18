/// Bound generated filenames, retaining a hash when transliteration would lose
/// identity. The generated folders are short; user-chosen output roots remain
/// subject to OutputPolicy and the host filesystem's own path limits.
fn host_safe_file_name(name: &str) -> String {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut slug = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '@') {
                c
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
    let needs_hash =
        !name.is_ascii() || slug != stem || slug.len() > 120 || slug.is_empty() || reserved;
    if slug.is_empty() || reserved {
        slug.insert_str(0, "asset-");
    }
    if needs_hash {
        slug.truncate(100);
    }
    if needs_hash {
        slug.push('-');
        slug.push_str(&sha256_hex(name.as_bytes())[..12]);
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

/// `/icons/ic:round-arrow-left.svg` as `/icons/ic-round-arrow-left-b041d24e668d.svg`. Only
/// the file name is touched; the folder the code looks in is left alone.
fn host_safe_asset_path(path: &str) -> String {
    for prefix in ["/icons/", "/images/"] {
        if let Some(name) = path.strip_prefix(prefix) {
            return format!("{prefix}{}", host_safe_file_name(name));
        }
    }
    match path.rsplit_once('/') {
        Some((directory, name)) => format!("{directory}/{}", host_safe_file_name(name)),
        None => host_safe_file_name(path),
    }
}
