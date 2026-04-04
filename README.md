# argo-history

独立的 Argo CD `Application` / `ApplicationSet` 历史备份与查看项目。

## 功能

- Validating admission webhook 拦截 `CREATE`、`UPDATE`、`DELETE`
- 将 `Application` / `ApplicationSet` 的清洗后版本保存到持久卷
- 文件命名格式为 `app-操作-时间.yaml` 或 `appset-操作-时间.yaml`
- 自带 Web 查看页面，按 `App` 和 `AppSet` 分栏查看历史
- 支持默认查看内容和单独下载版本文件

## 常用命令

```bash
make fmt
make test
make check
make docker-build
make helm-install
make smoke-test
```
