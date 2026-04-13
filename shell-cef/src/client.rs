use std::path::PathBuf;

use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Parser, Debug)]
#[command(name = "twake-get", version, about = "Fetch a URL via Shell-CEF")]
struct Cli {
    url: String,

    #[arg(env = "TWAKE_SHELL_CEF_SOCK", default_value = "/tmp/twake-shell-cef.sock", hide = true)]
    sock: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let sock_path = cli.sock.unwrap_or_else(|| PathBuf::from("/tmp/twake-shell-cef.sock"));

    let mut stream = UnixStream::connect(&sock_path).await?;

    let request = serde_json::json!({
        "action": "navigate",
        "params": {
            "url": cli.url,
            "timeout_ms": 30000
        }
    });

    stream.write_all(request.to_string().as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let (reader, _) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;

    let response: serde_json::Value = serde_json::from_str(&line)?;

    if response["status"] == "ok" {
        println!("{}", response["data"]["body"]);
        Ok(())
    } else {
        eprintln!("Error: {} - {}", response["error"]["code"], response["error"]["message"]);
        std::process::exit(1);
    }
}
