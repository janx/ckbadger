{{/*
Expand the name of the chart.
*/}}
{{- define "ckbadger.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "ckbadger.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "ckbadger.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "ckbadger.labels" -}}
helm.sh/chart: {{ include "ckbadger.chart" . }}
{{ include "ckbadger.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "ckbadger.selectorLabels" -}}
app.kubernetes.io/name: {{ include "ckbadger.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Create the name of the service account to use
*/}}
{{- define "ckbadger.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "ckbadger.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Redis URL
*/}}
{{- define "ckbadger.redisUrl" -}}
{{- if .Values.redis.enabled }}
redis://{{ include "ckbadger.fullname" . }}-redis-master:6379
{{- else if .Values.externalRedis.host }}
redis://{{ .Values.externalRedis.host }}:{{ .Values.externalRedis.port }}
{{- else }}
""
{{- end }}
{{- end }}
