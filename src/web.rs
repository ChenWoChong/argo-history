use anyhow::{Context, Result};
use askama::Template;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use serde::Deserialize;
use similar::{ChangeTag, TextDiff};
use tower_http::services::ServeDir;
use tracing::error;
use urlencoding::encode;

use crate::{
    admission::{AdmissionReviewRequest, AdmissionReviewResponse},
    model::{ObjectHistory, Operation, ResourceKind},
    storage::HistoryStore,
    templates::{
        CodeLine, CodeToken, HighlightedBlock, HistoryTemplate, Pagination, PaginationLink,
        SidebarGroup, SidebarObject, VersionLink,
    },
};

const PAGE_SIZE: usize = 8;

#[derive(Clone)]
pub struct AppState {
    pub store: HistoryStore,
}

#[derive(Debug, Deserialize, Default)]
pub struct PageQuery {
    pub q: Option<String>,
    pub version: Option<String>,
    pub objects_page: Option<usize>,
    pub versions_page: Option<usize>,
}

pub fn http_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/apps") }))
        .route("/healthz", get(|| async { "ok" }))
        .route("/apps", get(history_index))
        .route("/appsets", get(history_index))
        .route("/apps/{namespace}/{name}", get(history_app_object))
        .route("/appsets/{namespace}/{name}", get(history_appset_object))
        .route(
            "/download/{resource}/{namespace}/{name}/{file_name}",
            get(download_snapshot),
        )
        .route("/api/v1/{resource}", get(list_api))
        .route("/api/v1/{resource}/{namespace}/{name}", get(object_api))
        .route(
            "/api/v1/download/{resource}/{namespace}/{name}/{file_name}",
            get(download_snapshot),
        )
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}

pub fn webhook_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/webhook/application",
            axum::routing::post(application_webhook),
        )
        .route(
            "/webhook/applicationset",
            axum::routing::post(applicationset_webhook),
        )
        .with_state(state)
}

async fn history_index(
    State(state): State<AppState>,
    path: axum::http::Uri,
    Query(query): Query<PageQuery>,
) -> Result<Html<String>, AppError> {
    let kind = resource_from_path(path.path())?;
    render_history_page(&state, kind, None, query).await
}

async fn history_object(
    kind: ResourceKind,
    state: AppState,
    namespace: String,
    name: String,
    query: PageQuery,
) -> Result<Html<String>, AppError> {
    let namespace = decode_namespace(&namespace);
    render_history_page(&state, kind, Some((namespace, name)), query).await
}

async fn history_app_object(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
    Query(query): Query<PageQuery>,
) -> Result<Html<String>, AppError> {
    history_object(ResourceKind::App, state, namespace, name, query).await
}

async fn history_appset_object(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
    Query(query): Query<PageQuery>,
) -> Result<Html<String>, AppError> {
    history_object(ResourceKind::AppSet, state, namespace, name, query).await
}

