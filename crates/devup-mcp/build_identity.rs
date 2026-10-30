pub fn git_identity(commit: Option<&str>, dirty: bool) -> Option<String> {
    dirty_identity(commit, dirty.then_some(""))
}

pub fn dirty_identity(commit: Option<&str>, diff: Option<&str>) -> Option<String> {
    let commit = commit.filter(|value| safe(value))?;
    Some(match diff {
        None => commit.to_owned(),
        Some(hash) if safe(hash) => format!("{commit}-dirty.{hash}"),
        Some(_) => format!("{commit}-dirty"),
    })
}

pub fn safe(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
