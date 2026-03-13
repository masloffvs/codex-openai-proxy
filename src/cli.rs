use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value = "8080")]
    pub port: u16,
    #[arg(long, default_value = "~/.codex/auth.json")]
    pub auth_path: String,
}
