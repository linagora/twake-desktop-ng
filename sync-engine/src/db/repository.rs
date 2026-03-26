use sqlx::{SqlitePool, Row};
use uuid::Uuid;
use crate::models::{FileNode, FileState};

pub struct FileRepository {
    pub pool: SqlitePool,
}

impl FileRepository {
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(database_url).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn get(&self, path: &str) -> Result<Option<FileNode>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT * FROM file_nodes WHERE path = ?"
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            FileNode {
                id: Uuid::parse_str(&r.get::<String, _>("id")).unwrap(),
                remote_id: r.get("remote_id"),
                path: r.get("path"),
                state: r.get::<String, _>("state").as_str().into(),
                size: r.get::<i64, _>("size") as u64,
                modified: r.get("modified"),
                is_dir: r.get("is_dir"),
                parent_id: r.get::<Option<String>, _>("parent_id").map(|s| Uuid::parse_str(&s).unwrap()),
            }
        }))
    }

    pub async fn insert(&self, node: &FileNode) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO file_nodes (id, remote_id, path, state, size, modified, is_dir, parent_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(node.id.to_string())
        .bind(&node.remote_id)
        .bind(&node.path)
        .bind(node.state.to_string())
        .bind(node.size as i64)
        .bind(&node.modified)
        .bind(node.is_dir as i32)
        .bind(node.parent_id.map(|id| id.to_string()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_state(&self, path: &str, state: FileState) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE file_nodes SET state = ? WHERE path = ?"
        )
        .bind(state.to_string())
        .bind(path)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_dir(&self, parent_path: &str) -> Result<Vec<FileNode>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT * FROM file_nodes WHERE parent_id = (SELECT id FROM file_nodes WHERE path = ?)"
        )
        .bind(parent_path)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| {
            FileNode {
                id: Uuid::parse_str(&r.get::<String, _>("id")).unwrap(),
                remote_id: r.get("remote_id"),
                path: r.get("path"),
                state: r.get::<String, _>("state").as_str().into(),
                size: r.get::<i64, _>("size") as u64,
                modified: r.get("modified"),
                is_dir: r.get("is_dir"),
                parent_id: r.get::<Option<String>, _>("parent_id").map(|s| Uuid::parse_str(&s).unwrap()),
            }
        }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_repository_crud() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        
        let repo = FileRepository { pool };
        
        let mut node = FileNode::new_ghost("/test/file.txt", false);
        node.remote_id = Some("remote-123".to_string());
        node.modified = "2026-01-01T00:00:00Z".to_string();
        node.size = 1024;
        
        repo.insert(&node).await.unwrap();
        
        let retrieved = repo.get("/test/file.txt").await.unwrap().unwrap();
        assert_eq!(retrieved.id, node.id);
        assert_eq!(retrieved.path, "/test/file.txt");
        assert_eq!(retrieved.state, FileState::Ghost);
    }

    #[tokio::test]
    async fn test_update_state() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        
        let repo = FileRepository { pool };
        
        let mut node = FileNode::new_ghost("/test/file.txt", false);
        node.modified = "2026-01-01T00:00:00Z".to_string();
        
        repo.insert(&node).await.unwrap();
        repo.update_state("/test/file.txt", FileState::Hydrated).await.unwrap();
        
        let retrieved = repo.get("/test/file.txt").await.unwrap().unwrap();
        assert_eq!(retrieved.state, FileState::Hydrated);
    }

    #[tokio::test]
    async fn test_list_dir() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        
        let repo = FileRepository { pool };
        
        let mut parent = FileNode::new_ghost("/test", true);
        parent.id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        parent.modified = "2026-01-01T00:00:00Z".to_string();
        repo.insert(&parent).await.unwrap();
        
        let mut child1 = FileNode::new_ghost("/test/file1.txt", false);
        child1.parent_id = Some(parent.id);
        child1.modified = "2026-01-01T00:00:00Z".to_string();
        repo.insert(&child1).await.unwrap();
        
        let mut child2 = FileNode::new_ghost("/test/file2.txt", false);
        child2.parent_id = Some(parent.id);
        child2.modified = "2026-01-01T00:00:00Z".to_string();
        repo.insert(&child2).await.unwrap();
        
        let children = repo.list_dir("/test").await.unwrap();
        assert_eq!(children.len(), 2);
    }
}
