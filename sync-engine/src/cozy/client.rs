use bytes::Bytes;
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};
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

    /// Upload a new file to Cozy
    pub async fn upload(
        &self,
        dir_id: &str,
        name: &str,
        content: &[u8],
    ) -> Result<CozyFileResult, CozyError> {
        let url = format!(
            "{}/files/{}?Name={}",
            self.base_url,
            dir_id,
            urlencoding::encode(name)
        );
        debug!("POST {}", url);

        let req = self
            .http
            .post(&url)
            .header("Accept", "application/vnd.api+json")
            .header("Content-Type", "application/octet-stream")
            .header("If-None-Match", "*");

        let resp = self.apply_auth(req).body(content.to_vec()).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CozyError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let result: FileUploadResponse = resp.json().await?;
        let file_result = CozyFileResult {
            id: result.data.id,
            rev: result.data.meta.rev,
            path: result.data.attributes.path,
            updated_at: result.data.attributes.updated_at,
        };

        info!("Uploaded file {} to dir {} (id: {})", name, dir_id, file_result.id);
        Ok(file_result)
    }

    /// Update an existing file's content
    pub async fn update(
        &self,
        file_id: &str,
        content: &[u8],
        if_match: Option<&str>,
    ) -> Result<CozyFileResult, CozyError> {
        let url = format!("{}/files/{}", self.base_url, file_id);
        debug!("PUT {}", url);

        let mut req = self
            .http
            .put(&url)
            .header("Accept", "application/vnd.api+json")
            .header("Content-Type", "application/octet-stream");

        if let Some(rev) = if_match {
            req = req.header("If-Match", rev);
        }

        let resp = self.apply_auth(req).body(content.to_vec()).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CozyError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let result: FileUploadResponse = resp.json().await?;
        let file_result = CozyFileResult {
            id: result.data.id,
            rev: result.data.meta.rev,
            path: result.data.attributes.path,
            updated_at: result.data.attributes.updated_at,
        };

        info!("Updated file {} (rev: {})", file_id, file_result.rev);
        Ok(file_result)
    }

    /// Delete a file from Cozy
    pub async fn delete(&self, file_id: &str, if_match: Option<&str>) -> Result<(), CozyError> {
        let url = format!("{}/files/{}", self.base_url, file_id);
        debug!("DELETE {}", url);

        let mut req = self.http.delete(&url);

        if let Some(rev) = if_match {
            req = req.header("If-Match", rev);
        }

        let resp = self.apply_auth(req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CozyError::Api {
                status: status.as_u16(),
                body,
            });
        }

        info!("Deleted file {}", file_id);
        Ok(())
    }

    /// Create a directory
    pub async fn mkdir(
        &self,
        parent_id: &str,
        name: &str,
    ) -> Result<CozyFolderResult, CozyError> {
        let url = format!(
            "{}/folders?Name={}&parent={}",
            self.base_url,
            urlencoding::encode(name),
            parent_id
        );
        debug!("POST {}", url);

        let req = self
            .http
            .post(&url)
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

        let result: FolderCreateResponse = resp.json().await?;
        let folder_result = CozyFolderResult {
            id: result.data.id,
            rev: result.data.meta.rev,
            path: result.data.attributes.path,
        };

        info!("Created directory {} in parent {} (id: {})", name, parent_id, folder_result.id);
        Ok(folder_result)
    }

    /// Move/rename a file or directory
    pub async fn r#move(
        &self,
        file_id: &str,
        new_dir_id: &str,
        new_name: Option<&str>,
        if_match: Option<&str>,
    ) -> Result<CozyFileResult, CozyError> {
        let url = format!("{}/files/{}", self.base_url, file_id);
        debug!("PATCH {}", url);

        #[derive(Serialize)]
        struct MoveRequest {
            data: MoveData,
        }

        #[derive(Serialize)]
        struct MoveData {
            r#type: &'static str,
            id: String,
            relationships: MoveRelationships,
            #[serde(skip_serializing_if = "Option::is_none")]
            attributes: Option<MoveAttributes>,
        }

        #[derive(Serialize)]
        struct MoveRelationships {
            parent: ParentData,
        }

        #[derive(Serialize)]
        struct ParentData {
            data: ParentType,
        }

        #[derive(Serialize)]
        struct ParentType {
            r#type: &'static str,
            id: String,
        }

        #[derive(Serialize)]
        struct MoveAttributes {
            #[serde(skip_serializing_if = "Option::is_none")]
            name: Option<String>,
        }

        let body = MoveRequest {
            data: MoveData {
                r#type: "file",
                id: file_id.to_string(),
                relationships: MoveRelationships {
                    parent: ParentData {
                        data: ParentType {
                            r#type: "directory",
                            id: new_dir_id.to_string(),
                        },
                    },
                },
                attributes: new_name.map(|name| MoveAttributes {
                    name: Some(name.to_string()),
                }),
            },
        };

        let mut req = self
            .http
            .patch(&url)
            .header("Accept", "application/vnd.api+json")
            .header("Content-Type", "application/vnd.api+json");

        if let Some(rev) = if_match {
            req = req.header("If-Match", rev);
        }

        let resp = self.apply_auth(req).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CozyError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let result: FileUploadResponse = resp.json().await?;
        let file_result = CozyFileResult {
            id: result.data.id,
            rev: result.data.meta.rev,
            path: result.data.attributes.path,
            updated_at: result.data.attributes.updated_at,
        };

        info!("Moved file {} to dir {} (new name: {:?})", file_id, new_dir_id, new_name);
        Ok(file_result)
    }
}

