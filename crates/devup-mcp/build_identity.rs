pub fn git_identity(commit: Option<&str>, dirty: bool) -> Option<String> {
    let commit = commit.filter(|value| safe(value))?;
    let suffix = if dirty { "-dirty" } else { "" };
    Some(format!("{commit}{suffix}"))
}

pub fn safe(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
