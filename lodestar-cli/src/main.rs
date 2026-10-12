use clap::Parser;
use lodestar_cli::{Cli, Client};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let client = Client::new(&cli.server);
    let code = match lodestar_cli::run(&client, cli.command, &mut std::io::stdout()).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("lodestar: {e:#}");
            2
        }
    };
    std::process::exit(code);
}
