# argo-history

独立的 Argo CD `Application` / `ApplicationSet` 历史备份与查看项目。

## 文档

- [整体架构与 API 文档](/Users/wochong/Documents/code/truth-ai/infras/argo-history/docs/architecture.md)

## 功能

- Validating admission webhook 拦截 `CREATE`、`UPDATE`、`DELETE`
- 将 `Application` / `ApplicationSet` 的清洗后版本保存到持久卷
- 文件命名格式为 `app-操作-时间.yaml` 或 `appset-操作-时间.yaml`
- 默认按对象保留最近 14 天历史
- 自带 Web 查看页面，按 `App` 和 `AppSet` 分栏查看历史
- 前端通过局部刷新更新对象列表和详情区，不再每次整页跳转
- App 列表会把 `AppSet` 生成的 `App` 单独折叠分组
- YAML 预览和 diff 预览均为彩色高亮显示
- 支持搜索、来源标识、默认查看内容、diff 预览和单独下载版本文件

## 常用命令

```bash
make fmt
make test
make check
make docker-build
make helm-install
make smoke-test
```

## Helm 安装说明

Chart 默认启用 `cert-manager` 来签发 admission webhook 证书。

- 集群已安装 `cert-manager`：直接执行 `make helm-install`
- 集群未安装 `cert-manager`：执行 `make helm-install HELM_ARGS='--set certManager.enabled=false'`

关闭 `cert-manager` 后，chart 会自动生成并复用一个自签名 webhook TLS Secret，同时将 CA 直接写入 `ValidatingWebhookConfiguration`。
