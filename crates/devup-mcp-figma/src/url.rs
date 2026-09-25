use url::Url;

use serde::{Deserialize, Serialize};

use super::DevupError;

/// The link for the file the one attached Devup Bridge plugin has open.
///
/// A plugin often cannot say which file that is - `figma.fileKey` comes back
/// empty in Dev Mode - so there is no Figma link to hand an agent. The agent
/// that needed one invented `/design/bridge/bridge` to get past the url field
/// and was then told the invented key was the file's. This is the link to use
/// instead: it routes to the plugin rather than to Figma, `?node-id=` narrows
/// it as on any Figma link, and without one the node selected in Figma is
/// meant.
pub const BRIDGE_CURRENT_URL: &str = "figma-bridge://current";

/// Figma file keys never contain `:`, so a key that starts with this can only
/// ever name a bridge plugin - never a file the direct path could fetch.
pub(crate) const BRIDGE_KEY_PREFIX: &str = "bridge:";

/// What `figma-bridge://current` parses to: a placeholder the server binds to
/// the attached plugin before anything is read.
pub const BRIDGE_CURRENT_KEY: &str = "bridge:current";

/// Whether only a bridge plugin can serve `file_key`.
pub fn is_bridge_only_key(file_key: &str) -> bool {
    file_key.starts_with(BRIDGE_KEY_PREFIX)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigmaTarget {
    pub file_key: String,
    pub node_id: Option<String>,
    pub branch_key: Option<String>,
}

impl FigmaTarget {
    /// The file the one attached bridge plugin has open, which is what a
    /// request made with no url means.
    pub fn bridge_current() -> Self {
        Self {
            file_key: BRIDGE_CURRENT_KEY.to_owned(),
            node_id: None,
            branch_key: None,
        }
    }

    pub fn parse(input: &str) -> Result<Self, DevupError> {
        let url = Url::parse(input)
            .map_err(|_| DevupError::unsupported_file("Not a valid Figma link."))?;
        if url.scheme() == "figma-bridge" {
            if url.host_str() != Some("current") || !matches!(url.path(), "" | "/") {
                return Err(DevupError::unsupported_file(
                    "The only bridge link is figma-bridge://current, optionally with ?node-id=.",
                ));
            }
            return Ok(Self {
                node_id: node_id_param(&url)?,
                ..Self::bridge_current()
            });
        }
        if url.scheme() != "https" || !matches!(url.host_str(), Some("figma.com" | "www.figma.com"))
        {
            return Err(DevupError::unsupported_file(
                "Only HTTPS Figma design links are supported.",
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
                    "Not a supported Figma design, file, or branch link.",
                ));
            }
        };

        validate_key(&file_key)?;
        if let Some(branch_key) = &branch_key {
            validate_key(branch_key)?;
        }

        Ok(Self {
            file_key,
            node_id: node_id_param(&url)?,
            branch_key,
        })
    }

    /// Whether this names the file a bridge plugin has open rather than a
    /// file Figma itself can serve.
    pub fn is_bridge_only(&self) -> bool {
        is_bridge_only_key(&self.file_key)
    }

    /// A link that routes back to this target, narrowed to `node_id` when one
    /// is given.
    ///
    /// A bridge-only target links as `figma-bridge://current`. Wrapping its key
    /// in a Figma URL would hand the caller a link to a file that does not
    /// exist, under a key it would then take for the file's own.
    pub fn link(&self, node_id: Option<&str>) -> String {
        let base = if self.is_bridge_only() {
            BRIDGE_CURRENT_URL.to_owned()
        } else if let Some(branch_key) = &self.branch_key {
            format!(
                "https://www.figma.com/branch/{}/{branch_key}/devup",
                self.file_key
            )
        } else {
            format!("https://www.figma.com/design/{}/devup", self.file_key)
        };
        match node_id {
            Some(node_id) => format!("{base}?node-id={}", node_id.replace(':', "-")),
            None => base,
        }
    }
}

fn node_id_param(url: &Url) -> Result<Option<String>, DevupError> {
    url.query_pairs()
        .find_map(|(name, value)| (name == "node-id").then(|| value.into_owned()))
        .map(|node_id| normalize_node_id(&node_id))
        .transpose()
}

fn validate_key(key: &str) -> Result<(), DevupError> {
    if key.len() < 6
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(DevupError::unsupported_file(
            "Figma file or branch key format is invalid.",
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
            "Figma node-id format is invalid.",
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
            "Figma node-id format is invalid.",
        ));
    }
    Ok(normalized)
}
