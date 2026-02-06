mod config;
mod dispatch;
mod proxy;
mod url_parser;
mod zulip;

use zulip::ZulipClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Commands that don't need credentials
    if args.is_empty()
        || args
            .first()
            .is_some_and(|a| a == "help" || a == "--help" || a == "-h" || a == "setup")
    {
        let command = args.join(" ");
        let cmd = if command.is_empty() { "help" } else { &command };
        let dummy = ZulipClient::new(String::new(), String::new(), String::new());
        let output = dispatch::dispatch(&dummy, cmd).await?;
        println!("{output}");
        return Ok(());
    }

    // Everything else needs credentials
    let creds = config::load_credentials()?;
    let client = ZulipClient::new(creds.url, creds.email, creds.api_key);

    // `mcp` subcommand: run as MCP server
    if args.first().is_some_and(|a| a == "mcp") {
        return proxy::run_mcp(client).await;
    }

    // `acp` subcommand: run as ACP proxy
    if args.first().is_some_and(|a| a == "acp") {
        return proxy::run_acp(client).await;
    }

    // CLI mode: join args into a command string and dispatch
    let command = args.join(" ");
    let output = dispatch::dispatch(&client, &command).await?;
    println!("{output}");
    Ok(())
}