async fn render_history_page(
    state: &AppState,
    kind: ResourceKind,
    selected: Option<(String, String)>,
    query: PageQuery,
) -> Result<Html<String>, AppError> {
    let search_query = query.q.unwrap_or_default();
    let app_count = state.store.list_objects(ResourceKind::App).await?.len();
    let appset_count = state.store.list_objects(ResourceKind::AppSet).await?.len();
    let search_lower = search_query.to_ascii_lowercase();
    let objects = state
        .store
        .list_objects(kind)
        .await?
        .into_iter()
        .filter(|item| {
            if search_lower.is_empty() {
                return true;
            }
            let haystack = format!(
                "{} {} {}",
                item.name.to_ascii_lowercase(),
                item.namespace.to_ascii_lowercase(),
                item.source_label.to_ascii_lowercase()
            );
            haystack.contains(&search_lower)
        })
        .collect::<Vec<_>>();

    let selected_key = selected
        .as_ref()
        .map(|(namespace, name)| (crate::model::namespace_key(namespace), name.clone()));

    let object_page = resolve_object_page(&objects, &selected_key, query.objects_page);
    let object_page_count = total_pages(objects.len(), PAGE_SIZE);
    let object_slice = paginate_slice(&objects, object_page, PAGE_SIZE);

    let sidebar_objects = object_slice
        .iter()
        .map(|item| SidebarObject {
            name: item.name.clone(),
            namespace: item.namespace.clone(),
            source_label: item.source_label.clone(),
            latest_timestamp: item
                .latest_timestamp
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            version_count: item.version_count,
            href: object_href(
                kind,
                &item.namespace_key,
                &item.name,
                &search_query,
                object_page,
                None,
                None,
            ),
            is_selected: selected_key
                .as_ref()
                .map(|(namespace, name)| namespace == &item.namespace_key && name == &item.name)
                .unwrap_or(false),
        })
        .collect::<Vec<_>>();
    let object_groups = build_sidebar_groups(kind, sidebar_objects);
    let object_pagination = build_pagination(
        object_page,
        object_page_count,
        objects.len(),
        "objects_page",
        format!("/{}", kind.route()),
        |page| collection_href(kind, &search_query, page),
    );

    let mut has_selection = false;
    let mut selected_name = String::new();
    let mut selected_namespace = String::new();
    let mut selected_source_label = String::new();
    let mut versions = Vec::new();
    let mut versions_pagination = None;
    let mut preview_block = HighlightedBlock {
        title: "YAML 预览".to_string(),
        lines: vec![plain_line("请选择一个对象查看内容。")],
    };
    let mut diff_block = None;

    if let Some((namespace, name)) = selected {
        let history = state.store.get_history(kind, &namespace, &name).await?;
        if history.versions.is_empty() {
            return Err(AppError::not_found("history"));
        }

        has_selection = true;
        selected_name = history.name.clone();
        selected_namespace = history.namespace.clone();
        selected_source_label = history.source_label.clone();

        let versions_page =
            resolve_versions_page(&history, query.version.as_deref(), query.versions_page);
        let active_file = query
            .version
            .filter(|value| history.versions.iter().any(|item| item.file_name == *value))
            .unwrap_or_else(|| {
                paginate_slice(&history.versions, versions_page, PAGE_SIZE)[0]
                    .file_name
                    .clone()
            });

        let preview_content = state
            .store
            .read_snapshot(kind, &namespace, &name, &active_file)
            .await
            .with_context(|| "read selected snapshot")?;
        preview_block = HighlightedBlock {
            title: "YAML 预览".to_string(),
            lines: highlight_yaml_block(&preview_content),
        };

        let active_index = history
            .versions
            .iter()
            .position(|item| item.file_name == active_file)
            .unwrap_or(0);
        if active_index + 1 < history.versions.len() {
            let previous_file = history.versions[active_index + 1].file_name.clone();
            let previous_content = state
                .store
                .read_snapshot(kind, &namespace, &name, &previous_file)
                .await
                .with_context(|| "read previous snapshot")?;
            diff_block = Some(HighlightedBlock {
                title: format!("差异预览: {}", previous_file),
                lines: highlight_diff_block(&previous_content, &preview_content),
            });
        }

        let version_page_count = total_pages(history.versions.len(), PAGE_SIZE);
        versions = paginate_slice(&history.versions, versions_page, PAGE_SIZE)
            .iter()
            .map(|item| VersionLink {
                title: format!("{} / {}", item.operation, item.file_name),
                timestamp: item.timestamp.clone(),
                href: object_href(
                    kind,
                    &crate::model::namespace_key(&namespace),
                    &name,
                    &search_query,
                    object_page,
                    Some(versions_page),
                    Some(&item.file_name),
                ),
                download_href: format!(
                    "/download/{}/{}/{}/{}",
                    kind.route(),
                    crate::model::namespace_key(&namespace),
                    name,
                    item.file_name
                ),
                is_active: item.file_name == active_file,
            })
            .collect();
        versions_pagination = build_pagination(
            versions_page,
            version_page_count,
            history.versions.len(),
            "versions_page",
            format!(
                "/{}/{}/{}",
                kind.route(),
                crate::model::namespace_key(&namespace),
                name
            ),
            |page| {
                object_href(
                    kind,
                    &crate::model::namespace_key(&namespace),
                    &name,
                    &search_query,
                    object_page,
                    Some(page),
                    Some(&active_file),
                )
            },
        );
    }

    let template = HistoryTemplate {
        active_route: kind.route(),
        apps_href: collection_href(ResourceKind::App, &search_query, object_page),
        appsets_href: collection_href(ResourceKind::AppSet, &search_query, object_page),
        search_action: format!("/{}", kind.route()),
        app_count,
        appset_count,
        object_groups,
        object_pagination,
        current_objects_page: object_page,
        search_query,
        has_selection,
        selected_name,
        selected_namespace,
        selected_source_label,
        versions,
        versions_pagination,
        preview_block,
        diff_block,
    };

    let body = template
        .render()
        .map_err(|error| AppError::from(anyhow::anyhow!(error)))?;
    Ok(Html(body))
}

