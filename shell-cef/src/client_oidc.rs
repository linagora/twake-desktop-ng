use std::path::PathBuf;

use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Parser, Debug)]
#[command(name = "twake-auth", version, about = "Authenticate via OIDC using Shell-CEF")]
struct Cli {
    #[arg(env = "TWAKE_SHELL_CEF_SOCK", default_value = "/tmp/twake-shell-cef.sock", hide = true)]
    sock: Option<PathBuf>,

    #[arg(long, env = "ISSUER", default_value = "https://sso.linagora.com")]
    issuer: String,

    #[arg(long, env = "CLIENT_ID", default_value = "tcalendar")]
    client_id: String,

    #[arg(long, env = "REDIRECT_URI", default_value = "http://localhost:5000/callback")]
    redirect_uri: String,

    #[arg(long, env = "PKCE", default_value = "true")]
    pkce: bool,

    #[arg(long, env = "SCOPES", default_value = "openid profile email")]
    scopes: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let sock_path = cli.sock.unwrap_or_else(|| PathBuf::from("/tmp/twake-shell-cef.sock"));

    let scopes: Vec<String> = cli.scopes.split_whitespace().map(String::from).collect();

    println!("Starting OIDC authentication flow...");
    println!("  Issuer: {}", cli.issuer);
    println!("  Client ID: {}", cli.client_id);
    println!("  Redirect URI: {}", cli.redirect_uri);
    println!("  PKCE: {}", cli.pkce);
    println!("  Scopes: {:?}", scopes);
    println!();
    println!("A browser will open for authentication.");
    println!("Please complete the login, then close the browser window.");

    let mut stream = UnixStream::connect(&sock_path).await?;

    let request = serde_json::json!({
        "action": "auth.oidc_start",
        "params": {
            "issuer": cli.issuer,
            "client_id": cli.client_id,
            "redirect_uri": cli.redirect_uri,
            "pkce": cli.pkce,
            "scopes": scopes
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
        println!();
        println!("Authentication successful!");
        println!();
        let data = &response["data"];
        if let Some(token) = data.get("access_token").and_then(|t| t.as_str()) {
            println!("Access Token: {}", token);
        }
        if let Some(token) = data.get("refresh_token").and_then(|t| t.as_str()) {
            println!("Refresh Token: {}", token);
        }
        if let Some(token) = data.get("id_token").and_then(|t| t.as_str()) {
            println!("ID Token: {}", token);
        }
        if let Some(exp) = data.get("expires_in").and_then(|e| e.as_i64()) {
            println!("Expires in: {} seconds", exp);
        }
        Ok(())
    } else {
        eprintln!("Error: {} - {}", response["error"]["code"], response["error"]["message"]);
        std::process::exit(1);
    }
}
