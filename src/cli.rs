use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
pub struct Cli {
    #[clap(subcommand)]
    pub subcmd: SubCommands,
}
#[derive(Debug, Subcommand)]
pub enum SubCommands {
    /// list all current database
    List(ListArgs),
    /// download from a url, skip it if it's already in database
    Download(DownloadArgs),
    /// scan the filesystem, download all missing files from database
    Fix(FixArgs),
    /// translate the JSON file to my JSON file.
    Translate(TranslateArgs),
}
#[derive(Args, Debug)]
pub struct DownloadArgs {
    #[clap(short, long)]
    pub cookie_file: Option<PathBuf>,
    #[clap(short, long)]
    pub data_path: Option<PathBuf>,
    pub url: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    #[clap(short, long)]
    pub data_path: PathBuf,
}

#[derive(Args, Debug)]
pub struct FixArgs {
    #[clap(short, long)]
    pub cookie_file: Option<PathBuf>,
    #[clap(short, long)]
    pub data_path: PathBuf,
}

#[derive(Args, Debug)]
pub struct TranslateArgs {
    #[clap(short, long)]
    /// the input json from chrome
    pub input: Option<String>,
    #[clap(short, long)]
    /// the input file of json
    pub file: Option<PathBuf>,
}