async fn list_api(
    State(state): State<AppState>,
    Path(resource): Path<String>,
) -> Result<Json<Vec<crate::model::ObjectSummary>>, AppError> {
    let kind =
        ResourceKind::from_route(&resource).ok_or_else(|| AppError::not_found("resource"))?;
    Ok(Json(state.store.list_objects(kind).await?))
}

async fn object_api(
    State(state): State<AppState>,
    Path((resource, namespace, name)): Path<(String, String, String)>,
) -> Result<Json<ObjectHistory>, AppError> {
    let kind =
        ResourceKind::from_route(&resource).ok_or_else(|| AppError::not_found("resource"))?;
    Ok(Json(
        state
            .store
            .get_history(kind, &decode_namespace(&namespace), &name)
            .await?,
    ))
}

async fn download_snapshot(
    State(state): State<AppState>,
    Path((resource, namespace, name, file_name)): Path<(String, String, String, String)>,
) -> Result<Response, AppError> {
    let kind =
        ResourceKind::from_route(&resource).ok_or_else(|| AppError::not_found("resource"))?;
    let content = state
        .store
        .read_snapshot(kind, &decode_namespace(&namespace), &name, &file_name)
        .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/yaml"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", file_name),
            ),
        ],
        content,
    )
        .into_response())
}

async fn application_webhook(
    State(state): State<AppState>,
    Json(review): Json<AdmissionReviewRequest>,
) -> Json<AdmissionReviewResponse> {
    Json(handle_admission(&state, ResourceKind::App, review).await)
}

async fn applicationset_webhook(
    State(state): State<AppState>,
    Json(review): Json<AdmissionReviewRequest>,
) -> Json<AdmissionReviewResponse> {
    Json(handle_admission(&state, ResourceKind::AppSet, review).await)
}

async fn handle_admission(
    state: &AppState,
    kind: ResourceKind,
    review: AdmissionReviewRequest,
) -> AdmissionReviewResponse {
    let request = match review.request {
        Some(request) => request,
        None => return AdmissionReviewResponse::deny("unknown".to_string(), "missing request"),
    };

    if request.dry_run.unwrap_or(false) {
        return AdmissionReviewResponse::allow(request.uid);
    }

    let operation = match Operation::parse(&request.operation) {
        Some(operation) => operation,
        None => return AdmissionReviewResponse::allow(request.uid),
    };

    let manifest = match operation {
        Operation::Delete => request.old_object.as_ref().or(request.object.as_ref()),
        Operation::Create | Operation::Update => request.object.as_ref(),
    };

    if let Some(manifest) = manifest {
        match state.store.save_snapshot(kind, operation, manifest).await {
            Ok(result) => {
                tracing::info!(
                    resource = kind.route(),
                    operation = operation.as_str(),
                    namespace = request.namespace.unwrap_or_default(),
                    name = request.name.unwrap_or_default(),
                    written = result.written,
                    file_name = result.file_name,
                    "snapshot stored"
                );
            }
            Err(error) => {
                error!(?error, resource = kind.route(), "failed to save snapshot");
                return AdmissionReviewResponse::deny(
                    request.uid,
                    format!("backup failed: {error}"),
                );
            }
        }
    }

    AdmissionReviewResponse::allow(request.uid)
}

