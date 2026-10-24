/// Drops a trailing width label after a device word: `desktop 1440`,
/// `mobile (360px)`. Names without a device word keep their trailing number.
fn strip_width_label(name: String) -> String {
    let trimmed = name.trim_end_matches([')', ' ']);
    let trimmed = trimmed.strip_suffix("px").unwrap_or(trimmed);
    let without_digits = trimmed.trim_end_matches(|c: char| c.is_ascii_digit());
    if without_digits.len() == trimmed.len() {
        return name;
    }
    let base = without_digits.trim_end_matches([' ', '-', '/', '(', '_']);
    if ["desktop", "tablet", "mobile"]
        .iter()
        .any(|device| base.ends_with(device))
    {
        base.to_owned()
    } else {
        name
    }
}
