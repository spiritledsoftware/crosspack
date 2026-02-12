use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crosspack_installer::{default_user_prefix, PrefixLayout};
use crosspack_registry::RegistryIndex;

#[derive(Parser, Debug)]
#[command(name = "crosspack")]
#[command(about = "Native cross-platform package manager", long_about = None)]
struct Cli {
    #[arg(long)]
    registry_root: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Search { query: String },
    Info { name: String },
    Install { spec: String },
    Upgrade { spec: Option<String> },
    Uninstall { name: String },
    List,
    Pin { spec: String },
    Doctor,
    InitShell,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Search { query } => {
            let root = cli.registry_root.unwrap_or_else(|| PathBuf::from("."));
            let index = RegistryIndex::open(root);
            for name in index.search_names(&query)? {
                println!("{name}");
            }
        }
        Commands::Info { name } => {
            let root = cli.registry_root.unwrap_or_else(|| PathBuf::from("."));
            let index = RegistryIndex::open(root);
            let versions = index.package_versions(&name)?;

            if versions.is_empty() {
                println!("No package found: {name}");
            } else {
                println!("Package: {name}");
                for manifest in versions {
                    println!("- {}", manifest.version);
                }
            }
        }
        Commands::Install { spec } => {
            println!("install is scaffolded, not implemented yet: {spec}");
        }
        Commands::Upgrade { spec } => {
            println!("upgrade is scaffolded, not implemented yet: {spec:?}");
        }
        Commands::Uninstall { name } => {
            println!("uninstall is scaffolded, not implemented yet: {name}");
        }
        Commands::List => {
            println!("list is scaffolded, not implemented yet");
        }
        Commands::Pin { spec } => {
            println!("pin is scaffolded, not implemented yet: {spec}");
        }
        Commands::Doctor => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            println!("prefix: {}", layout.prefix().display());
            println!("bin: {}", layout.bin_dir().display());
            println!("cache: {}", layout.cache_dir().display());
        }
        Commands::InitShell => {
            let prefix = default_user_prefix()?;
            let bin = PrefixLayout::new(prefix).bin_dir();
            if cfg!(windows) {
                println!("setx PATH \"%PATH%;{}\"", bin.display());
            } else {
                println!("export PATH=\"{}:$PATH\"", bin.display());
            }
        }
    }

    Ok(())
}
