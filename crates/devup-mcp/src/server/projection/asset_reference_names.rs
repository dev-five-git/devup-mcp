/// Every asset the generated code points at, renamed the way its bytes will
/// be written. The code holds each path inside a string or a `url(...)`, so a
/// name runs to the first quote or closing parenthesis - spaces and dots
/// belong to it, as they do in `Frame 269.png`.
fn with_host_safe_asset_paths(code: &str) -> String {
    let mut safe = String::with_capacity(code.len());
    let mut rest = code;
    loop {
        let Some((at, prefix)) = ["/icons/", "/images/"]
            .into_iter()
            .filter_map(|prefix| rest.find(prefix).map(|at| (at, prefix)))
            .min_by_key(|(at, _)| *at)
        else {
            safe.push_str(rest);
            return safe;
        };
        let after = at + prefix.len();
        safe.push_str(&rest[..after]);
        let opener = rest[..at].chars().last().or_else(|| safe.chars().last());
        let terminators: &[char] = match opener {
            Some('"') => &['"'],
            Some('\'') => &['\''],
            _ => &['"', '\'', ')'],
        };
        let mut end = rest[after..]
            .find(terminators)
            .map_or(rest.len(), |offset| after + offset);
        // srcSet contains several URLs followed by numeric density/width
        // descriptors. Keep each URL separate while retaining spaces inside
        // ordinary quoted layer filenames.
        let candidate = &rest[after..end];
        let first = candidate
            .split_once(',')
            .map_or(candidate, |(first, _)| first)
            .trim_end();
        if let Some((filename, descriptor)) = first.rsplit_once(char::is_whitespace)
            && descriptor
                .strip_suffix('x')
                .or_else(|| descriptor.strip_suffix('w'))
                .and_then(|n| n.parse::<f64>().ok())
                .is_some_and(|n| n.is_finite() && n > 0.0)
        {
            end = after + filename.trim_end().len();
        }
        safe.push_str(&host_safe_file_name(&rest[after..end]));
        rest = &rest[end..];
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
