use clap::Parser;
use lodestar_cli::{Cli, Client};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let server = match lodestar_cli::resolve_server(cli.server) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("lodestar: {error:#}");
            std::process::exit(2);
        }
    };
    let client = Client::new(&server);
    let code = match lodestar_cli::run(&client, cli.command, &mut std::io::stdout()).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("lodestar: {error:#}");
            2
        }
    };
    std::process::exit(code);
}
