use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct AdmissionReviewRequest {
    pub request: Option<AdmissionRequest>,
}

#[derive(Debug, Deserialize)]
pub struct AdmissionRequest {
    pub uid: String,
    pub operation: String,
    #[serde(rename = "dryRun")]
    pub dry_run: Option<bool>,
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub object: Option<Value>,
    #[serde(rename = "oldObject")]
    pub old_object: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct AdmissionReviewResponse {
    #[serde(rename = "apiVersion")]
    pub api_version: &'static str,
    pub kind: &'static str,
    pub response: AdmissionResponse,
}

#[derive(Debug, Serialize)]
pub struct AdmissionResponse {
    pub uid: String,
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AdmissionStatus>,
}

#[derive(Debug, Serialize)]
pub struct AdmissionStatus {
    pub message: String,
}

impl AdmissionReviewResponse {
    pub fn allow(uid: String) -> Self {
        Self {
            api_version: "admission.k8s.io/v1",
            kind: "AdmissionReview",
            response: AdmissionResponse {
                uid,
                allowed: true,
                status: None,
            },
        }
    }

    pub fn deny(uid: String, message: impl Into<String>) -> Self {
        Self {
            api_version: "admission.k8s.io/v1",
            kind: "AdmissionReview",
            response: AdmissionResponse {
                uid,
                allowed: false,
                status: Some(AdmissionStatus {
                    message: message.into(),
                }),
            },
        }
    }
}