fn build_sidebar_groups(kind: ResourceKind, objects: Vec<SidebarObject>) -> Vec<SidebarGroup> {
    if kind == ResourceKind::AppSet {
        return vec![SidebarGroup {
            title: "AppSet 对象".to_string(),
            count: objects.len(),
            open: true,
            objects,
        }];
    }

    let mut direct = Vec::new();
    let mut generated = Vec::new();
    for object in objects {
        if object.source_label.starts_with("Generated by AppSet/") {
            generated.push(object);
        } else {
            direct.push(object);
        }
    }

    let mut groups = Vec::new();
    if !direct.is_empty() {
        groups.push(SidebarGroup {
            title: "直连 App".to_string(),
            count: direct.len(),
            open: true,
            objects: direct,
        });
    }
    if !generated.is_empty() {
        groups.push(SidebarGroup {
            title: "AppSet 生成 App".to_string(),
            count: generated.len(),
            open: false,
            objects: generated,
        });
    }
    groups
}

fn highlight_yaml_block(content: &str) -> Vec<CodeLine> {
    split_lines(content)
        .into_iter()
        .map(|line| CodeLine {
            class_name: "code-line yaml-line".to_string(),
            prefix: String::new(),
            tokens: tokenize_yaml_line(&line),
        })
        .collect()
}

fn highlight_diff_block(previous: &str, current: &str) -> Vec<CodeLine> {
    let diff = TextDiff::from_lines(previous, current);
    let mut has_changes = false;
    let mut lines = Vec::new();

    for change in diff.iter_all_changes() {
        let (class_name, prefix) = match change.tag() {
            ChangeTag::Delete => {
                has_changes = true;
                ("code-line diff-line diff-remove", "- ")
            }
            ChangeTag::Insert => {
                has_changes = true;
                ("code-line diff-line diff-add", "+ ")
            }
            ChangeTag::Equal => ("code-line diff-line diff-context", "  "),
        };

        for line in split_lines(change.value()) {
            lines.push(CodeLine {
                class_name: class_name.to_string(),
                prefix: prefix.to_string(),
                tokens: tokenize_yaml_line(&line),
            });
        }
    }

    if !has_changes {
        return vec![plain_line("当前版本与上一版本无内容差异。")];
    }

    lines
}

