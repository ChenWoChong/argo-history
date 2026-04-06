use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde_json::{Map, Value};
use tokio::task;

use crate::model::{
    ObjectHistory, ObjectOverview, ObjectSummary, Operation, ResourceKind, SnapshotMeta,
    display_timestamp, namespace_display, namespace_key,
};

#[derive(Clone)]
pub struct HistoryStore {
    root: Arc<PathBuf>,
    retention_days: u64,
    now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

#[derive(Debug, Clone)]
pub struct SaveResult {
    pub written: bool,
    pub file_name: String,
}

#[derive(Debug, Clone)]
struct ParsedFileName {
    operation: String,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct VersionEntry {
    meta: SnapshotMeta,
    timestamp: DateTime<Utc>,
    manifest: Value,
}

impl HistoryStore {
    pub fn new(root: impl Into<PathBuf>, retention_days: u64) -> Result<Self> {
        Self::with_clock(root.into(), retention_days, Arc::new(Utc::now))
    }

    fn with_clock(
        root: PathBuf,
        retention_days: u64,
        now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Result<Self> {
        fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
        Ok(Self {
            root: Arc::new(root),
            retention_days,
            now,
        })
    }

    pub async fn save_snapshot(
        &self,
        kind: ResourceKind,
        operation: Operation,
        raw_manifest: &Value,
    ) -> Result<SaveResult> {
        let store = self.clone();
        let manifest = raw_manifest.clone();
        task::spawn_blocking(move || store.save_snapshot_sync(kind, operation, &manifest))
            .await
            .map_err(|error| anyhow!("join save snapshot task: {error}"))?
    }

    pub async fn list_objects(&self, kind: ResourceKind) -> Result<Vec<ObjectSummary>> {
        let store = self.clone();
        task::spawn_blocking(move || store.list_objects_sync(kind))
            .await
            .map_err(|error| anyhow!("join list objects task: {error}"))?
    }

    pub async fn get_history(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
    ) -> Result<ObjectHistory> {
        let store = self.clone();
        let namespace = namespace.to_string();
        let name = name.to_string();
        task::spawn_blocking(move || store.get_history_sync(kind, &namespace, &name))
            .await
            .map_err(|error| anyhow!("join history task: {error}"))?
    }

    pub async fn read_snapshot(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
        file_name: &str,
    ) -> Result<String> {
        let store = self.clone();
        let namespace = namespace.to_string();
        let name = name.to_string();
        let file_name = file_name.to_string();
        task::spawn_blocking(move || store.read_snapshot_sync(kind, &namespace, &name, &file_name))
            .await
            .map_err(|error| anyhow!("join read snapshot task: {error}"))?
    }

    pub fn retention_days(&self) -> u64 {
        self.retention_days
    }

    fn save_snapshot_sync(
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
        let now = (self.now)();
        self.prune_old_snapshots_sync(&dir, now)?;

        if let Some((latest_file, latest_content)) = self.latest_file_and_content(&dir)? {
            let latest_operation = parse_file_name(kind, &latest_file)?.operation;
            if latest_operation == operation.as_str() && latest_content == yaml {
                return Ok(SaveResult {
                    written: false,
                    file_name: latest_file,
                });
            }
        }

        let file_name = build_file_name(kind, operation, now);
        let path = dir.join(&file_name);
        fs::write(&path, yaml).with_context(|| format!("write {}", path.display()))?;

        Ok(SaveResult {
            written: true,
            file_name,
        })
    }

    fn list_objects_sync(&self, kind: ResourceKind) -> Result<Vec<ObjectSummary>> {
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
                let name = name_entry.file_name().to_string_lossy().to_string();
                let versions = self.list_versions_sync(kind, &namespace, &name)?;
                let latest_timestamp = versions.first().map(|item| item.meta.timestamp.clone());
                let source_label = self.latest_source_label(kind, &namespace, &name, &versions)?;
                objects.push(ObjectSummary {
                    namespace: namespace_display(&namespace),
                    namespace_key: namespace_key(&namespace),
                    name,
                    source_label,
                    version_count: versions.len(),
                    latest_timestamp,
                });
            }
        }

