use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub history: HistorySettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    #[serde(default = "default_http_bind_addr")]
    pub http_bind_addr: String,
    #[serde(default = "default_webhook_bind_addr")]
    pub webhook_bind_addr: String,
    #[serde(default)]
    pub tls: TlsSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TlsSettings {
    #[serde(default = "default_cert_path")]
    pub cert_path: String,
    #[serde(default = "default_key_path")]
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistorySettings {
    #[serde(default = "default_history_root_dir")]
    pub root_dir: String,
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("read config file {}", path.display()))?;
        let settings =
            serde_yaml::from_str::<Self>(&content).with_context(|| "parse config yaml")?;
        Ok(settings)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerSettings::default(),
            history: HistorySettings::default(),
        }
    }
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            http_bind_addr: default_http_bind_addr(),
            webhook_bind_addr: default_webhook_bind_addr(),
            tls: TlsSettings::default(),
        }
    }
}

impl Default for TlsSettings {
    fn default() -> Self {
        Self {
            cert_path: default_cert_path(),
            key_path: default_key_path(),
        }
    }
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            root_dir: default_history_root_dir(),
            retention_days: default_retention_days(),
        }
    }
}

fn default_http_bind_addr() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_webhook_bind_addr() -> String {
    "0.0.0.0:9443".to_string()
}

fn default_cert_path() -> String {
    "/certs/tls.crt".to_string()
}

fn default_key_path() -> String {
    "/certs/tls.key".to_string()
}

fn default_history_root_dir() -> String {
    "/var/lib/argo-history".to_string()
}

fn default_retention_days() -> u64 {
    14
}
