use bytes::Bytes;
use reqwest::Client;
use serde::{Deserialize, Deserializer};
use tracing::{debug, info};

pub const ROOT_DIR_ID: &str = "io.cozy.files.root-dir";

/// A file or directory entry returned by the Cozy /files API.
#[derive(Debug, Clone)]
pub struct CozyEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub updated_at: String,
}

/// Minimal Cozy HTTP client for the io.cozy.files doctype.
#[derive(Clone)]
pub struct CozyClient {
    http: Client,
    base_url: String,
    token: String,
    cookie: Option<String>,
}

impl CozyClient {
    pub fn new(base_url: &str, token: &str, cookie: Option<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            cookie,
        }
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let req = req.header("Authorization", format!("Bearer {}", self.token));
        match &self.cookie {
            Some(c) => req.header("Cookie", c.as_str()),
            None => req,
        }
    }

    /// List direct children of a directory by its Cozy ID.
    /// Handles pagination automatically.
    pub async fn list_dir(&self, dir_id: &str) -> Result<Vec<CozyEntry>, CozyError> {
        let mut entries = Vec::new();
        let mut url = format!(
            "{}/files/{}?page[limit]=100",
            self.base_url, dir_id
        );

        loop {
            debug!("GET {}", url);
            let req = self.http
                .get(&url)
                .header("Accept", "application/vnd.api+json");
            let resp = self.apply_auth(req).send().await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(CozyError::Api {
                    status: status.as_u16(),
                    body,
                });
            }

            let body: DirResponse = resp.json().await?;

            // Get parent dir path from the `data` field to construct child paths
            let parent_path = body.data
                .as_ref()
                .and_then(|d| d.attributes.path.clone())
                .unwrap_or_else(|| "/".to_string());

            for inc in &body.included {
                let is_dir = inc.attributes.r#type == "directory";
                let size = if is_dir {
                    0
                } else {
                    inc.attributes.size.unwrap_or(0)
                };
                // Use the entry's path if present, otherwise build from parent + name
                let path = inc.attributes.path.clone().unwrap_or_else(|| {
                    let p = parent_path.trim_end_matches('/');
                    format!("{}/{}", p, inc.attributes.name)
                });
                entries.push(CozyEntry {
                    id: inc.id.clone(),
                    name: inc.attributes.name.clone(),
                    path,
                    is_dir,
                    size,
                    updated_at: inc.attributes.updated_at.clone().unwrap_or_default(),
                });
            }

            // Follow pagination
            match body.links.and_then(|l| l.next) {
                Some(next) if !next.is_empty() => {
                    url = if next.starts_with("http") {
                        next
                    } else {
                        format!("{}{}", self.base_url, next)
                    };
                }
                _ => break,
            }
        }

        info!("Listed {} entries in dir {}", entries.len(), dir_id);
        Ok(entries)
    }

    /// Recursively list all files and directories starting from dir_id.
    pub async fn list_recursive(&self, dir_id: &str) -> Result<Vec<CozyEntry>, CozyError> {
        let mut all = Vec::new();
        let mut stack = vec![dir_id.to_string()];

        while let Some(current_dir) = stack.pop() {
            let children = self.list_dir(&current_dir).await?;
            for child in &children {
                if child.is_dir {
                    stack.push(child.id.clone());
                }
            }
            all.extend(children);
        }

        info!("Recursive listing: {} total entries", all.len());
        Ok(all)
    }

    /// Download a file's content by its Cozy ID.
    pub async fn download(&self, file_id: &str) -> Result<Bytes, CozyError> {
        let url = format!("{}/files/download/{}", self.base_url, file_id);
        debug!("GET {}", url);

        let req = self.http.get(&url);
        let resp = self.apply_auth(req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CozyError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let bytes = resp.bytes().await?;
        info!("Downloaded {} bytes for file {}", bytes.len(), file_id);
        Ok(bytes)
    }
}

// -- JSON:API deserialization structs --

#[derive(Deserialize)]
struct DirResponse {
    data: Option<DirData>,
    #[serde(default)]
    included: Vec<IncludedEntry>,
    links: Option<PaginationLinks>,
}

#[derive(Deserialize)]
struct DirData {
    attributes: DirDataAttributes,
}

#[derive(Deserialize)]
struct DirDataAttributes {
    path: Option<String>,
}

#[derive(Deserialize)]
struct PaginationLinks {
    next: Option<String>,
}

#[derive(Deserialize)]
struct IncludedEntry {
    id: String,
    attributes: EntryAttributes,
}

#[derive(Deserialize)]
struct EntryAttributes {
    r#type: String,
    name: String,
    path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_size")]
    size: Option<u64>,
    updated_at: Option<String>,
}

/// Cozy returns size as either a number or a string — handle both.
fn deserialize_size<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNum {
        Num(u64),
        Str(String),
    }
    match Option::<StringOrNum>::deserialize(d)? {
        Some(StringOrNum::Num(n)) => Ok(Some(n)),
        Some(StringOrNum::Str(s)) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

// -- Errors --

#[derive(Debug, thiserror::Error)]
pub enum CozyError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Cozy API error {status}: {body}")]
    Api { status: u16, body: String },
}
