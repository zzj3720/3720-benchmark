# 直播平台整体优化调研

调研时间：2026-09-06，约 14:00–14:20（Asia/Shanghai）。范围：当前工作区代码、运行中的本机服务、公网只读 API、现有测试及隔离复现。没有修改生产代码、重启或发布服务。

结论：优先处理数据投影、请求状态和游戏回放契约，再优化性能与观赛界面。当前已有单写者 journal、SSE、不可变资源缓存和按尝试加载回放的基础，可以在此基础上分层修复，无需先整体重写。

## 证据范围与限制

- 当前工作区 `main` 的 HEAD 为 `c0e3f17`，有大量既有未提交改动；代码结论针对调研时工作树。
- 运行中的 release 指向 `20260802T113601Z-1932`。本地代码与部署版本不能视为完全一致；下文分别标记运行实测、隔离复现和代码风险。
- 公网首页、本机 3000 首页、3740 health、runs 与 SSE 均可响应。抽样公网 Parabox 回放返回 200、15 帧、2 组操作。
- iab 不可用；Chrome 创建/选择标签页均超时，未取得有效截图。因此这不是完成的视觉/无障碍审计，没有声称验证布局、手机效果、实际帧率、键盘导航或浏览器断网恢复。
- 当前 API 报告 48 条记录、7 个游戏、0 条 live。没有观察到真实运行中的连续更新；并发、断流及半行写入通过隔离代码复现，不代表已证实线上发生过数据丢失。

## 用户流程检查

1. **进入大盘：接口通畅，信息组织需优化。** Parabox 有 28 条 run；当前代码将它们全部绘制，并循环使用 8 种颜色。没有模型筛选、曲线开关或按实验配置分组；同一模型还存在展示名与 provider ID 混用。`MODELS` 实际统计的是 run 数。视觉可读性仍需截图检查。
2. **选择 run：请求一致性与恢复不可靠。** 已复现旧详情覆盖新详情；加载失败没有独立恢复路径。连接在线不能代表详情新鲜。
3. **观看当前状态：状态提示与时间需核对。** lease 变化但 sequence 不变时详情不刷新；直播模式内回看旧帧仍可能标注“位于最新状态”。
4. **选择历史尝试：部分游戏实测不可选。** Operator、Kitchen 返回非空分组，但 attempts 全空；失败关卡也可能被目录过滤。
5. **返回直播：已复现被未完成请求拉回回放。** 导出与播放共用当前画面和游标，还需补充切换、取消、卸载测试。
6. **重连与部署恢复：存在已确认的依赖缺口。** 当前服务运行正常，但生产 release 的依赖链接已失效；本次未重启验证。

## 第一优先级

### 1. 日志投影可能漏掉跨读取边界的事件〔隔离复现〕

`project_journal_range` 将 `take(length-offset).lines()` 的末尾半行也写入投影，并补换行；`sync_live_projection` 随后将 cursor 推到文件长度。下一次追加读取从 JSON 中间开始。一条合法事件被拆成两条非法 JSON，消费者会跳过，直到重建投影才能恢复。小日志的 `append_relevant_rows` 也有读到半行后跳过、仍缓存文件长度的同类风险。

实际 recorder 用 `serde_json::to_writer` 再单独写换行，读写边界不能假设天然对齐。抽取当前生产函数在临时文件复现：权威文件最终为一行 `{"source":"game","sequence":1,"payload":42}`，投影却把 `42}` 放到了下一行。

建议：游标只提交到最后一个完整换行，保留未完成尾部；区分不完整尾部、解析错误和 I/O 错误。补充半行写入、进程中断与重放一致性测试。另需核对 durable 承诺：`append_json_line` 当前没有 fsync；进程可见与掉电持久性应有明确约定。

定位：[gateway.rs:513](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:513)、[gateway.rs:562](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:562)、[gateway.rs:598](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:598)、[lib.rs:1056](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/lib.rs:1056)。

### 2. 详情与回放缺少统一请求生命周期〔隔离复现〕

从当前 `page.tsx` 抽取实际 effect/函数，使用可控制完成顺序的 fetch，得到：

| 场景 | 实际结果 | 影响 |
|---|---|---|
| sequence 1 请求慢、2 请求快，先返回 2 再返回 1 | 最终展示 sequence 1 | 画面、分数或目标倒退 |
| lease 丢失，summary 变为非 live，但 sequence 不变 | 没有重新请求详情 | 大盘与详情可能给出不同运行状态 |
| 首次详情请求失败，后续没有新事件 | 仅请求一次，连接仍为 live，详情为空 | 持续停在“正在接入测试”；SSE keepalive 不触发重试 |
| 网关发送合法 JSON 的 error 消息 | runs 被清空，连接仍为 live | 故障被显示成在线、无运行 |
| 回放加载中调用“返回直播”，然后旧请求完成 | 又选中旧回放 | 用户选择被异步返回覆盖 |

