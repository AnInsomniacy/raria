# Raria 完整重写 aria2 — 执行级定稿

> 本文件从 Round 4 最终实施计划同步。详见完整版：
> [implementation_plan.md](file:///Users/sekiro/.gemini/antigravity/brain/6a858c51-692c-4cff-9932-e3990f75de18/implementation_plan.md)

## 摘要

5 阶段 + 硬出口门 + 四级完成状态 (`has_code → wired → tested → client_verified`)

1. **接线 + CLI 拆分 + 测试底座** — Content-Disposition/FileAllocation/Cookie/Proxy 接入热路径
2. **非 BT 协议闭环** — netrc, conditional-get, timeout/retry, auto-rename, FTP/SFTP 修复
3. **管理器闭环** — session/continue 一体化, RPC 动态生效, hook scripts, Metalink 语义
4. **BT 接入完整** — capability spike 前置 → BtOps trait → 功能接入
5. **性能 & 高级** — 仅 benchmark 证明后进入

## 技术栈

保留: tokio, reqwest, suppaftp, russh, librqbit, jsonrpsee, redb, governor, quick-xml, clap, tracing

新增: rust-netrc, tracing-appender, daemonize, wiremock
