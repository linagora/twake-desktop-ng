use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use clap::Parser;
use tracing::{info, warn};

use twake_sync::cozy::client::{CozyClient, ROOT_DIR_ID};
use twake_sync::fuse::fuse_backend::{TwakeFuseFs, ROOT_INODE};
use twake_sync::fuse::mount_fuse;
use twake_sync::models::FileNode;
use twake_sync::services::upload_queue::UploadQueue;

#[derive(Parser, Debug)]
#[command(name = "twake-vfs", about = "Twake FUSE virtual filesystem")]
struct Args {
    /// Mount point path
    #[arg(short, long)]
    mount: PathBuf,

    /// Cozy stack URL (e.g. https://pvi.stg.lin-saas.com)
    #[arg(short = 'u', long)]
    url: String,

    /// Cozy Bearer token (from DevTools Network tab)
    #[arg(short, long)]
    token: String,

    /// Session cookie string, e.g. "sess-cozyXXX=value; lemonldap=value"
    #[arg(long)]
    cookie: Option<String>,

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

    tokio::fs::create_dir_all(&mount).await?;
    tokio::fs::create_dir_all(&cache_dir).await?;

    info!("Cozy URL: {}", args.url);
    info!("Cache dir: {:?}", cache_dir);

    let cozy = CozyClient::new(&args.url, &args.token, args.cookie);
    
    info!("Starting UploadQueue (simple mode, no DB persistence)");
    let upload_queue = Arc::new(UploadQueue::new_simple(cozy.clone()));
    
    let fs = TwakeFuseFs::new(cache_dir, cozy.clone(), upload_queue);

    // Populate the FUSE tree from Cozy
    info!("Fetching file tree from Cozy...");
    let entries = cozy.list_recursive(ROOT_DIR_ID).await?;

    // Map cozy_id -> inode so we can build parent-child relationships
    let mut path_to_inode: HashMap<String, u64> = HashMap::new();
    path_to_inode.insert("/".to_string(), ROOT_INODE);

    // Sort entries so directories come before their children
    let mut sorted = entries.clone();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    for entry in &sorted {
        let mut node = FileNode::new_ghost(&entry.path, entry.is_dir);
        node.remote_id = Some(entry.id.clone());
        node.size = entry.size;
        node.modified = entry.updated_at.clone();

        let parent_path = std::path::Path::new(&entry.path)
            .parent()
            .map(|p| p.to_str().unwrap_or("/"))
            .unwrap_or("/");
        let parent_path = if parent_path.is_empty() { "/" } else { parent_path };

        let parent_inode = path_to_inode.get(parent_path).copied().unwrap_or_else(|| {
            warn!("Parent path {:?} not found for {:?}, attaching to root", parent_path, entry.path);
            ROOT_INODE
        });

        let inode = fs.register_node(node, parent_inode).await;
        path_to_inode.insert(entry.path.clone(), inode);
    }

    info!("Registered {} entries from Cozy", sorted.len());
    info!("Mounting FUSE at {:?}", mount);

    let mount_handle = mount_fuse(fs, mount.to_str().unwrap()).await?;

    info!("FUSE mounted. Use `fusermount3 -u {:?}` to unmount.", mount);
    mount_handle.await?;

    Ok(())
}