        objects.sort_by(|a, b| a.name.cmp(&b.name).then(a.namespace.cmp(&b.namespace)));
        Ok(objects)
    }

    fn get_history_sync(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
    ) -> Result<ObjectHistory> {
        let mut versions = self.list_versions_sync(kind, namespace, name)?;
        let source_label = self.latest_source_label(kind, namespace, name, &versions)?;
        let overview = build_object_overview(&versions, &source_label, self.retention_days);
        enrich_version_summaries(&mut versions);

        Ok(ObjectHistory {
            resource: kind.route().to_string(),
            namespace: namespace_display(namespace),
            namespace_key: namespace_key(namespace),
            name: name.to_string(),
            source_label,
            overview,
            versions: versions.into_iter().map(|item| item.meta).collect(),
        })
    }

    fn list_versions_sync(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
    ) -> Result<Vec<VersionEntry>> {
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
            let content = fs::read_to_string(entry.path())
                .with_context(|| format!("read {}", entry.path().display()))?;
            let manifest =
                serde_yaml::from_str::<Value>(&content).with_context(|| "parse snapshot yaml")?;
            versions.push(VersionEntry {
                meta: SnapshotMeta {
                    file_name,
                    operation: parsed.operation.to_uppercase(),
                    timestamp: display_timestamp(parsed.timestamp),
                    timestamp_rfc3339: parsed.timestamp.to_rfc3339(),
                    summary: String::new(),
                },
                timestamp: parsed.timestamp,
                manifest,
            });
        }

        versions.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| b.meta.file_name.cmp(&a.meta.file_name))
        });
        Ok(versions)
    }

    fn read_snapshot_sync(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
        file_name: &str,
    ) -> Result<String> {
        validate_file_name(file_name)?;
        let path = self.object_dir(kind, namespace, name).join(file_name);
        fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
    }

    fn latest_file_and_content(&self, dir: &Path) -> Result<Option<(String, String)>> {
        let mut newest: Option<(String, DateTime<Utc>)> = None;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            let timestamp = parse_timestamp_from_any_snapshot(&file_name)?;
            if newest
                .as_ref()
                .map(|(_, current)| timestamp > *current)
                .unwrap_or(true)
            {
                newest = Some((file_name, timestamp));
            }
        }

        if let Some((file_name, _)) = newest {
            let content = fs::read_to_string(dir.join(&file_name))?;
            return Ok(Some((file_name, content)));
        }
        Ok(None)
    }

    fn prune_old_snapshots_sync(&self, dir: &Path, now: DateTime<Utc>) -> Result<()> {
        if self.retention_days == 0 || !dir.exists() {
            return Ok(());
        }
        let cutoff = now - Duration::days(self.retention_days as i64);
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            let timestamp = parse_timestamp_from_any_snapshot(&file_name)?;
            if timestamp < cutoff {
                fs::remove_file(entry.path()).with_context(|| {
                    format!("remove expired snapshot {}", entry.path().display())
                })?;
            }
        }
        Ok(())
    }

    fn latest_source_label(
        &self,
        kind: ResourceKind,
        namespace: &str,
        name: &str,
        versions: &[VersionEntry],
    ) -> Result<String> {
        if kind == ResourceKind::AppSet {
            return Ok("ApplicationSet".to_string());
        }
        let Some(version) = versions.first() else {
            return Ok("Unknown".to_string());
        };
        let raw = self.read_snapshot_sync(kind, namespace, name, &version.meta.file_name)?;
        let manifest =
            serde_yaml::from_str::<Value>(&raw).with_context(|| "parse snapshot yaml")?;
        Ok(infer_source_label(kind, &manifest))
    }

    fn object_dir(&self, kind: ResourceKind, namespace: &str, name: &str) -> PathBuf {
        self.root
            .join(kind.route())
            .join(namespace_key(namespace))
            .join(name)
    }
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
    Ok(ParsedFileName {
        operation: operation.to_string(),
        timestamp: parse_timestamp(timestamp)?,
    })
}

