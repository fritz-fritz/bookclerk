//! `libation config` — get settings (LibationCli: `get-setting`).

use clap::Subcommand;
use libation_config::Config;

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print a configuration value by dotted key.
    Get {
        /// Dotted key, e.g. `storage.backend`, `download.quality`, `paths.files_dir`.
        key: String,
    },
    /// Print the effective configuration as TOML-ish summary.
    Show,
    /// Print resolved filesystem paths.
    Paths,
}

pub fn run(command: ConfigCommand, config: &Config) -> anyhow::Result<()> {
    match command {
        ConfigCommand::Get { key } => {
            let value =
                lookup(config, &key).ok_or_else(|| anyhow::anyhow!("unknown config key: {key}"))?;
            println!("{value}");
            Ok(())
        }
        ConfigCommand::Show => {
            println!("storage.backend = {:?}", config.storage.backend);
            println!(
                "storage.local.root = {}",
                config.storage.local.root.display()
            );
            println!("storage.s3.bucket = {}", config.storage.s3.bucket);
            println!("download.quality = {:?}", config.download.quality);
            println!("download.format = {:?}", config.download.format);
            println!("download.widevine = {}", config.download.widevine);
            println!("download.xhe_aac = {}", config.download.xhe_aac);
            println!("library.auto_liberate = {}", config.library.auto_liberate);
            println!(
                "library.scan_interval_minutes = {}",
                config.library.scan_interval_minutes
            );
            println!("daemon.listen = {}", config.daemon.listen);
            Ok(())
        }
        ConfigCommand::Paths => {
            let paths = config.paths();
            println!("files_dir\t{}", paths.files_dir.display());
            println!("config_file\t{}", paths.config_file.display());
            println!("library_db\t{}", paths.library_db.display());
            println!("cache_dir\t{}", paths.cache_dir.display());
            println!("log_dir\t{}", paths.log_dir.display());
            Ok(())
        }
    }
}

fn lookup(config: &Config, key: &str) -> Option<String> {
    let paths = config.paths.as_ref();
    Some(match key {
        "storage.backend" => format!("{:?}", config.storage.backend).to_ascii_lowercase(),
        "storage.local.root" => config.storage.local.root.display().to_string(),
        "storage.s3.bucket" => config.storage.s3.bucket.clone(),
        "storage.s3.prefix" => config.storage.s3.prefix.clone(),
        "storage.s3.region" => config.storage.s3.region.clone(),
        "download.quality" => format!("{:?}", config.download.quality).to_ascii_lowercase(),
        "download.format" => format!("{:?}", config.download.format).to_ascii_lowercase(),
        "download.widevine" => config.download.widevine.to_string(),
        "download.xhe_aac" => config.download.xhe_aac.to_string(),
        "library.auto_liberate" => config.library.auto_liberate.to_string(),
        "library.scan_interval_minutes" => config.library.scan_interval_minutes.to_string(),
        "daemon.listen" => config.daemon.listen.clone(),
        "paths.files_dir" => paths?.files_dir.display().to_string(),
        "paths.library_db" => paths?.library_db.display().to_string(),
        "paths.cache_dir" => paths?.cache_dir.display().to_string(),
        _ => return None,
    })
}
