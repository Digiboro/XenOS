//! XenOS Build System (xtask)
//!
//! Система сборки XenOS на базе cargo xtask pattern.
//! Заменяет Makefile и предоставляет единый интерфейс для всех операций сборки.

mod cli;
mod build;
mod config;
mod generate;
mod image;
mod run;
mod util;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Command::Build(args) => build::execute(args),
        Command::Generate(args) => generate::execute(args),
        Command::Image(args) => image::execute(args),
        Command::Run(args) => run::execute(args),
        Command::Test(args) => run::execute_test(args),
        Command::Clean(args) => build::clean(args),
        Command::Menuconfig => config::menuconfig(),
        Command::Defconfig => config::defconfig(),
    }
}