fn parse_timestamp_from_any_snapshot(file_name: &str) -> Result<DateTime<Utc>> {
    validate_file_name(file_name)?;
    let base = file_name.trim_end_matches(".yaml");
    let timestamp = base
        .rsplit_once('-')
        .map(|(_, value)| value)
        .ok_or_else(|| anyhow!("invalid snapshot file {}", file_name))?;
    parse_timestamp(timestamp)
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
    Ok(naive.and_utc() + Duration::milliseconds(i64::from(millis)))
}

fn build_object_overview(
    versions: &[VersionEntry],
    source_label: &str,
    retention_days: u64,
) -> ObjectOverview {
    let latest = versions.first().map(|item| &item.meta);
    let oldest = versions.last().map(|item| &item.meta);
    ObjectOverview {
        first_backup_at: oldest
            .map(|item| item.timestamp.clone())
            .unwrap_or_else(|| "-".to_string()),
        latest_backup_at: latest
            .map(|item| item.timestamp.clone())
            .unwrap_or_else(|| "-".to_string()),
        total_versions: versions.len(),
        latest_operation: latest
            .map(|item| item.operation.clone())
            .unwrap_or_else(|| "-".to_string()),
        source_label: source_label.to_string(),
        retention_days,
    }
}

fn enrich_version_summaries(versions: &mut [VersionEntry]) {
    for index in 0..versions.len() {
        let summary = match versions[index].meta.operation.as_str() {
            "CREATE" => "对象首次进入历史备份".to_string(),
            "DELETE" => "对象从集群中删除".to_string(),
            "UPDATE" => {
                if let Some(previous) = versions.get(index + 1) {
                    summarize_manifest_change(&versions[index].manifest, &previous.manifest)
                } else {
                    "对象内容已更新".to_string()
                }
            }
            _ => "对象发生变化".to_string(),
        };
        versions[index].meta.summary = summary;
    }
}

fn summarize_manifest_change(current: &Value, previous: &Value) -> String {
    let current_map = flatten_manifest(current);
    let previous_map = flatten_manifest(previous);

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for key in current_map.keys() {
        if !previous_map.contains_key(key) {
            added.push(key.clone());
        } else if previous_map.get(key) != current_map.get(key) {
            changed.push(key.clone());
        }
    }

    for key in previous_map.keys() {
        if !current_map.contains_key(key) {
            removed.push(key.clone());
        }
    }

    let mut parts = Vec::new();
    if !changed.is_empty() {
        parts.push(format!("修改 {}", summarize_paths(&changed)));
    }
    if !added.is_empty() {
        parts.push(format!("新增 {}", summarize_paths(&added)));
    }
    if !removed.is_empty() {
        parts.push(format!("删除 {}", summarize_paths(&removed)));
    }

    if parts.is_empty() {
        "对象内容已更新".to_string()
    } else {
        parts.join("，")
    }
}

fn summarize_paths(paths: &[String]) -> String {
    let shown = paths.iter().take(2).cloned().collect::<Vec<_>>();
    if paths.len() <= 2 {
        shown.join("、")
    } else {
        format!("{} 等 {} 项", shown.join("、"), paths.len())
    }
}

fn flatten_manifest(value: &Value) -> BTreeMap<String, String> {
    let mut output = BTreeMap::new();
    flatten_manifest_inner(value, "", &mut output);
    output
}

fn flatten_manifest_inner(value: &Value, prefix: &str, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let next = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_manifest_inner(item, &next, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let next = format!("{prefix}[{index}]");
                flatten_manifest_inner(item, &next, out);
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.insert(prefix.to_string(), scalar_value(value));
            }
        }
    }
}

