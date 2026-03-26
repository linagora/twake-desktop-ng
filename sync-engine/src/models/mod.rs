pub mod file_node;
pub mod file_state;
pub mod file_status;

pub use file_node::FileNode;
pub use file_state::FileState;
pub use file_status::FileStatus;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_state_display() {
        assert_eq!(FileState::Ghost.to_string(), "ghost");
        assert_eq!(FileState::Hydrated.to_string(), "hydrated");
        assert_eq!(FileState::Modified.to_string(), "modified");
        assert_eq!(FileState::Syncing.to_string(), "syncing");
        assert_eq!(FileState::Conflict.to_string(), "conflict");
        assert_eq!(FileState::Error.to_string(), "error");
    }

    #[test]
    fn test_file_state_from_str() {
        assert_eq!(FileState::from("ghost"), FileState::Ghost);
        assert_eq!(FileState::from("GHOST"), FileState::Ghost);
        assert_eq!(FileState::from("hydrated"), FileState::Hydrated);
        assert_eq!(FileState::from("MODIFIED"), FileState::Modified);
        assert_eq!(FileState::from("unknown"), FileState::Error);
    }

    #[test]
    fn test_file_node_new_ghost() {
        let node = FileNode::new_ghost("/test/file.txt", false);
        
        assert_eq!(node.path, "/test/file.txt");
        assert_eq!(node.state, FileState::Ghost);
        assert_eq!(node.size, 0);
        assert!(!node.is_dir);
        assert!(node.id != uuid::Uuid::nil());
    }

    #[test]
    fn test_file_node_serialization() {
        let node = FileNode::new_ghost("/test/file.txt", false);
        let json = node.to_json().unwrap();
        
        let deserialized = FileNode::from_json(&json).unwrap();
        
        assert_eq!(node.id, deserialized.id);
        assert_eq!(node.path, deserialized.path);
        assert_eq!(node.state, deserialized.state);
    }
}