建议：每个 run 采用可取消、只接受最新结果的请求流程；详情 revision 包含 lifecycle/lease；失败显示可重试状态并做有界退避，不能依赖游戏产生新事件。保留最后一份有效快照并标记过期，单独展示连接状态和数据新鲜度。直播、回看、历史尝试、导出使用明确状态转换。

定位：[page.tsx:388](/Users/zuozijian/3720-benchmark/observer-platform/app/page.tsx:388)、[page.tsx:419](/Users/zuozijian/3720-benchmark/observer-platform/app/page.tsx:419)、[page.tsx:782](/Users/zuozijian/3720-benchmark/observer-platform/app/page.tsx:782)。

### 3. 回放目录的公共模型不能正确覆盖所有游戏〔运行实测 + 代码根因〕

抽样 Operator 有 63 个 group、0 个 attempt，Kitchen 有 6 个 group、0 个 attempt。不是 HTTP 失败，接口返回了空目录内容。

`event_context` 主要读取 `/level/reference`、`/level/id` 或 `selected`，没有统一表达班次、场景等游戏语义。`replay_groups` 对 reference 缺失的记录用 `level:<id>` 创建分组，但填充 attempts 时又直接跳过 reference 缺失的记录，导致空分组。另一个独立问题是只为成功尝试或 overworld 建组，因此一个关卡若从未成功，其失败尝试不会出现在目录中，即使关闭“跳过失败尝试”。

建议：游戏 observer 明确提供稳定的 episode/attempt 标识、标题、边界与得分变化；公共层据此分组，保证每个已记录的可回放尝试都有入口。将“每次正向得分就封口”的规则与实时游戏的班次边界分开；累计分数与本次增分也应分字段，避免累计 score 被 UI 渲染成 `+score`。

定位：[gateway.rs:1229](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:1229)、[gateway.rs:1370](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:1370)、[gateway.rs:1418](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:1418)、[page.tsx:1190](/Users/zuozijian/3720-benchmark/observer-platform/app/page.tsx:1190)。

### 4. 生产 release 依赖工作区，当前链接已失效〔文件实测〕

LaunchAgent 的 WorkingDirectory 是 `~/.local/share/3720-benchmark-live/current`。该 release 的 `node_modules` 是指向工作区 `observer-platform/node_modules` 的软链接，目标当前不存在，`node_modules/vinext/dist/cli.js` 也不存在。运行中的网页进程仍能返回 200；无法据此保证重启后还能启动。

发布脚本先切换 current 再重启，失败后没有自动恢复旧链接，只检查 gateway health，没有网页与 SSE 的完整就绪验证。建议发布包固定运行依赖，候选版本先做独立端口健康检查，再切换，并提供可验证的回滚路径。不要把当前进程存活等同于发布包完整。

定位：[publish-local.sh:40](/Users/zuozijian/3720-benchmark/observer-platform/scripts/publish-local.sh:40)。

## 第二优先级：更新成本和观赛表达

### 5. SSE 与详情仍承担过多历史数据〔运行实测 + 代码分析〕

- 18 秒公网 SSE 样本：1 个数据事件、1 次 keepalive，共 109,944 bytes。首包包含 48 条 run 和 1,074 个分数点。它不是持续每秒发送；有新 revision 时仍会重新发送完整大盘。
- 普通 runs 响应 gzip 传输约 20.5 KB，单次耗时约 1.06 秒；SSE 首字节约 0.27 秒。仅为当前机器一次测量，不能作为 P95 或负载测试结果。
- 浏览器传入 `run_id`，但当前 Rust `subscribe` 不解析该参数；所有订阅拿到同样的完整 runs。共用序列化缓存是已有优点，但全历史仍随更新重复传输。
- 网关详情缓存只有 4 个 run；失效时在全局 details mutex 内读取、排序、重建整个 run，recent_agent_rows 每次最多重读 32 MiB。异步 handler 内这些同步 I/O/CPU 操作存在并发阻塞风险。
- summary 构建同样遍历历史 game 事件；materialize_game_event 会读取 state_snapshot 对象。因此“只拉增量字节”不等于“只做增量投影”。

建议：大盘初始快照与变更通知分离，历史曲线独立缓存；详情拆开状态、回放目录、活动、经验的 revision。网关按 chain 增量维护投影、按 chain 控制并发，重计算移出请求执行路径。以真实长 run、多个同时观看的 run 和慢客户端进行定量验证后再确定缓存预算。

定位：[gateway.rs:273](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:273)、[gateway.rs:374](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:374)、[gateway.rs:693](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:693)、[gateway.rs:935](/Users/zuozijian/3720-benchmark/tools/observer/runtime/src/gateway.rs:935)。

### 6. “当前时间”“当前状态”和回放时间需明确分开〔代码风险〕

Home 把 `snapshotNow` 和每条 run 的 `observed_at` 同时设为 SSE 的 `generated_at`；大盘调用 `taskDuration` 时两者相同，所以 liveTail 为 0。无事件时只有独立 WallClock 更新，累计运行与未得分时间不会持续前进。后端 `consumed_ms` 又取最后一条事件的有效时间，不能简单改成网页墙钟计时，否则会把暂停算进去。