fn split_lines(content: &str) -> Vec<String> {
    let mut lines = content
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect::<Vec<_>>();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn plain_line(message: &str) -> CodeLine {
    CodeLine {
        class_name: "code-line diff-note".to_string(),
        prefix: String::new(),
        tokens: vec![CodeToken {
            class_name: "tok-plain".to_string(),
            content: message.to_string(),
        }],
    }
}

fn tokenize_yaml_line(line: &str) -> Vec<CodeToken> {
    let mut tokens = Vec::new();
    if line.is_empty() {
        return tokens;
    }

    let indent_len = line.chars().take_while(|char| char.is_whitespace()).count();
    if indent_len > 0 {
        tokens.push(CodeToken {
            class_name: "tok-indent".to_string(),
            content: line[..indent_len].to_string(),
        });
    }

    let mut rest = &line[indent_len..];
    if rest.starts_with("- ") {
        tokens.push(CodeToken {
            class_name: "tok-list".to_string(),
            content: "- ".to_string(),
        });
        rest = &rest[2..];
    }

    if rest.trim_start().starts_with('#') {
        tokens.push(CodeToken {
            class_name: "tok-comment".to_string(),
            content: rest.to_string(),
        });
        return tokens;
    }

    if let Some(separator) = find_yaml_key_separator(rest) {
        let key = &rest[..separator];
        let after_key = &rest[separator..];
        if !key.is_empty() {
            tokens.push(CodeToken {
                class_name: "tok-key".to_string(),
                content: key.to_string(),
            });
        }

        if after_key.starts_with(": ") {
            tokens.push(CodeToken {
                class_name: "tok-punct".to_string(),
                content: ":".to_string(),
            });
            tokens.push(CodeToken {
                class_name: "tok-space".to_string(),
                content: " ".to_string(),
            });
            tokens.extend(tokenize_value_tokens(&after_key[2..]));
            return tokens;
        }

        if after_key == ":" {
            tokens.push(CodeToken {
                class_name: "tok-punct".to_string(),
                content: ":".to_string(),
            });
            return tokens;
        }
    }

    tokens.extend(tokenize_value_tokens(rest));
    tokens
}

fn find_yaml_key_separator(line: &str) -> Option<usize> {
    let mut single_quote = false;
    let mut double_quote = false;
    let chars = line.char_indices().peekable();

    for (index, char) in chars {
        match char {
            '\'' if !double_quote => single_quote = !single_quote,
            '"' if !single_quote => double_quote = !double_quote,
            ':' if !single_quote && !double_quote => {
                let tail = &line[index..];
                if tail == ":" || tail.starts_with(": ") {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn tokenize_value_tokens(value: &str) -> Vec<CodeToken> {
    let mut tokens = Vec::new();
    let chars = value.chars().collect::<Vec<_>>();
    let mut index = 0;

    while index < chars.len() {
        let current = chars[index];

        if current.is_whitespace() {
            let start = index;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            tokens.push(CodeToken {
                class_name: "tok-space".to_string(),
                content: chars[start..index].iter().collect(),
            });
            continue;
        }

        if current == '#' {
            tokens.push(CodeToken {
                class_name: "tok-comment".to_string(),
                content: chars[index..].iter().collect(),
            });
            break;
        }

        if matches!(current, '[' | ']' | '{' | '}' | ':' | ',') {
            tokens.push(CodeToken {
                class_name: "tok-punct".to_string(),
                content: current.to_string(),
            });
            index += 1;
            continue;
        }

        if current == '"' || current == '\'' {
            let quote = current;
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == quote {
                    index += 1;
                    break;
                }
                index += 1;
            }
            tokens.push(CodeToken {
                class_name: "tok-string".to_string(),
                content: chars[start..index].iter().collect(),
            });
            continue;
        }

        let start = index;
        while index < chars.len()
            && !chars[index].is_whitespace()
            && !matches!(chars[index], '[' | ']' | '{' | '}' | ':' | ',' | '#')
        {
            index += 1;
        }
        let word = chars[start..index].iter().collect::<String>();
        tokens.push(CodeToken {
            class_name: classify_scalar(&word).to_string(),
            content: word,
        });
    }

    tokens
}

fn classify_scalar(word: &str) -> &'static str {
    if word.is_empty() {
        return "tok-plain";
    }
    if matches!(
        word,
        "true" | "false" | "yes" | "no" | "on" | "off" | "True" | "False" | "YES" | "NO"
    ) {
        return "tok-bool";
    }
    if matches!(word, "null" | "Null" | "NULL" | "~") {
        return "tok-null";
    }
    if word.parse::<i64>().is_ok() || word.parse::<f64>().is_ok() {
        return "tok-number";
    }
    if word.contains("://") {
        return "tok-string";
    }
    "tok-plain"
}

fn collection_href(kind: ResourceKind, search_query: &str, object_page: usize) -> String {
    let mut query = Vec::new();
    if !search_query.is_empty() {
        query.push(format!("q={}", encode(search_query)));
    }
    if object_page > 1 {
        query.push(format!("objects_page={object_page}"));
    }
    if query.is_empty() {
        format!("/{}", kind.route())
    } else {
        format!("/{0}?{1}", kind.route(), query.join("&"))
    }
}

fn object_href(
    kind: ResourceKind,
    namespace: &str,
    name: &str,
    search_query: &str,
    object_page: usize,
    versions_page: Option<usize>,
    version: Option<&str>,
) -> String {
    let mut query = Vec::new();
    if !search_query.is_empty() {
        query.push(format!("q={}", encode(search_query)));
    }
    if object_page > 1 {
        query.push(format!("objects_page={object_page}"));
    }
    if let Some(versions_page) = versions_page.filter(|page| *page > 1) {
        query.push(format!("versions_page={versions_page}"));
    }
    if let Some(version) = version {
        query.push(format!("version={}", encode(version)));
    }

    if query.is_empty() {
        format!("/{}/{}/{}", kind.route(), namespace, name)
    } else {
        format!(
            "/{}/{}/{}?{}",
            kind.route(),
            namespace,
            name,
            query.join("&")
        )
    }
}

fn total_pages(total: usize, page_size: usize) -> usize {
    usize::max(1, total.div_ceil(page_size))
}

fn current_page(requested: Option<usize>, total_pages: usize) -> usize {
    requested.unwrap_or(1).clamp(1, total_pages)
}

fn paginate_slice<T>(items: &[T], page: usize, page_size: usize) -> &[T] {
    if items.is_empty() {
        return items;
    }
    let start = (page - 1) * page_size;
    let end = usize::min(start + page_size, items.len());
    &items[start..end]
}

fn resolve_object_page(
    objects: &[crate::model::ObjectSummary],
    selected_key: &Option<(String, String)>,
    requested: Option<usize>,
) -> usize {
    let total = total_pages(objects.len(), PAGE_SIZE);
    if let Some(requested) = requested {
        return current_page(Some(requested), total);
    }
    if let Some((namespace, name)) = selected_key {
        if let Some(index) = objects
            .iter()
            .position(|item| item.namespace_key == *namespace && item.name == *name)
        {
            return index / PAGE_SIZE + 1;
        }
    }
    1
}

fn resolve_versions_page(
    history: &ObjectHistory,
    selected_version: Option<&str>,
    requested: Option<usize>,
) -> usize {
    let total = total_pages(history.versions.len(), PAGE_SIZE);
    if let Some(requested) = requested {
        return current_page(Some(requested), total);
    }
    if let Some(selected_version) = selected_version {
        if let Some(index) = history
            .versions
            .iter()
            .position(|item| item.file_name == selected_version)
        {
            return index / PAGE_SIZE + 1;
        }
    }
    1
}

fn build_pagination<F>(
    current: usize,
    total: usize,
    total_items: usize,
    input_name: &str,
    form_action: String,
    href_for: F,
) -> Option<Pagination>
where
    F: Fn(usize) -> String,
{
    if total <= 1 {
        return None;
    }

    let mut links = Vec::new();
    if current > 1 {
        links.push(PaginationLink {
            label: "首页".to_string(),
            href: Some(href_for(1)),
            is_active: false,
            is_gap: false,
        });
    }
    if current > 1 {
        links.push(PaginationLink {
            label: "上一页".to_string(),
            href: Some(href_for(current - 1)),
            is_active: false,
            is_gap: false,
        });
    }

    let start = usize::max(1, current.saturating_sub(1));
    let end = usize::min(total, current + 1);
    let mut page_numbers = Vec::new();
    page_numbers.push(1);
    for page in start..=end {
        if !page_numbers.contains(&page) {
            page_numbers.push(page);
        }
    }
    if !page_numbers.contains(&total) {
        page_numbers.push(total);
    }
    page_numbers.sort_unstable();

    let mut previous_page = None;
    for page in page_numbers {
        if let Some(previous) = previous_page
            && page > previous + 1
        {
            links.push(PaginationLink {
                label: "…".to_string(),
                href: None,
                is_active: false,
                is_gap: true,
            });
        }
        links.push(PaginationLink {
            label: page.to_string(),
            href: Some(href_for(page)),
            is_active: page == current,
            is_gap: false,
        });
        previous_page = Some(page);
    }

    if current < total {
        links.push(PaginationLink {
            label: "下一页".to_string(),
            href: Some(href_for(current + 1)),
            is_active: false,
            is_gap: false,
        });
        links.push(PaginationLink {
            label: "末页".to_string(),
            href: Some(href_for(total)),
            is_active: false,
            is_gap: false,
        });
    }

    Some(Pagination {
        links,
        input_name: input_name.to_string(),
        form_action,
        total_items,
        total_pages: total,
    })
}

fn resource_from_path(path: &str) -> Result<ResourceKind, AppError> {
    ResourceKind::from_route(path.trim_start_matches('/'))
        .ok_or_else(|| AppError::not_found("resource"))
}

fn decode_namespace(namespace: &str) -> String {
    if namespace == "_cluster" {
        String::new()
    } else {
        namespace.to_string()
    }
}

pub struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}
