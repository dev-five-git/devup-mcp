use url::Url;

use super::DevupError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FigmaTarget {
    pub file_key: String,
    pub node_id: Option<String>,
    pub branch_key: Option<String>,
}

impl FigmaTarget {
    pub fn parse(input: &str) -> Result<Self, DevupError> {
        let url = Url::parse(input)
            .map_err(|_| DevupError::unsupported_file("올바른 Figma 링크가 아닙니다."))?;
        if url.scheme() != "https" || !matches!(url.host_str(), Some("figma.com" | "www.figma.com"))
        {
            return Err(DevupError::unsupported_file(
                "HTTPS Figma 디자인 링크만 사용할 수 있습니다.",
            ));
        }

        let segments = url
            .path_segments()
            .map(|segments| segments.filter(|part| !part.is_empty()).collect::<Vec<_>>())
            .unwrap_or_default();
        let (file_key, branch_key) = match segments.as_slice() {
            ["design" | "file", file_key, ..] => ((*file_key).to_owned(), None),
            ["branch", file_key, branch_key, ..] => {
                ((*file_key).to_owned(), Some((*branch_key).to_owned()))
            }
            _ => {
                return Err(DevupError::unsupported_file(
                    "지원하는 Figma design, file 또는 branch 링크가 아닙니다.",
                ));
            }
        };

        validate_key(&file_key)?;
        if let Some(branch_key) = &branch_key {
            validate_key(branch_key)?;
        }

        let node_id = url
            .query_pairs()
            .find_map(|(name, value)| (name == "node-id").then(|| value.into_owned()))
            .map(|node_id| normalize_node_id(&node_id))
            .transpose()?;

        Ok(Self {
            file_key,
            node_id,
            branch_key,
        })
    }
}

fn validate_key(key: &str) -> Result<(), DevupError> {
    if key.len() < 6
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(DevupError::unsupported_file(
            "Figma 파일 또는 브랜치 키 형식이 올바르지 않습니다.",
        ));
    }
    Ok(())
}

fn normalize_node_id(node_id: &str) -> Result<String, DevupError> {
    let normalized = if node_id.contains(':') {
        node_id.to_owned()
    } else if let Some((left, right)) = node_id.split_once('-') {
        format!("{left}:{right}")
    } else {
        return Err(DevupError::unsupported_file(
            "Figma node-id 형식이 올바르지 않습니다.",
        ));
    };

    let mut parts = normalized.split(':');
    let valid = matches!((parts.next(), parts.next(), parts.next()), (Some(left), Some(right), None)
        if !left.is_empty()
            && !right.is_empty()
            && left.bytes().all(|byte| byte.is_ascii_digit())
            && right.bytes().all(|byte| byte.is_ascii_digit()));
    if !valid {
        return Err(DevupError::unsupported_file(
            "Figma node-id 형식이 올바르지 않습니다.",
        ));
    }
    Ok(normalized)
}