fn scalar_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.to_string(),
        _ => value.to_string(),
    }
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
    let owner_references = metadata
        .get("ownerReferences")
        .and_then(sanitize_owner_references)
        .map(Value::Array);

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
    if let Some(owner_references) = owner_references {
        metadata_out.insert("ownerReferences".to_string(), owner_references);
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

fn infer_source_label(kind: ResourceKind, manifest: &Value) -> String {
    if kind == ResourceKind::AppSet {
        return "ApplicationSet".to_string();
    }

    let owner_refs = manifest
        .pointer("/metadata/ownerReferences")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for owner in owner_refs {
        let owner_kind = owner
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let owner_name = owner
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if owner_kind == "ApplicationSet" && !owner_name.is_empty() {
            return format!("Generated by AppSet/{owner_name}");
        }
    }

    "Direct Application".to_string()
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

fn sanitize_owner_references(value: &Value) -> Option<Vec<Value>> {
    let owners = value.as_array()?;
    let sanitized = owners
        .iter()
        .filter_map(|item| item.as_object())
        .map(|item| {
            let mut out = Map::new();
            if let Some(api_version) = item.get("apiVersion") {
                out.insert("apiVersion".to_string(), api_version.clone());
            }
            if let Some(kind) = item.get("kind") {
                out.insert("kind".to_string(), kind.clone());
            }
            if let Some(name) = item.get("name") {
                out.insert("name".to_string(), name.clone());
            }
            if let Some(controller) = item.get("controller") {
                out.insert("controller".to_string(), controller.clone());
            }
            Value::Object(out)
        })
        .collect::<Vec<_>>();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn fixed_store(root: PathBuf, now: DateTime<Utc>) -> HistoryStore {
        HistoryStore::with_clock(root, 14, Arc::new(move || now)).expect("store")
    }

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
        let now = DateTime::parse_from_rfc3339("2026-04-04T02:26:15Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let store = fixed_store(dir.path().to_path_buf(), now);
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
            .save_snapshot_sync(ResourceKind::App, Operation::Create, &raw)
            .expect("save");
        let objects = store.list_objects_sync(ResourceKind::App).expect("list");
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].name, "demo");
        let history = store
            .get_history_sync(ResourceKind::App, "argocd", "demo")
            .expect("history");
        assert_eq!(history.versions.len(), 1);
    }

    #[test]
    fn versions_are_sorted_by_timestamp_desc() {
        let dir = tempdir().expect("tempdir");
        let now = DateTime::parse_from_rfc3339("2026-04-04T02:30:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let store = fixed_store(dir.path().to_path_buf(), now);
        let object_dir = dir.path().join("apps/argocd/demo");
        fs::create_dir_all(&object_dir).expect("dir");
        fs::write(
            object_dir.join("app-create-20260404T022000000Z.yaml"),
            "kind: Application\n",
        )
        .expect("write create");
        fs::write(
            object_dir.join("app-delete-20260404T022200000Z.yaml"),
            "kind: Application\n",
        )
        .expect("write delete");
        fs::write(
            object_dir.join("app-update-20260404T022100000Z.yaml"),
            "kind: Application\n",
        )
        .expect("write update");

        let versions = store
            .list_versions_sync(ResourceKind::App, "argocd", "demo")
            .expect("versions");
        let names = versions
            .into_iter()
            .map(|item| item.meta.file_name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "app-delete-20260404T022200000Z.yaml",
                "app-update-20260404T022100000Z.yaml",
                "app-create-20260404T022000000Z.yaml"
            ]
        );
    }

    #[test]
    fn prune_snapshots_older_than_retention_days() {
        let dir = tempdir().expect("tempdir");
        let now = DateTime::parse_from_rfc3339("2026-04-20T00:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let store = fixed_store(dir.path().to_path_buf(), now);
        let object_dir = dir.path().join("apps/argocd/demo");
        fs::create_dir_all(&object_dir).expect("dir");
        fs::write(
            object_dir.join("app-create-20260401T000000000Z.yaml"),
            "kind: Application\n",
        )
        .expect("old");
        fs::write(
            object_dir.join("app-update-20260418T000000000Z.yaml"),
            "kind: Application\n",
        )
        .expect("new");

        store
            .prune_old_snapshots_sync(&object_dir, now)
            .expect("prune old files");

        assert!(
            !object_dir
                .join("app-create-20260401T000000000Z.yaml")
                .exists()
        );
        assert!(
            object_dir
                .join("app-update-20260418T000000000Z.yaml")
                .exists()
        );
    }
}
