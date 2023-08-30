use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
pub struct Cli {
    #[clap(short, long)]
    pub cookie_file: Option<PathBuf>,
    #[clap(short, long)]
    pub data_path: Option<PathBuf>,
    pub url: Vec<String>,
}
