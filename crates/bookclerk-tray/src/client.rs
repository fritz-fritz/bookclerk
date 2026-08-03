//! Talk to the local `bookclerkd` HTTP API from the tray thread.

use std::path::PathBuf;
use std::time::Duration;

/// Configuration for the in-process tray (no child `bookclerkd` process).
#[derive(Debug, Clone)]
pub struct TrayConfig {
    pub base_url: String,
    pub auth_enabled: bool,
    pub operator_token: Option<String>,
    pub token_path: Option<PathBuf>,
}

impl TrayConfig {
    #[must_use]
    pub fn base_url(listen: &str) -> String {
        let listen = listen.trim().trim_end_matches('/');
        if listen.starts_with("http://") || listen.starts_with("https://") {
            listen.to_string()
        } else {
            format!("http://{listen}")
        }
    }

    #[must_use]
    pub fn ui_url(&self) -> String {
        if self.auth_enabled {
            if let Some(token) = self.operator_token.as_deref().filter(|t| !t.is_empty()) {
                return format!("{}/#token={token}", self.base_url.trim_end_matches('/'));
            }
        }
        format!("{}/", self.base_url.trim_end_matches('/'))
    }

    pub fn open_ui(&self) -> anyhow::Result<()> {
        open::that(self.ui_url())?;
        Ok(())
    }

    pub fn trigger_scan(&self) -> anyhow::Result<()> {
        let url = format!("{}/api/library/scan", self.base_url.trim_end_matches('/'));
        let mut req = ureq::post(&url)
            .header("Content-Type", "application/json")
            .config()
            .timeout_global(Some(Duration::from_secs(10)))
            .build();
        if self.auth_enabled {
            if let Some(token) = self.operator_token.as_deref() {
                req = req.header("Authorization", format!("Bearer {token}"));
            }
        }
        req.send("{}")?;
        Ok(())
    }

    pub fn print_operator_token(&self) {
        if !self.auth_enabled {
            eprintln!("bookclerk: operator auth is disabled");
            return;
        }
        match self.operator_token.as_deref() {
            Some(token) if !token.is_empty() => {
                if let Some(path) = &self.token_path {
                    eprintln!(
                        "bookclerk: operator token (file {}):\n{token}",
                        path.display()
                    );
                } else {
                    eprintln!("bookclerk: operator token:\n{token}");
                }
            }
            _ => eprintln!("bookclerk: no operator token available"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TrayConfig;

    #[test]
    fn base_url_normalizes() {
        assert_eq!(
            TrayConfig::base_url("127.0.0.1:8787"),
            "http://127.0.0.1:8787"
        );
        assert_eq!(
            TrayConfig::base_url("http://127.0.0.1:8787/"),
            "http://127.0.0.1:8787"
        );
    }

    #[test]
    fn ui_url_embeds_token_fragment() {
        let cfg = TrayConfig {
            base_url: "http://127.0.0.1:8787".into(),
            auth_enabled: true,
            operator_token: Some("abc".into()),
            token_path: None,
        };
        assert_eq!(cfg.ui_url(), "http://127.0.0.1:8787/#token=abc");
    }
}
