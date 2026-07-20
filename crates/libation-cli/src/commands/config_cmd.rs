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
            println!("storage.s3.prefix = {}", config.storage.s3.prefix);
            println!("storage.s3.region = {}", config.storage.s3.region);
            println!(
                "storage.s3.endpoint = {}",
                config
                    .storage
                    .s3
                    .endpoint
                    .as_deref()
                    .unwrap_or("-")
            );
            println!(
                "storage.s3.force_path_style = {}",
                config.storage.s3.force_path_style
            );
            println!("download.quality = {:?}", config.download.quality);
            println!("download.format = {:?}", config.download.format);
            println!("download.widevine = {}", config.download.widevine);
            println!("download.xhe_aac = {}", config.download.xhe_aac);
            println!(
                "download.widevine_cdm = {}",
                config
                    .download
                    .widevine_cdm
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "-".into())
            );
            println!(
                "download.folder_template = {}",
                config
                    .download
                    .folder_template
                    .as_deref()
                    .unwrap_or("-")
            );
            println!(
                "download.file_template = {}",
                config.download.file_template.as_deref().unwrap_or("-")
            );
            println!("download.download_cover = {}", config.download.download_cover);
            println!("download.download_pdf = {}", config.download.download_pdf);
            println!("download.create_cue = {}", config.download.create_cue);
            println!("download.fixup_metadata = {}", config.download.fixup_metadata);
            println!(
                "download.save_chapter_json = {}",
                config.download.save_chapter_json
            );
            println!("download.cover_size = {}", config.download.cover_size);
            println!("download.chapter_layout = {}", config.download.chapter_layout);
            println!("library.auto_liberate = {}", config.library.auto_liberate);
            println!(
                "library.scan_interval_minutes = {}",
                config.library.scan_interval_minutes
            );
            println!("daemon.listen = {}", config.daemon.listen);
            println!("daemon.json_logs = {}", config.daemon.json_logs);
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
        "storage.s3.endpoint" => config
            .storage
            .s3
            .endpoint
            .clone()
            .unwrap_or_default(),
        "storage.s3.force_path_style" => config.storage.s3.force_path_style.to_string(),
        "download.quality" => format!("{:?}", config.download.quality).to_ascii_lowercase(),
        "download.format" => format!("{:?}", config.download.format).to_ascii_lowercase(),
        "download.widevine" => config.download.widevine.to_string(),
        "download.xhe_aac" => config.download.xhe_aac.to_string(),
        "download.widevine_cdm" => config
            .download
            .widevine_cdm
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        "download.folder_template" => config
            .download
            .folder_template
            .clone()
            .unwrap_or_default(),
        "download.file_template" => config.download.file_template.clone().unwrap_or_default(),
        "download.download_cover" => config.download.download_cover.to_string(),
        "download.download_pdf" => config.download.download_pdf.to_string(),
        "download.create_cue" => config.download.create_cue.to_string(),
        "download.fixup_metadata" => config.download.fixup_metadata.to_string(),
        "download.save_chapter_json" => config.download.save_chapter_json.to_string(),
        "download.cover_size" => config.download.cover_size.clone(),
        "download.chapter_layout" => config.download.chapter_layout.clone(),
        "library.auto_liberate" => config.library.auto_liberate.to_string(),
        "library.scan_interval_minutes" => config.library.scan_interval_minutes.to_string(),
        "daemon.listen" => config.daemon.listen.clone(),
        "daemon.json_logs" => config.daemon.json_logs.to_string(),
        "paths.files_dir" => paths?.files_dir.display().to_string(),
        "paths.config_file" => paths?.config_file.display().to_string(),
        "paths.library_db" => paths?.library_db.display().to_string(),
        "paths.cache_dir" => paths?.cache_dir.display().to_string(),
        "paths.log_dir" => paths?.log_dir.display().to_string(),
        _ => return None,
    })
}
