use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{Map, Value};

use crate::model::{
    ObjectHistory, ObjectSummary, Operation, ResourceKind, SnapshotMeta, display_timestamp,
    namespace_display, namespace_key,
};

#[derive(Clone)]
pub struct HistoryStore {
    root: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SaveResult {
    pub written: bool,
    pub file_name: String,
}

impl HistoryStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
        Ok(Self {
            root: Arc::new(root),
        })
    }

    pub fn save_snapshot(
        &self,
        kind: ResourceKind,
        operation: Operation,
        raw_manifest: &Value,
    ) -> Result<SaveResult> {
        let sanitized = sanitize_manifest(raw_manifest)?;
        let namespace = sanitized
            .pointer("/metadata/namespace")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let name = sanitized
            .pointer("/metadata/name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("metadata.name is required"))?
            .to_string();

        let dir = self.object_dir(kind, &namespace, &name);
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

        let yaml = serde_yaml::to_string(&sanitized).with_context(|| "serialize yaml")?;
        let file_name = build_file_name(kind, operation, Utc::now());

        if let Some((latest_file, latest_content)) = self.latest_file_and_content(&dir)? {
            let latest_operation = parse_file_name(kind, &latest_file)?.operation;
            if latest_operation == operation.as_str() && latest_content == yaml {
                return Ok(SaveResult {
                    written: false,
                    file_name: latest_file,
                });
            }
        }

        let path = dir.join(&file_name);
        fs::write(&path, yaml).with_context(|| format!("write {}", path.display()))?;

        Ok(SaveResult {
            written: true,
            file_name,
        })
    }

    pub fn list_objects(&self, kind: ResourceKind) -> Result<Vec<ObjectSummary>> {
        let root = self.root.join(kind.route());
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut objects = Vec::new();
        for namespace_entry in
            fs::read_dir(&root).with_context(|| format!("read {}", root.display()))?
        {
            let namespace_entry = namespace_entry?;
            if !namespace_entry.file_type()?.is_dir() {
                continue;
            }
            let namespace_key_value = namespace_entry.file_name().to_string_lossy().to_string();
            let namespace = if namespace_key_value == "_cluster" {
                String::new()
            } else {
                namespace_key_value.clone()
            };
            for name_entry in fs::read_dir(namespace_entry.path())? {
                let name_entry = name_entry?;
                if !name_entry.file_type()?.is_dir() {
                    continue;
                }
                let versions = self.list_versions(
                    kind,
                    &namespace,
                    &name_entry.file_name().to_string_lossy(),
                )?;
                let latest_timestamp = versions.first().map(|item| item.timestamp.clone());
                objects.push(ObjectSummary {
                    namespace: namespace_display(&namespace),
                    namespace_key: namespace_key(&namespace),
                    name: name_entry.file_name().to_string_lossy().to_string(),
                    version_count: versions.len(),
                    latest_timestamp,
                });
            }
        }

        objects.sort_by(|a, b| a.name.cmp(&b.name).then(a.namespace.cmp(&b.namespace)));
        Ok(objects)
    }

    pub fn get_history(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
    ) -> Result<ObjectHistory> {
        Ok(ObjectHistory {
            resource: kind.route().to_string(),
            namespace: namespace_display(namespace),
            namespace_key: namespace_key(namespace),
            name: name.to_string(),
            versions: self.list_versions(kind, namespace, name)?,
        })
    }

    pub fn list_versions(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
    ) -> Result<Vec<SnapshotMeta>> {
        let dir = self.object_dir(kind, namespace, name);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut versions = Vec::new();
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            let parsed = parse_file_name(kind, &file_name)?;
            versions.push(SnapshotMeta {
                file_name,
                operation: parsed.operation.to_uppercase(),
                timestamp: display_timestamp(parsed.timestamp),
            });
        }

        versions.sort_by(|a, b| b.file_name.cmp(&a.file_name));
        Ok(versions)
    }

    pub fn read_snapshot(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
        file_name: &str,
    ) -> Result<String> {
        validate_file_name(file_name)?;
        let path = self.object_dir(kind, namespace, name).join(file_name);
        let content =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        Ok(content)
    }

    fn latest_file_and_content(&self, dir: &Path) -> Result<Option<(String, String)>> {
        let mut file_names = fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        file_names.sort();
        if let Some(file_name) = file_names.pop() {
            let content = fs::read_to_string(dir.join(&file_name))?;
            return Ok(Some((file_name, content)));
        }
        Ok(None)
    }

    fn object_dir(&self, kind: ResourceKind, namespace: &str, name: &str) -> PathBuf {
        self.root
            .join(kind.route())
            .join(namespace_key(namespace))
            .join(name)
    }
}

#[derive(Debug)]
struct ParsedFileName {
    operation: String,
    timestamp: DateTime<Utc>,
}

fn validate_file_name(file_name: &str) -> Result<()> {
    if file_name.contains('/') || file_name.contains("..") {
        bail!("invalid file name");
    }
    Ok(())
}

fn build_file_name(kind: ResourceKind, operation: Operation, now: DateTime<Utc>) -> String {
    let timestamp = format!(
        "{}{:03}Z",
        now.format("%Y%m%dT%H%M%S"),
        now.timestamp_subsec_millis()
    );
    format!(
        "{}-{}-{}.yaml",
        kind.filename_prefix(),
        operation,
        timestamp
    )
}

