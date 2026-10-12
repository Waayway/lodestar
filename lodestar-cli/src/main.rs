use clap::Parser;
use lodestar_cli::{Cli, Client};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let server = match lodestar_cli::resolve_server(cli.server) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lodestar: {e:#}");
            std::process::exit(2);
        }
    };
    let client = Client::new(&server);
    let code = match lodestar_cli::run(&client, cli.command, &mut std::io::stdout()).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("lodestar: {e:#}");
            2
        }
    };
    std::process::exit(code);
}
