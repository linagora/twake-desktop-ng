use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use clap::Parser;
use tracing::{info, warn, error};

use twake_sync::cozy::client::{CozyClient, ROOT_DIR_ID};
use twake_sync::cozy::realtime::{realtime_connect, RealtimeEvent};
use twake_sync::fuse::fuse_backend::{TwakeFuseFs, SharedVfsState, ROOT_INODE};
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

    /// Fallback sync interval in seconds (default: 60)
    #[arg(long, default_value = "60")]
    sync_interval: u64,
}

fn expand_tilde(path: &PathBuf) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~") {
        let home = std::env::var("HOME").expect("HOME not set");
        PathBuf::from(home).join(stripped)
    } else {
        path.clone()
    }
}

/// Full sync: re-list remote and reconcile with local VFS.
async fn do_sync(shared: &SharedVfsState, cozy: &CozyClient) {
    match cozy.list_recursive(ROOT_DIR_ID).await {
        Ok(entries) => {
            shared.sync_with_remote(&entries).await;
            info!("Sync: done ({} remote entries)", entries.len());
        }
        Err(e) => {
            error!("Sync: failed to list remote: {}", e);
        }
    }
}

/// Fallback polling loop (in case WebSocket disconnects).
async fn sync_loop(shared: SharedVfsState, cozy: CozyClient, interval_secs: u64) {
    let interval = std::time::Duration::from_secs(interval_secs);
    loop {
        tokio::time::sleep(interval).await;
        info!("Sync poll: checking remote...");
        do_sync(&shared, &cozy).await;
    }
}

/// Realtime WebSocket loop with auto-reconnect.
async fn realtime_loop(
    shared: SharedVfsState,
    cozy: CozyClient,
    base_url: String,
    token: String,
    cookie: Option<String>,
) {
    loop {
        let shared_clone = shared.clone();
        let cozy_clone = cozy.clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<RealtimeEvent>(64);

        // Spawn WS connection in a task
        let url = base_url.clone();
        let tok = token.clone();
        let cook = cookie.clone();
        let ws_handle = tokio::spawn(async move {
            let result = realtime_connect(&url, &tok, cook.as_deref(), move |event| {
                let _ = tx.try_send(event);
            }).await;
            if let Err(e) = result {
                error!("Realtime: connection error: {}", e);
            }
        });

        // Process events — trigger sync on each change
        while let Some(event) = rx.recv().await {
            info!("Realtime event: {:?} — triggering sync", event);
            do_sync(&shared_clone, &cozy_clone).await;
        }

        // WS disconnected, wait and reconnect
        let _ = ws_handle.await;
        warn!("Realtime: disconnected, reconnecting in 5s...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let mount = expand_tilde(&args.mount);
    let sync_interval = args.sync_interval;

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

    let cozy = CozyClient::new(&args.url, &args.token, args.cookie.clone());

    let mut fs = TwakeFuseFs::new_without_upload_queue(cache_dir, cozy.clone());
    let shared = fs.shared_state();

    info!("Starting UploadQueue with shared VFS state");
    let upload_queue = Arc::new(UploadQueue::new_with_shared_state(cozy.clone(), shared.clone()));
    fs.set_upload_queue(upload_queue);

    // Populate the FUSE tree from Cozy
    info!("Fetching file tree from Cozy...");
    let entries = cozy.list_recursive(ROOT_DIR_ID).await?;

    let mut path_to_inode: HashMap<String, u64> = HashMap::new();
    path_to_inode.insert("/".to_string(), ROOT_INODE);

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

    // Start realtime WebSocket (primary) + fallback polling
    let rt_shared = shared.clone();
    let rt_cozy = cozy.clone();
    let rt_url = args.url.clone();
    let rt_token = args.token.clone();
    let rt_cookie = args.cookie.clone();
    tokio::spawn(async move {
        realtime_loop(rt_shared, rt_cozy, rt_url, rt_token, rt_cookie).await;
    });

    // Fallback polling (longer interval since realtime handles most events)
    tokio::spawn(sync_loop(shared, cozy.clone(), sync_interval));

    let mount_handle = mount_fuse(fs, mount.to_str().unwrap()).await?;

    info!("FUSE mounted. Use `fusermount3 -u {:?}` to unmount.", mount);
    mount_handle.await?;

    Ok(())
}
