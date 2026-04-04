{{- define "argo-history.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "argo-history.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := include "argo-history.name" . -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "argo-history.labels" -}}
app.kubernetes.io/name: {{ include "argo-history.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{- end -}}

{{- define "argo-history.selectorLabels" -}}
app.kubernetes.io/name: {{ include "argo-history.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "argo-history.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "argo-history.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{- define "argo-history.webhookServiceName" -}}
{{- printf "%s-webhook" (include "argo-history.fullname" .) -}}
{{- end -}}

{{- define "argo-history.uiServiceName" -}}
{{- printf "%s-ui" (include "argo-history.fullname" .) -}}
{{- end -}}

{{- define "argo-history.historyVolumeName" -}}
{{- printf "%s-history" (include "argo-history.fullname" .) -}}
{{- end -}}

{{- define "argo-history.configMapName" -}}
{{- printf "%s-config" (include "argo-history.fullname" .) -}}
{{- end -}}

{{- define "argo-history.certificateName" -}}
{{- printf "%s-webhook" (include "argo-history.fullname" .) -}}
{{- end -}}

{{- define "argo-history.issuerName" -}}
{{- if .Values.certManager.issuer.name -}}
{{- .Values.certManager.issuer.name -}}
{{- else -}}
{{- printf "%s-selfsigned" (include "argo-history.fullname" .) -}}
{{- end -}}
{{- end -}}

{{- define "argo-history.webhookSecretName" -}}
{{- if .Values.certManager.certificate.secretName -}}
{{- .Values.certManager.certificate.secretName -}}
{{- else -}}
{{- printf "%s-webhook-cert" (include "argo-history.fullname" .) -}}
{{- end -}}
{{- end -}}
