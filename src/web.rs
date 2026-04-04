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
use tower_http::services::ServeDir;
use tracing::error;

use crate::{
    admission::{AdmissionReviewRequest, AdmissionReviewResponse},
    model::{ObjectHistory, Operation, ResourceKind},
    storage::HistoryStore,
    templates::{HistoryTemplate, SidebarObject, VersionLink},
};

#[derive(Clone)]
pub struct AppState {
    pub store: HistoryStore,
}

#[derive(Debug, Deserialize)]
pub struct VersionQuery {
    pub version: Option<String>,
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
) -> Result<Html<String>, AppError> {
    let kind = resource_from_path(path.path())?;
    render_history_page(&state, kind, None, None).await
}

async fn history_object(
    kind: ResourceKind,
    state: AppState,
    namespace: String,
    name: String,
    version: Option<String>,
) -> Result<Html<String>, AppError> {
    let namespace = decode_namespace(&namespace);
    render_history_page(&state, kind, Some((namespace, name)), version).await
}

async fn history_app_object(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
    Query(query): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    history_object(ResourceKind::App, state, namespace, name, query.version).await
}

async fn history_appset_object(
    State(state): State<AppState>,
    Path((namespace, name)): Path<(String, String)>,
    Query(query): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    history_object(ResourceKind::AppSet, state, namespace, name, query.version).await
}

async fn render_history_page(
    state: &AppState,
    kind: ResourceKind,
    selected: Option<(String, String)>,
    selected_version: Option<String>,
) -> Result<Html<String>, AppError> {
    let app_count = state.store.list_objects(ResourceKind::App)?.len();
    let appset_count = state.store.list_objects(ResourceKind::AppSet)?.len();
    let objects = state.store.list_objects(kind)?;

    let selected_key = selected
        .as_ref()
        .map(|(namespace, name)| (crate::model::namespace_key(namespace), name.clone()));

    let sidebar = objects
        .iter()
        .map(|item| SidebarObject {
            name: item.name.clone(),
            namespace: item.namespace.clone(),
            latest_timestamp: item
                .latest_timestamp
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            version_count: item.version_count,
            href: format!("/{}/{}/{}", kind.route(), item.namespace_key, item.name),
            is_selected: selected_key
                .as_ref()
                .map(|(namespace, name)| namespace == &item.namespace_key && name == &item.name)
                .unwrap_or(false),
        })
        .collect::<Vec<_>>();

    let mut has_selection = false;
    let mut selected_name = String::new();
    let mut selected_namespace = String::new();
    let mut versions = Vec::new();
    let mut preview_content = String::new();

    if let Some((namespace, name)) = selected {
        let history = state.store.get_history(kind, &namespace, &name)?;
        if history.versions.is_empty() {
            return Err(AppError::not_found("history"));
        }
        has_selection = true;
        selected_name = history.name.clone();
        selected_namespace = history.namespace.clone();
        let active_file = selected_version
            .filter(|value| history.versions.iter().any(|item| item.file_name == *value))
            .unwrap_or_else(|| history.versions[0].file_name.clone());
        preview_content = state
            .store
            .read_snapshot(kind, &namespace, &name, &active_file)
            .with_context(|| "read selected snapshot")?;
        versions = history
            .versions
            .into_iter()
            .map(|item| VersionLink {
                title: format!("{} / {}", item.operation, item.file_name),
                timestamp: item.timestamp,
                href: format!(
                    "/{}/{}/{}?version={}",
                    kind.route(),
                    crate::model::namespace_key(&namespace),
                    name,
                    item.file_name
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
    }

    let template = HistoryTemplate {
        active_route: kind.route(),
        app_count,
        appset_count,
        objects: sidebar,
        has_selection,
        selected_name,
        selected_namespace,
        versions,
        preview_content,
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
    Ok(Json(state.store.list_objects(kind)?))
}

async fn object_api(
    State(state): State<AppState>,
    Path((resource, namespace, name)): Path<(String, String, String)>,
) -> Result<Json<ObjectHistory>, AppError> {
    let kind =
        ResourceKind::from_route(&resource).ok_or_else(|| AppError::not_found("resource"))?;
    Ok(Json(state.store.get_history(
        kind,
        &decode_namespace(&namespace),
        &name,
    )?))
}

async fn download_snapshot(
    State(state): State<AppState>,
    Path((resource, namespace, name, file_name)): Path<(String, String, String, String)>,
) -> Result<Response, AppError> {
    let kind =
        ResourceKind::from_route(&resource).ok_or_else(|| AppError::not_found("resource"))?;
    let content =
        state
            .store
            .read_snapshot(kind, &decode_namespace(&namespace), &name, &file_name)?;
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
    Json(handle_admission(&state, ResourceKind::App, review))
}

async fn applicationset_webhook(
    State(state): State<AppState>,
    Json(review): Json<AdmissionReviewRequest>,
) -> Json<AdmissionReviewResponse> {
    Json(handle_admission(&state, ResourceKind::AppSet, review))
}

fn handle_admission(
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
        match state.store.save_snapshot(kind, operation, manifest) {
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
