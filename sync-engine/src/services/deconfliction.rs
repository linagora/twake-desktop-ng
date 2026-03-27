use crate::cozy::client::CozyEntry;
use crate::models::FileNode;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ConflictResult {
    NoConflict,
    Conflict {
        local_modified: String,
        remote_modified: String,
        conflict_path: PathBuf,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum DeconflictError {
    #[error("Invalid date format: {0}")]
    InvalidDate(String),
}

pub struct DeconflictionService;

impl DeconflictionService {
    pub fn new() -> Self {
        Self
    }

    pub fn check_conflict(
        &self,
        local_node: &FileNode,
        remote_entry: Option<&CozyEntry>,
    ) -> Result<ConflictResult, DeconflictError> {
        let Some(remote) = remote_entry else {
            return Ok(ConflictResult::NoConflict);
        };

        let local_dt = self.parse_date(&local_node.modified)?;
        let remote_dt = self.parse_date(&remote.updated_at)?;

        if remote_dt > local_dt {
            let conflict_path = self.generate_conflict_path(Path::new(&local_node.path));
            Ok(ConflictResult::Conflict {
                local_modified: local_node.modified.clone(),
                remote_modified: remote.updated_at.clone(),
                conflict_path,
            })
        } else {
            Ok(ConflictResult::NoConflict)
        }
    }

    pub fn generate_conflict_path(&self, original: &Path) -> PathBuf {
        let parent = original.parent().unwrap_or(Path::new(""));
        let filename = original.file_name().unwrap_or_default().to_string_lossy();

        let parts: Vec<&str> = filename.split('.').collect();
        let conflict_name = if parts.len() > 1 {
            let name = parts[..parts.len() - 1].join(".");
            let ext = parts[parts.len() - 1];
            let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
            format!("{}.conflict.{}.{}", name, timestamp, ext)
        } else {
            let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
            format!("{}.conflict.{}", filename, timestamp)
        };

        parent.join(conflict_name)
    }

    fn parse_date(&self, date_str: &str) -> Result<DateTime<Utc>, DeconflictError> {
        DateTime::parse_from_rfc3339(date_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| DeconflictError::InvalidDate(date_str.to_string()))
    }
}

impl Default for DeconflictionService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_node(path: &str, modified: &str) -> FileNode {
        FileNode {
            id: uuid::Uuid::new_v4(),
            remote_id: Some("remote-123".to_string()),
            path: path.to_string(),
            state: crate::models::FileState::Modified,
            size: 1024,
            modified: modified.to_string(),
            is_dir: false,
            parent_id: None,
        }
    }

    fn create_remote_entry(id: &str, updated_at: &str) -> CozyEntry {
        CozyEntry {
            id: id.to_string(),
            name: "test.txt".to_string(),
            path: "/test.txt".to_string(),
            is_dir: false,
            size: 1024,
            updated_at: updated_at.to_string(),
        }
    }

    #[test]
    fn test_no_conflict_when_no_remote_entry() {
        let service = DeconflictionService::new();
        let node = create_test_node("/test.txt", "2026-03-27T10:00:00Z");

        let result = service.check_conflict(&node, None).unwrap();
        assert!(matches!(result, ConflictResult::NoConflict));
    }

    #[test]
    fn test_no_conflict_when_local_is_newer() {
        let service = DeconflictionService::new();
        let node = create_test_node("/test.txt", "2026-03-27T12:00:00Z");
        let remote = create_remote_entry("remote-123", "2026-03-27T10:00:00Z");

        let result = service.check_conflict(&node, Some(&remote)).unwrap();
        assert!(matches!(result, ConflictResult::NoConflict));
    }

    #[test]
    fn test_conflict_when_remote_is_newer() {
        let service = DeconflictionService::new();
        let node = create_test_node("/test.txt", "2026-03-27T10:00:00Z");
        let remote = create_remote_entry("remote-123", "2026-03-27T12:00:00Z");

        let result = service.check_conflict(&node, Some(&remote)).unwrap();
        assert!(matches!(result, ConflictResult::Conflict { .. }));

        if let ConflictResult::Conflict {
            local_modified,
            remote_modified,
            conflict_path,
        } = result
        {
            assert_eq!(local_modified, "2026-03-27T10:00:00Z");
            assert_eq!(remote_modified, "2026-03-27T12:00:00Z");
            assert!(conflict_path.to_string_lossy().contains("conflict"));
        }
    }

    #[test]
    fn test_invalid_date_returns_error() {
        let service = DeconflictionService::new();
        let node = create_test_node("/test.txt", "invalid-date");
        let remote = create_remote_entry("remote-123", "2026-03-27T10:00:00Z");

        let result = service.check_conflict(&node, Some(&remote));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeconflictError::InvalidDate(_)
        ));
    }

    #[test]
    fn test_generate_conflict_path_with_extension() {
        let service = DeconflictionService::new();
        let path = Path::new("/documents/report.txt");
        let conflict_path = service.generate_conflict_path(path);

        let conflict_str = conflict_path.to_string_lossy();
        assert!(conflict_str.contains("report.conflict."));
        assert!(conflict_str.ends_with(".txt"));
        assert!(conflict_str.contains("/documents/"));
    }

    #[test]
    fn test_generate_conflict_path_without_extension() {
        let service = DeconflictionService::new();
        let path = Path::new("/documents/Makefile");
        let conflict_path = service.generate_conflict_path(path);

        let conflict_str = conflict_path.to_string_lossy();
        assert!(conflict_str.contains("Makefile.conflict."));
        assert!(conflict_str.contains("/documents/"));
    }

    #[test]
    fn test_generate_conflict_path_with_double_extension() {
        let service = DeconflictionService::new();
        let path = Path::new("/documents/archive.tar.gz");
        let conflict_path = service.generate_conflict_path(path);

        let conflict_str = conflict_path.to_string_lossy();
        assert!(conflict_str.contains("archive.tar.conflict."));
        assert!(conflict_str.ends_with(".gz"));
    }

    #[test]
    fn test_generate_conflict_path_has_timestamp() {
        let service = DeconflictionService::new();
        let path = Path::new("/test.txt");
        let conflict_path = service.generate_conflict_path(path);

        let conflict_str = conflict_path.to_string_lossy();
        assert!(conflict_str.contains(".conflict."));
        let parts: Vec<&str> = conflict_str.split('.').collect();
        if parts.len() >= 3 {
            let timestamp_part = parts[parts.len() - 2];
            assert_eq!(timestamp_part.len(), 15);
        }
    }
}