`followingLive` 仅检查是否选择历史 attempt，不检查 cursor 是否停在旧帧；回看当前批次旧帧仍可能显示“位于最新状态”。建议服务端提供明确的 active execution anchor，浏览器仅在执行窗口内插值；当前状态、停留旧帧、历史回放与数据过期分别显示。

定位：[page.tsx:200](/Users/zuozijian/3720-benchmark/observer-platform/app/page.tsx:200)、[page.tsx:426](/Users/zuozijian/3720-benchmark/observer-platform/app/page.tsx:426)、[page.tsx:705](/Users/zuozijian/3720-benchmark/observer-platform/app/page.tsx:705)、[page.tsx:1144](/Users/zuozijian/3720-benchmark/observer-platform/app/page.tsx:1144)。

### 7. 观赛主流程缺少足够的选择与解释〔基于数据和代码，待视觉验证〕

建议将首页默认范围收敛到正在运行与近期有进展的 run；其余按模型、配置、版本和日期检索。对比时允许选择少量曲线，明确比较的是模型还是一次实验。详情首屏集中回答：正在做什么、最近发生什么、为何停住、数据有多新；Agent 活动目前必须选择历史尝试才会加载，不能回答实时观看时“Agent 现在在做什么”。

已有的可见消息与显式笔记可以继续使用；不需要展示隐藏推理。视觉方案、移动端排布、焦点顺序与对比度仍需完成真实浏览器截图和交互检查后确定。

## 抽样数据

每个游戏取 API 列表中第一条记录；下面的“实时帧”是 `live_replay.frames`，不是历史回放是否存在的判定。Sausage 样本没有 Agent，不把无回放当成故障。

| 游戏 | 详情未压缩大小 | groups | attempts | 实时帧 |
|---|---:|---:|---:|---:|
| Parabox | 314,403 B | 121 | 138 | 25 |
| Operator | 183,730 B | 63 | 0 | 0 |
| Sokoban | 41,591 B | 101 | 239 | 6 |
| Kitchen | 33,426 B | 6 | 0 | 0 |
| Minesweeper | 11,285 B | 13 | 102 | 0 |
| Swarm | 27,007 B | 0 | 0 | 0 |
| Sausage | 101,882 B | 0 | 0 | 0 |

本机样本详情请求耗时约 0.004–2.46 秒；缓存冷热未控制，不据此作游戏性能排名。具体样本 ID 见同目录 measurements.json。

## 验证结果与复现

- `cargo test --manifest-path tools/observer/runtime/Cargo.toml --lib`：22/22 通过。
- `node --test tests/edge-proxy.test.mjs tests/publish-local.test.mjs`：edge 4/4 通过，publish 失败。工作区缺少 dist 和 node_modules，脚本前置检查即退出；这不是证实发布实现回归。没有安装依赖或为调研重建生产包。
- 现有 rendered-html 测试大量使用源码正则，只能证明某段代码存在，不能覆盖异步完成顺序、空回放目录、状态转换或实际视觉效果。
- `frontend-probes.cjs` 抽取当前前端函数，在 VM 中控制请求完成顺序，输出上述五种异常。它是诊断脚本，不是浏览器端到端测试。
- `partial-projection.rs` 保留当前投影函数的隔离复现，在 `/tmp/3720-live-audit-*` 写入虚构测试数据，不读取真实 journal。

在仓库根目录可复现：

```sh
node results/observations/live-platform-research-2026-09-06/frontend-probes.cjs
rustc --edition=2024 --crate-name live_partial_probe results/observations/live-platform-research-2026-09-06/partial-projection.rs -o /tmp/3720-live-audit-partial
/tmp/3720-live-audit-partial
```

## 建议实施顺序与验收

1. **可靠性修复。** 先补半行尾读、请求乱序、返回直播、同 sequence 状态变化、无新事件时失败恢复的行为测试；修复后确保事件不漏、详情不倒退、用户选择不会被旧请求覆盖。同时使生产 release 的依赖完整可重启。
2. **统一游戏回放契约。** 为七个游戏建立小型真实事件样本，覆盖成功、失败、进行中、切关/班次及缺少 reference；每个合法尝试都有可点击入口，目录与接口返回一致。
3. **降低更新成本。** 测量长记录、多个观看者、频繁切换、慢网络下的响应时间、CPU、内存和传输量。先建立基线，再定目标；验收包括数据不变时不重传历史、同 run 请求合并、不同 run 不被同一重计算锁串行拖住。
4. **观赛体验调整。** 完成大盘→详情→失败回放→返回直播→断网恢复的截图与交互检查，再做筛选、对比、实时活动、数据新鲜度与移动端布局。导出补充取消/卸载/切换测试，并核对帧时序与内存上限。

首批工作应聚焦第 1、2 步，形成可独立验收的修复；界面改版建立在正确、可恢复的数据与状态上。