// -- Result types --

#[derive(Debug, Clone)]
pub struct CozyFileResult {
    pub id: String,
    pub rev: String,
    pub path: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct CozyFolderResult {
    pub id: String,
    pub rev: String,
    pub path: String,
}

// -- JSON:API deserialization structs --

#[derive(Deserialize)]
struct FileUploadResponse {
    data: FileData,
}

#[derive(Deserialize)]
struct FileData {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    r#type: String,
    attributes: FileAttributes,
    meta: FileMeta,
}

#[derive(Deserialize)]
struct FileAttributes {
    path: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct FileMeta {
    rev: String,
}

#[derive(Deserialize)]
struct FolderCreateResponse {
    data: FolderData,
}

#[derive(Deserialize)]
struct FolderData {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    r#type: String,
    attributes: FolderAttributes,
    meta: FolderMeta,
}

#[derive(Deserialize)]
struct FolderAttributes {
    path: String,
}

#[derive(Deserialize)]
struct FolderMeta {
    rev: String,
}

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

mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::new();
        for c in s.chars() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                    result.push(c);
                }
                c => {
                    for byte in c.to_string().as_bytes() {
                        result.push_str(&format!("%{:02X}", byte));
                    }
                }
            }
        }
        result
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::FileState;

    #[test]
    fn test_urlencoding_preserves_alphanumeric() {
        assert_eq!(urlencoding::encode("hello123"), "hello123");
        assert_eq!(urlencoding::encode("test-file_name.txt"), "test-file_name.txt");
    }

    #[test]
    fn test_urlencoding_encodes_special_chars() {
        assert_eq!(urlencoding::encode("hello world"), "hello%20world");
        assert_eq!(urlencoding::encode("file&name"), "file%26name");
        assert_eq!(urlencoding::encode("café"), "caf%C3%A9");
    }

    #[test]
    fn test_file_state_display() {
        assert_eq!(FileState::Ghost.to_string(), "ghost");
        assert_eq!(FileState::Hydrated.to_string(), "hydrated");
        assert_eq!(FileState::Modified.to_string(), "modified");
        assert_eq!(FileState::Syncing.to_string(), "syncing");
        assert_eq!(FileState::Synced.to_string(), "synced");
        assert_eq!(FileState::Conflict.to_string(), "conflict");
        assert_eq!(FileState::Error.to_string(), "error");
    }

    #[test]
    fn test_file_state_from_str() {
        assert_eq!(FileState::from("ghost"), FileState::Ghost);
        assert_eq!(FileState::from("GHOST"), FileState::Ghost);
        assert_eq!(FileState::from("hydrated"), FileState::Hydrated);
        assert_eq!(FileState::from("modified"), FileState::Modified);
        assert_eq!(FileState::from("syncing"), FileState::Syncing);
        assert_eq!(FileState::from("synced"), FileState::Synced);
        assert_eq!(FileState::from("conflict"), FileState::Conflict);
        assert_eq!(FileState::from("error"), FileState::Error);
        assert_eq!(FileState::from("invalid"), FileState::Error);
    }
}
