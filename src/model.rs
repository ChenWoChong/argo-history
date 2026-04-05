use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    App,
    AppSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectSummary {
    pub namespace: String,
    pub namespace_key: String,
    pub name: String,
    pub source_label: String,
    pub version_count: usize,
    pub latest_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotMeta {
    pub file_name: String,
    pub operation: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectHistory {
    pub resource: String,
    pub namespace: String,
    pub namespace_key: String,
    pub name: String,
    pub source_label: String,
    pub versions: Vec<SnapshotMeta>,
}

impl ResourceKind {
    pub fn from_route(route: &str) -> Option<Self> {
        match route {
            "apps" => Some(Self::App),
            "appsets" => Some(Self::AppSet),
            _ => None,
        }
    }

    pub fn route(self) -> &'static str {
        match self {
            Self::App => "apps",
            Self::AppSet => "appsets",
        }
    }

    pub fn filename_prefix(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::AppSet => "appset",
        }
    }
}

impl Operation {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "CREATE" => Some(Self::Create),
            "UPDATE" => Some(Self::Update),
            "DELETE" => Some(Self::Delete),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

impl Display for Operation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn namespace_key(namespace: &str) -> String {
    if namespace.is_empty() {
        "_cluster".to_string()
    } else {
        namespace.to_string()
    }
}

pub fn namespace_display(namespace: &str) -> String {
    if namespace.is_empty() {
        "cluster-scoped".to_string()
    } else {
        namespace.to_string()
    }
}

pub fn display_timestamp(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}
