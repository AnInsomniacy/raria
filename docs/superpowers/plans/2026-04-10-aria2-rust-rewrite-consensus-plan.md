# Raria 完整重写 aria2 共识实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `raria` 成为 Rust 原生、面向真实客户端与真实工作流可直接替代 aria2 的正式产品，而不是继续停留在“有很多代码、但产品语义尚未闭环”的状态。

**Architecture:** 保留当前 Rust 分层架构，不重建 `aria2-legacy` 的命令驱动内核；通过补齐运行时所有权契约、RPC/WS 对外契约、session/resume 可信路径、以及 BT 文件级可用性，把现有 crate 体系推进到产品替代级别。兼容性策略遵循“尽量兼容，但不被拖累；生态不支持时采用现代最优标准”。

**Tech Stack:** `tokio`, `reqwest`, `jsonrpsee`, `redb`, `governor`, `quick-xml`, `suppaftp`, `russh` + `russh-sftp`, `librqbit`, `tracing`, `tower-http`, `daemonize`, `tracing-appender`, `arc-swap`, `wiremock`.

---

## 一、共识结论

这是基于以下输入形成的共识计划：

- deep-interview 规格：[`/.omx/specs/deep-interview-aria2-rust-rewrite.md`](/Users/sekiro/Projects/VSCode/raria/.omx/specs/deep-interview-aria2-rust-rewrite.md)
- parity 台账：
  - [`docs/parity/option-tiers.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/option-tiers.md)
  - [`docs/parity/protocol-matrix.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/protocol-matrix.md)
  - [`docs/parity/rpc-matrix.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/rpc-matrix.md)
  - [`docs/parity/bt-gap-ledger.md`](/Users/sekiro/Projects/VSCode/raria/docs/parity/bt-gap-ledger.md)
- 当前代码热点：
  - [`crates/raria-rpc/src/server.rs`](/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/server.rs)
  - [`crates/raria-cli/src/daemon.rs`](/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs)
  - [`crates/raria-bt/src/service.rs`](/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/src/service.rs)
- 候选方案：
  - Claude / Antigravity v1
  - 后续修正版
  - 本地 product-replacement 计划
- 共识评审：
  - Architect 反馈：问题不在功能点数量，而在**运行时所有权**与**产品级契约**
  - Critic verdict：`ITERATE`，必须补 4 点后才能过审

## 二、RALPLAN-DR 摘要

### 1. Principles

1. **产品替代性优先**：首个“重写完成版”必须先服务真实客户端与真实用户工作流。
2. **生态优先**：能用成熟 crate 解决，就不要手搓协议或内核。
3. **语义闭环优先于功能接线**：`wired` 不等于“可替代”。
4. **显式管理兼容尾巴**：不把所有 legacy 细节都塞进 GA。
5. **台账与计划一致**：计划、代码、parity ledger 必须说同一种话。

### 2. Decision Drivers

1. 你明确选择了：**A = 先成为真正可替代 aria2 的产品**
2. 你明确要求：**尽可能兼容，但不要被拖累**
3. 你明确要求：**优先采用成熟 Rust 库和现代最佳实践**

### 3. Viable Options

#### Option A: parity-first

- 特征：优先清空 legacy 兼容差异，尽量先补完 CLI / `.aria2` / 旧语义
- 优点：对老用户和脚本最友好
- 缺点：会显著拖回“重造 aria2 内核”的路线

#### Option B: replacement-first

- 特征：优先完成 daemon / session / RPC / WebSocket / 客户端工作流 / 主协议路径
- 优点：最符合你的目标，最容易和成熟 crate 对齐
- 缺点：部分 legacy 边角兼容需要后置

#### Option C: hybrid-with-contracts

- 特征：以 replacement-first 为主，但把“哪些兼容项进入 GA、哪些进入 Tail、哪些永久视为 Gap”写成硬契约
- 优点：既避免无限补洞，也避免目标漂移
- 缺点：前期规划要求更严格

### 4. Chosen Option

选择 **Option C: hybrid-with-contracts**。

原因：

- 纯 replacement-first 如果没有硬契约，容易在执行期重新漂成 backlog-first
- 纯 parity-first 与你的“不要手搓轮子”原则冲突
- `hybrid-with-contracts` 最能把你的目标、crate 生态、当前代码现状和执行可操作性统一起来

## 三、ADR

### Decision

采用“**产品替代性优先 + 兼容性分层治理**”的总路线来完成 `raria` 对 `aria2-legacy` 的 Rust 替代。

### Drivers

- 产品替代性优先于 legacy 外形完整性
- 现有 Rust 分层架构已经形成，不应推倒重来
- mature crates 已覆盖大部分核心协议面
- 当前最大缺口是运行时语义和对外契约，而不是“代码文件太少”

### Alternatives Considered

- parity-first
- pure replacement-first
- hybrid-with-contracts

### Why Chosen

它是唯一同时满足以下约束的路线：

- 不拖回重造 aria2 内核
- 不牺牲真实产品替代性
- 能与 parity 台账协作，而不是与其冲突

### Consequences

- GA 不以“清空所有 `has_code`”为目标
- `.aria2`、XML-RPC、隐式 FTPS、部分 BT 细节不阻塞首个完成版
- 必须补充显式的运行时与接口契约，尤其是 session/resume 与 RPC/WS

### Follow-ups

- 将本计划拆成 5 个执行子计划
- 建立 `GA / Tail / Gap` 对应的 parity ledger 更新规则

## 四、客观评价 Claude 终极方案

### 值得采纳的部分

1. 它正确吸收了 deep-interview 的结论，不再盲目追求“全部清零”
2. 它把 `CORS` 提升为了重要问题，这比前两版更接近真实客户端环境
3. 它保持了成熟 crate 路线，没有走自研协议栈
4. 它已经开始按 `GA Hard Gate / Tail / Gap` 来组织，而不是纯技术层切分

### 仍然不够好的部分

1. **把 CORS 说成简单加 `permissive()` 的 blocker，过于粗糙**
   - 当前代码里确实没有任何 origin/CORS 处理，本地搜索也证实了这一点
   - 但真正要补的不是“加 CORS”四个字，而是**RPC/WS 拓扑 + origin/auth 契约**
2. **对 session/resume 仍然偏乐观**
   - 当前已有 `restore()`、segment checkpoint、session smoke
   - 但这不等于“产品语义可信赖”
3. **BT `select-file` 仍被默认视为纯 wiring**
   - 在 `librqbit` 之上是否能完全满足 aria2/AriaNg 预期，必须先做 capability spike
4. **缺少“计划与 parity ledger 绑定”的机制**
   - 如果不绑定，很容易出现文档和台账漂移

## 五、外部生态证据

- `librqbit` 提供 `Session`、`AddTorrentOptions`、`ManagedTorrent` 等高层 torrent 能力，适合作为 BT 产品层基座，不应自写 BT 栈。[librqbit docs](https://docs.rs/librqbit/latest/librqbit/)
- `jsonrpsee` 继续适合做 aria2 兼容 JSON-RPC 基座；当前版本明确支持 server 侧能力。[jsonrpsee docs](https://docs.rs/crate/jsonrpsee/latest)
- `tower-http::cors` 适合为浏览器直连场景补 HTTP CORS 层，但这只是 `origin/auth` 契约的一部分，不等于完整客户端兼容策略。[tower-http docs](https://docs.rs/tower-http/latest/tower_http/cors/)
- aria2 官方手册中存在 `--rpc-allow-origin-all`，说明浏览器跨域访问在原产品语义中本来就是一等配置项，而不是临时 hack。[aria2 manual](https://aria2.github.io/manual/en/html/aria2c.html)
- `suppaftp` 和 `russh-sftp` 都已提供高层协议支持，继续走“集成 + 产品验证”路线最合理。[suppaftp](https://docs.rs/suppaftp/latest/suppaftp/tokio/index.html) [russh-sftp](https://docs.rs/russh-sftp/latest/russh_sftp/)

## 六、必须先建立的 4 个契约

这是 Critic 要求补齐、也是本计划的核心增量。

### Contract 1: Session / Resume 运行时所有权契约

**目标：** 明确谁拥有什么状态、何时落盘、何时恢复、谁是权威。

#### 结论

- `raria-core::Engine` 拥有：
  - job lifecycle
  - job-level persistence
  - activation / pause / remove / complete / fail transitions
- `raria-range::SegmentExecutor` 拥有：
  - segment 下载过程
  - segment-level checkpoint 生成
- `redb Store` 是唯一权威持久化层
- `daemon` 拥有：
  - save cadence
  - process lifecycle
  - restart restore orchestration
- `.aria2` 控制文件**不进入权威路径**

#### GA 要求

- 必须证明：
  - job-level restore 可靠
  - segment-level resume offset 可靠
  - graceful shutdown 和 crash-restart 都不会破坏权威状态
- 必须新增专门的端到端用例，而不是只靠 store 单测

### Contract 2: RPC / WS 拓扑与 Origin/Auth 契约

**目标：** 明确 `raria` 对浏览器、桌面客户端、反向代理和本地 daemon 的支持模型。

#### 当前事实

- [`server.rs`](/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/server.rs) 当前直接 `Server::builder().build(...)`
- 本地代码无任何 `cors/origin/allow-origin` 逻辑
- 另开了 `ws_notify` 端口用于通知广播
- 这说明现在的实现**有 RPC 服务**，但**对外契约并未定型**

#### GA 契约

- 明确支持两种部署：
  - `本地桌面/CLI/同源反代`
  - `浏览器直连 daemon`
- 明确配置项：
  - `rpc-allow-origin-all`
  - `rpc-allowed-origins`（新增，若需要）
- HTTP JSON-RPC、WebSocket JSON-RPC、通知流之间的关系必须被写清楚：
  - 同端口还是独立端口
  - AriaNg 依赖哪一条
  - 鉴权如何在 HTTP 和 WS 上保持一致

#### 关键动作

- 先做 **topology spike**
  - 验证 AriaNg 当前实际接法
  - 验证 `jsonrpsee` 同端口 HTTP+WS 是否足以覆盖
  - 评估当前独立 `ws_notify` 端口模型是否必须保留
- **P0-1 强制出口条件：**
  - spike 结束后必须落一份 ADR，冻结**唯一目标形态**
  - 后续实现、测试、GA 验收全部跟随这一形态
  - 不允许进入 Workstream 2 后继续保持“单端口/双端口都可以”的漂移状态

#### 规划默认倾向

- 若 spike 证明可行，优先目标形态应是：
  - `http(s)://host:port/jsonrpc` 提供 HTTP JSON-RPC
  - `ws(s)://host:port/jsonrpc` 提供 WS JSON-RPC 与通知
  - 相同 auth/origin 契约
- 当前独立 `ws_notify` 双端口模型视为**过渡实现**，不是默认终态

### Contract 3: BT File Selection Capability 契约

**目标：** 不把 `select-file` 视为纯接线工作，而是先确认 `librqbit` 能否支撑产品语义。

#### GA 前必须做的 spike

- 检查 `AddTorrentOptions` / file selection API 的真实行为
- 检查 magnet 元数据延迟解析时，文件选择何时可用
- 检查 `getFiles` 能否输出：
  - path
  - size
  - selected
  - progress

#### 决策规则

- **本计划现在直接冻结结论：**
  - `GA` 只要求实现 **metadata-ready torrent / magnet after metadata resolution** 的文件选择路径
  - `Tail` 处理 metadata 未就绪前的高级交互、边缘变体和更细粒度同步问题
- 也就是说：
  - `select-file` 不是“全部语义进入 GA”
  - 也不是“全部延后”
  - 而是**主流客户端可用路径进入 GA**

### Contract 4: GA Exit 与 Parity Ledger 绑定契约

**目标：** 让计划与台账同步演进。

#### 规则

- 每个 GA 工作项都必须映射到：
  - `option-tiers.md`
  - `protocol-matrix.md`
  - `rpc-matrix.md`
- 每项完成后，必须更新状态：
  - `has_code -> wired`
  - `wired -> tested`
  - `tested -> client_verified`

#### GA 定义

GA 不是：

- “主观觉得能用了”
- “有测试就算完成”

GA 是：

- 验收矩阵通过
- 相关 parity 台账被同步提升
- 剩余差异被明确归类到 Tail 或 Gap

## 七、最终实施结构

不采用纯“5 阶段线性 backlog”，改为 3 个主阶段 + 2 个附属阶段。

### Phase 0: Blocking Spikes

先做 3 个风险消解，不直接大面积开工。

#### P0-1 RPC/WS Topology Spike

**Files:**

- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/server.rs`
- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/tests/ws_push.rs`
- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/tests/ws_parity.rs`

**Questions to answer:**

- AriaNg 当前是否能接受独立 `ws_notify` 端口？
- 是否必须支持同端口 `ws://host:6800/jsonrpc`？
- auth + origin 如何统一？

**Deliverable:**

- 一页 RPC/WS topology ADR
- 必须包含最终冻结项：
  - 目标形态：单端口 or 双端口
  - `/jsonrpc` 是否同时承载 HTTP + WS
  - 通知走同一 WS 连接还是独立 socket
  - HTTP/WS/notify 的统一 auth/origin 规则

#### P0-2 Session/Resume Ownership Audit

**Files:**

- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/engine.rs`
- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/persist.rs`
- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-range/src/executor.rs`
- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs`
- Inspect tests:
  - `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/tests/segment_checkpoint.rs`
  - `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/tests/session_smoke.rs`

**Questions to answer:**

- graceful shutdown 和 crash-restart 各自恢复链路是否完整？
- job.downloaded 和 segment state 是否存在漂移风险？
- 是否需要额外的 committed checkpoint 语义？

**Deliverable:**

- 一页 session/resume contract note

#### P0-3 BT Select-File Capability Spike

**Files:**

- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/src/service.rs`
- Inspect: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs`
- Inspect tests:
  - `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/tests/bt_dispatch.rs`
  - `/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/tests/bt_gap_ledger.rs`

**Questions to answer:**

- latest `librqbit` 是否稳定支持 file subset download？
- API 是否能覆盖 AriaNg 文件选择交互？
- metadata-before-selection 是否是 blocker？

**Deliverable:**

- capability memo + whether D1 remains GA

### Phase 1: Replacement Core Beta

这是第一个真正的“重写完成版”候选阶段。

#### Workstream 1: Daemon Productization

**Files:**

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/main.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/config.rs`

**Actions:**

- `--daemon`
- `--log`
- `save-session-interval`
- Unix signal policy

**New crates:**

- `daemonize`
- `tracing-appender`

#### Workstream 2: RPC/WS Client Contract

**Files:**

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/server.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/facade.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/engine.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/job.rs`

**Actions:**

- origin/auth contract implementation
- CORS strategy
- runtime global limiter mutation
- `connections` truthful reporting

**New crates:**

- `tower-http`
- `tower`
- `arc-swap`

#### Workstream 3: Session/Resume Reliability

**Files:**

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/engine.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-core/src/persist.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-range/src/executor.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/daemon.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/single.rs`

**Actions:**

- harden restore semantics
- bind save cadence to daemon lifecycle
- verify resume semantics under interruption / restart
- wire `min-split-size`
- wire `retry-wait`

#### Workstream 4: BT File-Level Usability

**Files:**

- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-bt/src/service.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/methods.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-rpc/src/facade.rs`
- Modify: `/Users/sekiro/Projects/VSCode/raria/crates/raria-cli/src/bt_runtime.rs`

**Actions:**

- `select-file` 的 GA 范围固定为：
  - metadata-ready 路径
  - RPC `addTorrent` / AriaNg 常见文件选择流程
- BT `getFiles` detail
- BT pause/resume confidence

### Phase 2: Runtime Integrity Gate

这一阶段不是新增大功能，而是让“Beta 看起来能跑”变成“语义上可信赖”。

#### Focus

- AriaNg happy path 全链路
- Motrix-like daemon path
- daemon restart semantics
- RPC mutation semantics
- session durability
- parity ledger 更新

### Phase 3: Secondary Protocol Promotion

在 Beta 稳定后，把次一级高价值项逐步提升。

#### Includes

- FTP lifecycle hardening
- SFTP key auth E2E
- Metalink hash enforcement
- Metalink failover
- BT peer detail
- `onBtDownloadComplete`

### Phase 4: Compatibility Tail

#### Includes

- hook scripts
- Digest auth
- `save-cookies`
- `seed-ratio` / `seed-time`
- `bt-tracker`
- mTLS
- select low-value proxy improvements

### Phase 5: Accepted Gap Review

每个 minor release 检查一次：

- upstream crate 是否补齐某个 gap
- 若补齐，则考虑从 Gap 移入 Tail

## 八、GA 验收矩阵

GA 必须通过以下真实工作流，而不是仅仅 `cargo test --workspace`。

### Scenario A: AriaNg 直连 daemon

1. daemon 启动
2. 浏览器连接成功
3. HTTP URI 下载正常
4. 速度、进度、连接数显示正确
5. pause / resume 正常
6. global rate limit 动态生效
7. BT torrent / magnet 基本可用
8. metadata-ready 情况下文件选择正常
9. daemon 重启后任务恢复

### Scenario B: Motrix-like RPC daemon flow

1. 添加任务
2. 查看状态
3. 停止 / 恢复
4. daemon restart
5. session 恢复

### Scenario C: CLI 直下替代

1. HTTP 下载
2. interruption + resume
3. checksum verify

## 九、与 parity 台账绑定的退出条件

GA 宣布条件：

1. `option-tiers.md` 中所有真正影响 AriaNg/Motrix/daemon 替代性的 Tier A 项，不再停留在 `has_code`
2. `protocol-matrix.md` 中主流 HTTP/FTP/SFTP/BT/Metalink 工作流至少达到 `tested`，关键路径达到 `client_verified`
3. `rpc-matrix.md` 中所有 AriaNg/Motrix 高频路径达到 `client_verified`
4. 剩余未完成项都被明确放进：
   - `Compatibility Tail`
   - `Accepted Gaps`

## 十、GA Workstream 与 Parity 台账映射表

| Workstream | Ledger | Rows / Capability | Current | Target |
| --- | --- | --- | --- | --- |
| P0-1 / Workstream 2 | `rpc-matrix.md` | WS notifications topology / real client path | `tested` but topology-specific | `client_verified` under frozen contract |
| Workstream 2 | `rpc-matrix.md` | `aria2.changeGlobalOption` | `wired` | `client_verified` |
| Workstream 2 | `rpc-matrix.md` | `aria2.tellStatus` client semantics | `client_verified` structurally, but `connections` wrong | keep `client_verified` with corrected semantics |
| Workstream 2 | `option-tiers.md` | `rpc-listen-all` | `tested` | `client_verified` if required by chosen topology |
| Workstream 2 | `option-tiers.md` | `enable-rpc` / `rpc-secret` | `client_verified` / `tested` | `client_verified` |
| Workstream 1 | `option-tiers.md` | `daemon` | `has_code` | `client_verified` |
| Workstream 1 | `option-tiers.md` | `log` | `has_code` | `tested` |
| Workstream 1 | `option-tiers.md` | `save-session-interval` | `has_code` | `tested` |
| Workstream 3 | `option-tiers.md` | `continue` | `has_code` | `client_verified` |
| Workstream 3 | `option-tiers.md` | `save-session` | `wired` | `client_verified` |
| Workstream 3 | `protocol-matrix.md` | HTTP resume | `wired` | `client_verified` |
| Workstream 3 | `protocol-matrix.md` | Session save / restore | `tested` | `client_verified` |
| Workstream 3 | `option-tiers.md` | `min-split-size` | `has_code` | `tested` |
| Workstream 3 | `option-tiers.md` | `retry-wait` | `has_code` | `tested` |
| Workstream 4 | `option-tiers.md` | `select-file` | `has_code` | `tested` for metadata-ready path |
| Workstream 4 | `protocol-matrix.md` | BT file selection | `has_code` | `tested` for metadata-ready path |
| Workstream 4 | `protocol-matrix.md` | BT pause / resume | `wired` | `client_verified` |
| Workstream 4 | `rpc-matrix.md` | `aria2.getFiles` BT semantics | `tested` structurally | `client_verified` for torrent file UX |

## 十一、可执行拆分建议

本计划过大，不应直接一次性执行。执行时必须拆成 5 个子计划：

1. `rpc-ws-contract-and-client-beta`
2. `daemon-session-resume`
3. `bt-file-ux`
4. `protocol-hardening`
5. `compatibility-tail`

## 十二、可用 agent types 建议

推荐可用 agent types：

- `planner`
- `architect`
- `critic`
- `executor`
- `test-engineer`
- `verifier`
- `dependency-expert`
- `writer`

## 十三、推荐执行分工

### 用 `$ralph` 的顺序执行方案

- Lane 1: `P0 spikes`
- Lane 2: `Phase 1 Workstream 1 + 2`
- Lane 3: `Phase 1 Workstream 3 + 4`
- Lane 4: `Phase 2 verification + parity ledger updates`

推荐 reasoning:

- 架构/契约设计：`high`
- 日常实现：`medium`
- 验证与回归：`medium`

### 用 `$team` 的并行执行方案

- Worker A: daemon/productization
- Worker B: RPC/WS contract + AriaNg path
- Worker C: session/resume semantics
- Worker D: BT file usability spike + implementation
- Worker E: parity ledger / docs / verification harness

推荐 reasoning:

- contract and topology lanes: `high`
- implementation lanes: `medium`
- test/doc lanes: `medium`

## 十四、团队验证路径

无论走 `$ralph` 还是 `$team`，最后都要走同一条验证路径：

1. 单元/集成测试通过
2. AriaNg happy path 手工验证
3. session/restart 验证
4. BT 文件级工作流验证
5. 更新 parity 台账
6. 写明剩余 Tail / Gap

## 十五、最终建议

最推荐的执行顺序不是“先把所有 backlog 写满”，而是：

1. **先做 P0 三个 spike**
2. **确认 RPC/WS、session/resume、BT select-file 三条契约**
3. **再进入 Replacement Core Beta**

这比直接照 Claude 方案“从 CORS 开始一路往下做”更稳，因为：

- 它先消解了最大的架构风险
- 它避免后面返工
- 它更符合“不要手搓轮子，但也不要被拖累”的原则
