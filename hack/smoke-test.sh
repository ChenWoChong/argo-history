#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NAMESPACE="${NAMESPACE:-argo-history}"
NODEPORT="${NODEPORT:-32080}"

kubectl delete application history-demo-app -n argocd --ignore-not-found --wait=true
kubectl delete applicationset history-demo -n argocd --ignore-not-found --wait=true

kubectl apply -f - <<'EOF'
apiVersion: argoproj.io/v1alpha1
kind: ApplicationSet
metadata:
  name: history-demo
  namespace: argocd
spec:
  generators:
    - list:
        elements:
          - name: history-demo
            namespace: default
  template:
    metadata:
      name: "{{name}}"
      labels:
        app.kubernetes.io/managed-by: argo-history-smoke
    spec:
      project: default
      source:
        repoURL: https://github.com/argoproj/argocd-example-apps.git
        targetRevision: HEAD
        path: guestbook
      destination:
        server: https://kubernetes.default.svc
        namespace: "{{namespace}}"
      syncPolicy:
        automated: {}
EOF

kubectl apply -f - <<'EOF'
apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: history-demo-app
  namespace: argocd
spec:
  project: default
  source:
    repoURL: https://github.com/argoproj/argocd-example-apps.git
    targetRevision: HEAD
    path: guestbook
  destination:
    server: https://kubernetes.default.svc
    namespace: default
EOF

kubectl patch applicationset history-demo -n argocd --type merge -p '{"spec":{"template":{"metadata":{"labels":{"history-version":"v2"}}}}}'
kubectl patch application history-demo-app -n argocd --type merge -p '{"spec":{"source":{"targetRevision":"master"}}}'
kubectl delete application history-demo-app -n argocd --wait=true
kubectl delete applicationset history-demo -n argocd --wait=true

sleep 5

curl -fsSL "http://127.0.0.1:${NODEPORT}/api/v1/apps" >/tmp/argo-history-apps.json
curl -fsSL "http://127.0.0.1:${NODEPORT}/api/v1/appsets" >/tmp/argo-history-appsets.json

grep -q "history-demo" /tmp/argo-history-appsets.json
grep -q "history-demo-app" /tmp/argo-history-apps.json

echo "Smoke test passed"
