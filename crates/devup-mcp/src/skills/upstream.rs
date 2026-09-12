use std::{
    fmt::{Display, Formatter},
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use reqwest::header::ETAG;

pub const SKILL_SOURCE_URL: &str =
    "https://raw.githubusercontent.com/dev-five-git/devup-ui/refs/heads/main/SKILL.md";

#[derive(Debug, Clone)]
pub struct FetchedSkill {
    pub contents: Vec<u8>,
    pub etag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillFetchError {
    Network(String),
    NotFound,
    HttpStatus(u16),
    MissingEtag,
}

impl Display for SkillFetchError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(message) => write!(
                formatter,
                "network failure while fetching {SKILL_SOURCE_URL}: {message}. Check connectivity and retry."
            ),
            Self::NotFound => write!(
                formatter,
                "GitHub returned 404 for {SKILL_SOURCE_URL}. Verify that the public main-branch SKILL.md still exists."
            ),
            Self::HttpStatus(status) => write!(
                formatter,
                "GitHub returned HTTP {status} for {SKILL_SOURCE_URL}. Retry later or verify GitHub availability."
            ),
            Self::MissingEtag => write!(
                formatter,
                "GitHub returned {SKILL_SOURCE_URL} without an ETag, so its provenance cannot be verified."
            ),
        }
    }
}

impl std::error::Error for SkillFetchError {}

#[async_trait]
pub trait SkillUpstream: Send + Sync {
    async fn fetch(&self) -> Result<FetchedSkill, SkillFetchError>;
}

pub struct RawGithubSkillUpstream {
    client: reqwest::Client,
}

impl RawGithubSkillUpstream {
    pub fn new() -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("devup-mcp/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("Could not create the HTTPS client for skill delivery.")?;
        Ok(Self { client })
    }
}

#[async_trait]
impl SkillUpstream for RawGithubSkillUpstream {
    async fn fetch(&self) -> Result<FetchedSkill, SkillFetchError> {
        let response = self
            .client
            .get(SKILL_SOURCE_URL)
            .send()
            .await
            .map_err(|error| SkillFetchError::Network(error.to_string()))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(SkillFetchError::NotFound);
        }
        if !status.is_success() {
            return Err(SkillFetchError::HttpStatus(status.as_u16()));
        }
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .ok_or(SkillFetchError::MissingEtag)?
            .to_owned();
        let contents = response
            .bytes()
            .await
            .map_err(|error| SkillFetchError::Network(error.to_string()))?
            .to_vec();
        Ok(FetchedSkill { contents, etag })
    }
}
