use std::path::PathBuf;
use std::sync::Arc;
use clap::Parser;
use tracing::info;

use twake_sync::db::repository::FileRepository;
use twake_sync::services::hydration::HydrationService;
use twake_sync::vfs::InMemoryVfs;
use twake_sync::vfs::vfs_trait::{FileMetadata, VfsBackend};

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "~/TwakeSync")]
    mount: PathBuf,

    #[arg(short, long, default_value = "sqlite:twake.db")]
    database: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::fmt::init();

    let vfs = Arc::new(InMemoryVfs::new());
    let repo = FileRepository::new(&args.database).await?;
    let _hydration = HydrationService::new(vfs.clone(), repo);

    vfs.create_placeholder(
        PathBuf::from("~/TwakeSync/test.txt").as_path(),
        FileMetadata {
            size: 1024,
            modified: String::from("2026-01-01T00:00:00Z"),
            is_dir: false,
        },
    ).await?;

    info!("FUSE mounted at {:?}", args.mount);

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