fn parse_file_name(kind: ResourceKind, file_name: &str) -> Result<ParsedFileName> {
    validate_file_name(file_name)?;
    let base = file_name.trim_end_matches(".yaml");
    let prefix = format!("{}-", kind.filename_prefix());
    let suffix = base
        .strip_prefix(&prefix)
        .ok_or_else(|| anyhow!("invalid snapshot file {}", file_name))?;
    let (operation, timestamp) = suffix
        .split_once('-')
        .ok_or_else(|| anyhow!("invalid snapshot file {}", file_name))?;
    let timestamp = parse_timestamp(timestamp)?;
    Ok(ParsedFileName {
        operation: operation.to_string(),
        timestamp,
    })
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    if value.len() != 19 || !value.ends_with('Z') {
        bail!("invalid timestamp {}", value);
    }
    let date = &value[..15];
    let millis = &value[15..18];
    let naive =
        NaiveDateTime::parse_from_str(date, "%Y%m%dT%H%M%S").with_context(|| "parse timestamp")?;
    let millis = millis
        .parse::<u32>()
        .with_context(|| "parse milliseconds")?;
    let timestamp = naive.and_utc() + chrono::Duration::milliseconds(i64::from(millis));
    Ok(timestamp)
}

pub fn sanitize_manifest(raw_manifest: &Value) -> Result<Value> {
    let obj = raw_manifest
        .as_object()
        .ok_or_else(|| anyhow!("manifest must be an object"))?;

    let api_version = obj
        .get("apiVersion")
        .cloned()
        .unwrap_or_else(|| Value::String("".to_string()));
    let kind = obj
        .get("kind")
        .cloned()
        .unwrap_or_else(|| Value::String("".to_string()));
    let metadata = obj
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("metadata is required"))?;

    let name = metadata
        .get("name")
        .cloned()
        .ok_or_else(|| anyhow!("metadata.name is required"))?;
    let namespace = metadata.get("namespace").cloned();
    let labels = metadata
        .get("labels")
        .and_then(to_sorted_string_map)
        .map(Value::Object);
    let annotations = metadata
        .get("annotations")
        .and_then(to_sorted_string_map)
        .map(filter_annotations)
        .filter(|map| !map.is_empty())
        .map(Value::Object);

    let mut metadata_out = Map::new();
    metadata_out.insert("name".to_string(), name);
    if let Some(namespace) = namespace {
        metadata_out.insert("namespace".to_string(), namespace);
    }
    if let Some(labels) = labels {
        metadata_out.insert("labels".to_string(), labels);
    }
    if let Some(annotations) = annotations {
        metadata_out.insert("annotations".to_string(), annotations);
    }

    let mut sanitized = Map::new();
    sanitized.insert("apiVersion".to_string(), api_version);
    sanitized.insert("kind".to_string(), kind);
    sanitized.insert("metadata".to_string(), Value::Object(metadata_out));
    if let Some(spec) = obj.get("spec") {
        sanitized.insert("spec".to_string(), spec.clone());
    }
    Ok(Value::Object(sanitized))
}

fn to_sorted_string_map(value: &Value) -> Option<Map<String, Value>> {
    let mut output = BTreeMap::new();
    for (key, value) in value.as_object()? {
        if let Some(text) = value.as_str() {
            output.insert(key.to_string(), Value::String(text.to_string()));
        }
    }
    Some(output.into_iter().collect())
}

fn filter_annotations(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .filter(|(key, _)| {
            !matches!(
                key.as_str(),
                "kubectl.kubernetes.io/last-applied-configuration" | "argocd.argoproj.io/refresh"
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn sanitize_manifest_removes_noise_fields() {
        let raw = json!({
            "apiVersion": "argoproj.io/v1alpha1",
            "kind": "ApplicationSet",
            "metadata": {
                "name": "demo",
                "namespace": "argocd",
                "resourceVersion": "1",
                "uid": "abc",
                "creationTimestamp": "2026-01-01T00:00:00Z",
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{}",
                    "example.com/keep": "yes"
                }
            },
            "spec": {
                "generators": []
            },
            "status": {
                "conditions": []
            }
        });

        let sanitized = sanitize_manifest(&raw).expect("sanitize");
        assert!(sanitized.get("status").is_none());
        assert_eq!(
            sanitized.pointer("/metadata/annotations/example.com~1keep"),
            Some(&Value::String("yes".to_string()))
        );
        assert_eq!(sanitized.pointer("/metadata/resourceVersion"), None);
    }

    #[test]
    fn store_groups_versions_by_object() {
        let dir = tempdir().expect("tempdir");
        let store = HistoryStore::new(dir.path()).expect("store");
        let raw = json!({
            "apiVersion": "argoproj.io/v1alpha1",
            "kind": "Application",
            "metadata": {
                "name": "demo",
                "namespace": "argocd"
            },
            "spec": {
                "project": "default"
            }
        });

        store
            .save_snapshot(ResourceKind::App, Operation::Create, &raw)
            .expect("save");
        let objects = store.list_objects(ResourceKind::App).expect("list");
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].name, "demo");
        let history = store
            .get_history(ResourceKind::App, "argocd", "demo")
            .expect("history");
        assert_eq!(history.versions.len(), 1);
    }
}
