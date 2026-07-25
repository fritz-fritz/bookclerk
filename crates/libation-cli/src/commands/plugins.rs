//! `libation plugins` — list external plugins declared in config.toml.

use clap::Subcommand;
use libation_config::Config;

#[derive(Debug, Subcommand)]
pub enum PluginsCommand {
    /// List plugins declared via `command` in config.toml.
    List,
}

pub async fn run(command: PluginsCommand, config: &Config) -> anyhow::Result<()> {
    match command {
        PluginsCommand::List => {
            let plugins = libation_plugin::discover_plugins(config)?;
            if plugins.is_empty() {
                println!("no external plugins in config (set `command` under [sources.*] or [integrations.*])");
                return Ok(());
            }
            for p in plugins {
                let enabled = match p.kind {
                    libation_plugin::PluginKind::Source => config.sources.is_enabled(&p.id),
                    libation_plugin::PluginKind::Integration => {
                        config.integrations.is_enabled(&p.id)
                    }
                    libation_plugin::PluginKind::Output => false,
                };
                println!(
                    "{} kind={} enabled={} command={}",
                    p.id,
                    p.kind.as_str(),
                    enabled,
                    p.command.display()
                );
            }
            Ok(())
        }
    }
}
