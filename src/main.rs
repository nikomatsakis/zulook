mod config;
mod dispatch;
mod proxy;
mod url_parser;
mod zulip;

use config::CredentialStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let store = CredentialStore::load()?;

    // `mcp` subcommand: run as MCP server
    if args.first().is_some_and(|a| a == "mcp") {
        return proxy::run_mcp(store).await;
    }

    // `acp` subcommand: run as ACP proxy
    if args.first().is_some_and(|a| a == "acp") {
        return proxy::run_acp(store).await;
    }

    // CLI mode: join args into a command string and dispatch
    let command = if args.is_empty() {
        "help".to_string()
    } else {
        args.join(" ")
    };
    let output = dispatch::dispatch(&store, &command).await?;
    println!("{output}");
    Ok(())
}
