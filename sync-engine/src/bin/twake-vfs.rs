use std::path::PathBuf;
use clap::Parser;
use tracing::info;

use twake_sync::fuse::fuse_backend::{TwakeFuseFs, ROOT_INODE};
use twake_sync::fuse::mount_fuse;
use twake_sync::models::FileNode;

#[derive(Parser, Debug)]
#[command(name = "twake-vfs", about = "Twake FUSE virtual filesystem")]
struct Args {
    /// Mount point path
    #[arg(short, long)]
    mount: PathBuf,

    /// Cache directory for hydrated files
    #[arg(short, long)]
    cache: Option<PathBuf>,
}

fn expand_tilde(path: &PathBuf) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~") {
        let home = std::env::var("HOME").expect("HOME not set");
        PathBuf::from(home).join(stripped)
    } else {
        path.clone()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let mount = expand_tilde(&args.mount);

    let cache_dir = args.cache
        .map(|p| expand_tilde(&p))
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").expect("HOME not set");
            PathBuf::from(home).join(".twake").join("cache")
        });

    // Create mount point if needed
    tokio::fs::create_dir_all(&mount).await?;
    tokio::fs::create_dir_all(&cache_dir).await?;

    info!("Cache dir: {:?}", cache_dir);

    let fs = TwakeFuseFs::new(cache_dir);

    // Register demo files — in production this comes from Cozy changes feed
    let docs_dir = FileNode::new_ghost("/documents", true);
    let docs_inode = fs.register_node(docs_dir, ROOT_INODE).await;

    let mut file1 = FileNode::new_ghost("/documents/rapport.pdf", false);
    file1.size = 5 * 1024 * 1024; // 5 MB
    file1.modified = "2026-03-26T10:00:00Z".to_string();
    fs.register_node(file1, docs_inode).await;

    let mut file2 = FileNode::new_ghost("/documents/notes.txt", false);
    file2.size = 1024;
    file2.modified = "2026-03-26T09:00:00Z".to_string();
    fs.register_node(file2, docs_inode).await;

    let shared_dir = FileNode::new_ghost("/shared", true);
    let shared_inode = fs.register_node(shared_dir, ROOT_INODE).await;

    let mut file3 = FileNode::new_ghost("/shared/spec.md", false);
    file3.size = 2048;
    file3.modified = "2026-03-25T14:00:00Z".to_string();
    fs.register_node(file3, shared_inode).await;

    info!("Mounting FUSE at {:?}", mount);
    let mount_handle = mount_fuse(fs, mount.to_str().unwrap()).await?;

    info!("FUSE mounted. Use `fusermount3 -u {:?}` to unmount.", mount);
    mount_handle.await?;

    Ok(())
}
