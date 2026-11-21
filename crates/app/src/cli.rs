use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SourceArg {
    Game,
    System,
}

#[derive(Debug, Parser)]
#[command(
    name = "scpsl-subs",
    version,
    about = "Запись звука SCP: SL для корпуса субтитров"
)]
pub struct Args {
    #[arg(long, value_name = "PATH")]
    pub record: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = SourceArg::Game)]
    pub source: SourceArg,

    #[arg(long, default_value = "SCPSL.exe")]
    pub game_process: String,

    #[arg(
        long,
        value_name = "SECS",
        help = "Остановить запись через указанное число секунд"
    )]
    pub duration: Option<u64>,

    #[arg(long)]
    pub debug: bool,
}
