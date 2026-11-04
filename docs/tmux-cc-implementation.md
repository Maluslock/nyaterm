# tmux -CC 实现说明（开发文档）

给要改这块代码的人。读完应该能做到四件事：把任何一处行为定位到具体文件与函数；知道哪些地方不能动（动了会静默坏）；自己加一个 tmux 通知；在没有图形界面的机器上用日志把问题定位到行。

配套文档：

| 文档 | 给谁 |
| --- | --- |
| `docs/tmux-cc-implementation.md`（本文） | 改代码的人 |
| `docs/tmux-cc-gap-analysis.md` | 想知道「和 WindTerm 还差什么」「为什么这么设计」 |
| `docs/tmux-cc-test-plan.md` | 手动验收的人（每条含操作 / 期望 / 客观判定 / 失败看哪行日志） |
| `docs/verification/tmux-cc-protocol-notes.md` | 实测出来的协议事实（有三条反直觉） |
| `docs/tmux-command-cheatsheet.md` | 用户向：tmux 命令行里能敲什么 |

## 1. 一句话架构

tmux 在控制模式（`tmux -CC`）下不再把界面渲染给终端，而是把**协议文本**混在 pane 输出流里发回来。nyaterm 在「输出合并器」的最前面把这股流截下来解析成结构化状态，前端拿这份状态自己画一套 pane 镜像；键盘输入再以 `send-keys` 逐字节发回目标 pane。

```
        ssh / pty / local / telnet / serial 的字节流
                          │
        SessionOutputCoalescer::push_owned  (src-tauri/src/core/output.rs:189)
                          │  gateway.filter(text)  ← 网关先看，消费掉的字节不排队、不上屏
                          ▼
   ┌──────────────── TmuxGateway (src-tauri/src/core/tmux/gateway.rs) ────────────────┐
   │ 检测 ESC P 1000 p → 激活                                                        │
   │ parse_line (protocol.rs) → %output / %layout-change / %window-* / %begin|%end    │
   │ 维护 windows / panes / layout、命令应答缓冲、capture 回放缓冲                     │
   │ 需要问 tmux 时：write_command → SessionCommand::Write{raw:true} 写回同一会话      │
   └──────────┬─────────────────────────────────────────┬─────────────────────────────┘
              │ tmux-state-<sid> (快照)                  │ tmux-pane-output-<sid>
              ▼                                         ▼
      useTmuxGateway.ts                          TmuxPaneTerminal（每 pane 一个 xterm）
              │                                         ▲
              ▼                                         │ tmux_gateway_capture_pane
      TmuxGatewayView（布局镜像 / 窗口条 / 命令行）      │ （xterm 挂载时回放历史画面）
```

输入方向：

```
xterm onData ──► tmux_gateway_input(paneId, data) ──► send_pane_input ──► send-keys -H -t %N
应用级输入（快捷命令 / 粘贴 / 脚本 / 启动命令）
      ──► SessionCommand::Write ──► forward_write_to_tmux_pane
            └─ 网关活跃且 origin 不是网关自己 ──► 改投「活动 pane」，否则才写进会话 stdin
```

关键点：控制模式下**写进会话 stdin 的字节会被 tmux 当命令解析**（`ls -la` → `parse error: usage: list-sessions`）。所以「用户想输入的东西」必须绕道 `send-keys`，只有网关自己的命令才走 stdin。

## 2. 后端

### 2.1 文件与职责

| 文件 | 职责 |
| --- | --- |
| `src-tauri/src/core/tmux/gateway.rs`（约 1400 行） | 网关本体：检测、激活、状态维护、命令下发、快照发布、事件发送 |
| `src-tauri/src/core/tmux/protocol.rs`（约 375 行） | 把一行控制模式文本解析成 `TmuxNotification`；`%output` 的八进制反转义（字节版 + lossy 版） |
| `src-tauri/src/core/tmux/layout.rs`（约 355 行） | tmux 布局串解析（`b25e,80x24,0,0,1`、`{...}` 链式切割）→ 递归树 |
| `src-tauri/src/core/tmux/types.rs`（约 147 行） | `TmuxWindow` / `TmuxPane` / `TmuxLayoutNode` / `TmuxGatewaySnapshot` / `TmuxGatewayMessage` |
| `src-tauri/src/core/tmux/integration_tests.rs`（约 440 行） | 起**真的 tmux**（PTY）跑控制模式，断言检测、布局、pane id、命令应答、窗口增删 |
| `src-tauri/src/core/tmux/mod.rs` | 模块导出 |

接入点（这块功能没有集中的初始化代码，分散在四条会话路径上）：

| 接入点 | 做什么 |
| --- | --- |
| `src-tauri/src/core/output.rs:82` | `SessionOutputCoalescer::for_app` 为每个会话取一个网关并挂到合并器（`with_gateway`）；`filter` 里网关**最先**看字节 |
| `src-tauri/src/core/output.rs:106` | `tmux_gateway()`：让会话路径拿到网关去设编码 |
| `src-tauri/src/core/ssh/io.rs`、`terminal_session/{local,serial,telnet}/session.rs` | 创建合并器后立刻 `gateway.set_encoding(&encoding)` |
| `src-tauri/src/core/ssh/io.rs` 的 `SessionCommand::Write` 分支 | `forward_write_to_tmux_pane(app, session_id, origin, data)`：应用级输入改投 pane |
| `src-tauri/src/cmd/tmux.rs` | 6 个 Tauri 命令（snapshot / input / resize / capture_pane / run_command / command） |
| `src-tauri/src/lib.rs:53` | `TmuxGatewayManager` 建一次，`.manage()` 进全局状态；6 个命令注册在 `generate_handler` |

### 2.2 生命周期

1. **挂上（未激活）**：会话建立时 `SessionOutputCoalescer::for_app` 无条件取网关挂上。日志 `terminal output coalescer created { gateway_attached: true }`。注意 `gateway_attached=false` 表示从 app state 拿不到 manager（配置问题）；`true` 但后面没有 `detected` 行，表示这条连接上 tmux 没发标记（没进控制模式）。
2. **等标记**：未激活时 `filter` 把字节原样交还终端，但**保留末尾最多 6 字节**（`MARKER_MAX_HOLD = CONTROL_MARKER.len() - 1`），防止 `ESC P 1000 p` 被网络分包切成两半。
3. **激活**：看到 `ESC P 1000 p` → `active = true`，标记**之前**的字节交还终端，标记**之后**的字节开始当协议流；立刻发 `list-windows` / `list-panes` 拿初始结构（`windows_command` / `panes_command`）。
4. **活跃**：所有字节都进 `parse_line`，`filter` 返回 `None`（终端什么都收不到，包括用户按的键——见下面的输入路径）。
5. **退出**：`%exit` → 停用、清空 windows/panes、把残余尾巴交还终端，并推一版 `exited: true` 的快照；前端据此退回普通终端并把焦点还给 xterm。

### 2.3 对外方法

| 方法 | 用途 |
| --- | --- |
| `filter(&self, text: &str) -> Option<String>` | 吃协议流；`None` = 全被消费，`Some(tail)` = 交还终端 |
| `snapshot(&self) -> TmuxGatewaySnapshot` | 当前完整状态（前端初次加载与兜底用） |
| `set_encoding(&self, encoding: &str)` | 会话编码（GBK 等）；变了就清每个 pane 的解码器流状态 |
| `resize(&self, width, height)` | `refresh-client -C WxH` |
| `active_pane_id()` / `pane_for_app_write(origin)` | 目标 pane 决策（后者排除 `InputOrigin::TerminalResponse`，即网关自己的命令） |
| `send_pane_input(pane_id, data)` | 用户键入 → `send-keys -H -t %N` |
| `capture_pane(pane_id)` | 请求某 pane 的当前可见画面（`capture-pane -p -e -t %N`），结果走 pane 输出事件（带 `replay` 标记） |
| `run_command_with_reply(command, request_id)` | 需要回显的命令行：带标签包一层，应答按 `request_id` 配对 |
| `run_ui_command(command)` | 不需要回显（改名、布局、缩放等） |
| `is_active()` | 是否在控制模式 |

### 2.4 命令应答怎么配对

tmux 对每条命令回 `%begin <time> <num> <flags>` … `%end` / `%error`。同一时刻可能有多条命令在飞（初始化两条、capture 一条、用户命令行一条），所以不能按顺序配对。做法是：UI 命令包成

```
display-message -p 'NYATERM-REPLY:<id>'; <用户命令>; display-message -p 'NYATERM-REPLY-END:<id>'
```

（三条独立行，见不变量 3），解析时把 `NYATERM-REPLY:<id>` 与 `NYATERM-REPLY-END:<id>` 之间的行收进该 `request_id` 的缓冲，`%error` 记 `error`。结果通过 `tmux-command-response-<sid>` 事件回前端（`TmuxCommandResponse { request_id, output, error }`）。

### 2.5 客户端尺寸

控制模式下每有客户端尺寸变化，tmux 会重排并向所有 pane 发 `%layout-change`。前端在布局容器上挂 `ResizeObserver`，算出单元格尺寸后 `refresh-client -C WxH`；**只在尺寸真的变了才发**（去重在 `TmuxGatewayView.tsx` 的 `lastClientSizeRef` / `reportClientSizeRef`）。没有单元格尺寸时不能发——按 80x24 猜会把 tmux 尺寸改错。

### 2.6 编码

`%output` 里是**原始字节**（八进制转义），不是 UTF-8 文本。`protocol.rs` 的 `unescape_output_bytes` 先还原字节，网关再按 pane 用 `TerminalOutputDecoder`（和普通终端同一套）流式解码——多字节字符被拆到两条 `%output` 里也不会碎（`a_burst_of_pane_output_is_merged_per_pane` 与 `pane_output_is_decoded_with_the_session_encoding` 两个测试锁住）。capture 回放走同一条解码路径。

## 3. 协议通知（`parse_line` 支持的全部）

| 行 | 触发 | 处理 |
| --- | --- | --- |
| `%output %N <data>` | pane 有输出 | 反转义成字节 → 按 pane 累积 → 同批 flush 合并 → 解码 → `tmux-pane-output-<sid>` |
| `%begin …` / `%end …` / `%error …` | 命令块边界 | 切分应答；带 `NYATERM-REPLY:` 标签的进 `request_id` 缓冲 |
| `%layout-change @W <layout> <visible> <flags>` | 分屏 / 改尺寸 / 切布局 | 重解析布局，更新窗口的 `layout`，置脏 |
| `%window-add @W` / `%window-close @W` / `%window-renamed @W name` / `%window-pane-changed` | 窗口表变化 | 增量改窗口表，置脏 |
| `%session-changed $N name` | 会话名 | 记进快照 |
| `%message <text>` | tmux ≥3.3 的提示 | 存 `TmuxGatewayMessage{text, sequence}`，前端 6 秒淡出 |
| `%exit [reason]` | 控制模式结束 | 停用 + 清状态 + 尾巴交还 + 推 `exited: true` |
| 其他 `%...` | —— | 忽略（不报错，前向兼容） |

窗口信息的来源是 `list-windows` 的格式化输出，字段固定 8 个：

```
#{window_id}\t#{window_index}\t#{window_name}\t#{window_active}\t#{window_zoomed_flag}\t#{window_activity_flag}\t#{window_bell_flag}\t#{window_layout}
```

`apply_window_line` 要求 ≥8 字段——少字段说明 tmux 版本或格式被改坏了，宁可这行不生效也不要错位解析。

## 4. 前端

### 4.1 事件与封装

`src/lib/tmuxGateway.ts` 是唯一允许拼事件名的地方：

| 导出 | 用途 |
| --- | --- |
| `tmuxStateEvent(sid)` → `tmux-state-<sid>` | 快照（`TmuxGatewaySnapshot`） |
| `tmuxPaneOutputEvent(sid)` → `tmux-pane-output-<sid>` | `{ paneId, data, replay }` |
| `tmuxCommandResponseEvent(sid)` → `tmux-command-response-<sid>` | `TmuxCommandResponse` |
| `fetchTmuxSnapshot` / `tmuxGatewayInput` / `resizeTmuxClient` / `requestTmuxPaneCapture` / `runTmuxCommandWithReply` / `runTmuxUiCommand` | 对应的 Tauri 命令 |

`src/hooks/useTmuxGateway.ts` 负责：首次 `fetchTmuxSnapshot` + 订阅 `tmux-state-*`，把快照存进 state。

### 4.2 组件

| 组件 | 职责 | 注意 |
| --- | --- | --- |
| `TmuxPaneHost.tsx` | 在普通终端与 tmux 视图之间切换 | 普通终端**保持挂载**（`inert`），否则退回时 shell 的现场没了；切走/切回时把焦点还给 `textarea.xterm-helper-textarea` |
| `TmuxGatewayView.tsx`（约 1060 行） | 布局镜像、窗口条、命令行、前缀模拟、右键菜单、分隔条 | 见 4.3；挂载在 `src/components/terminal/PaneWorkspace.tsx:441` |
| `TmuxPaneTerminal.tsx`（约 580 行） | 每个 pane 一个 xterm | 挂载时 `capture-pane` 回放（**每个 xterm 实例只回放一次**）；复用 app 的高亮 / 行号时间戳 gutter / 命令补全 / 查找 |
| `TmuxWindowList.tsx` | `Ctrl-b w` 的窗口/窗格列表 | 光标先落在活动 pane 那行；行数变化时 clamp 选中项 |
| `src/lib/tmuxPaneInput.ts` | 键入 → `send-keys` 载荷 | 单字符/退格/方向键/粘贴的编码规则 |
| `src/lib/tmuxPaneResize.ts` | 拖拽像素 → 单元格增量 | `nextPaneResize({paneId, axis, deltaPx, cellWidth, cellHeight, sentCells})` |

### 4.3 状态从哪来

- **唯一真相是后端快照**。前端不自己推断窗口表、pane 尺寸、活动 pane。
- 前端只持有一小撮瞬态：拖拽中的像素偏移、命令行输入与历史、前缀 pending 键、`capture` 回放去重标记（按 xterm 实例）。
- 任何「前端算完再发命令」的地方（改名、布局、缩放、分隔条）都是先发命令、**等快照**，不做本地乐观更新——避免和 tmux 的真实状态打架。

## 5. 关键不变量

这些是踩过的坑，改动时当约束读。

| # | 不变量 | 出处 | 违反会怎样 |
| --- | --- | --- | --- |
| 1 | 布局串叶子里的数字是 pane **id** 的数字部分（`%12` → 12），不是 `#{pane_index}` | `layout.rs:apply_pane_ids` + `gateway.rs:apply_pane_line` | pane 全黑（id 对不上，输出发给了不存在/错误的 pane） |
| 2 | 控制模式只推**新**输出，历史必须 `capture-pane` 回放 | `gateway.rs:capture_begin/screen/end_command` | 后接入的 pane 空白 |
| 3 | tmux 3.2 解析不了「带 `capture-pane` 的 `;` 串联行」（`%error parse error: syntax error`） | `gateway.rs` 里 capture 的三条独立命令 | capture 永远不返回 |
| 4 | 写进会话 stdin 的字节 = tmux 命令；用户的输入必须改投 pane，且 `SessionCommand::Write { raw: true }` | `ssh/io.rs:forward_write_to_tmux_pane`、`gateway.rs:write_command` | 用户敲的命令被 tmux 当命令执行（`ls -la` → parse error）；GBK 会话被重编码 |
| 5 | 每个 xterm 实例只回放一次 capture | `TmuxPaneTerminal.tsx` 的 `capturedPaneRef`（按实例） | 每帧都回放 → 画面闪、重复 |
| 6 | `refresh-client -C` 只在尺寸变化时发 | `TmuxGatewayView.tsx` 的 `lastClientSizeRef` | tmux 反复重排 + 全量快照风暴 |
| 7 | 前缀期间忽略纯修饰键、IME `Process`/`Unidentified`/`isComposing`、非单字符 | `TmuxGatewayView.tsx` 的 keydown 处理 | `ProcessProcess…`、全角 `：` 被当命令打进 shell |
| 8 | 分隔条拖拽必须自愈（`buttons === 0` 算松手；监听 pointercancel/blur） | `startDividerDragFor` | 拖到窗口外松手后永久卡住（此后鼠标一动就 resize） |
| 9 | leaf 上 `<TmuxPaneTerminal key={pane.id ?? `leaf-${pane.index}`}>` | `TmuxGatewayView.tsx` 的 `renderNode` | React 按位置复用 xterm → 旧 pane 的 scrollback/alt-screen/搜索串到新 pane |
| 10 | 前缀键的 keydown 不能吞掉自家输入框（INPUT/TEXTAREA，排除 `xterm-helper-textarea`） | 同上 | 改名输入框里 `Ctrl-b` 被吃、打不了字 |

## 6. 两条完整路径（照着读代码）

### 6.1 键入一个字符

`TmuxPaneTerminal` 的 xterm `onData` → `applyTmuxPaneInput`（`src/lib/tmuxPaneInput.ts`）编码 → `tmuxGatewayInput(sessionId, paneId, payload)`（`cmd/tmux.rs:30`）→ `TmuxGateway::send_pane_input` → `send-keys -H -t %N <hex…>` 写回会话 stdin → tmux 把按键交给该 pane → pane 回显 → `%output` → 事件 → 对应 xterm。

应用级输入（快捷命令、粘贴、`StartupCommand`）走另一条：`SessionCommand::Write` → `forward_write_to_tmux_pane`：网关活跃且 `origin != TerminalResponse` → `send_pane_input(active_pane)`，否则原样写。

### 6.2 pane 输出到屏幕

`filter` 收到 `%output %N <octal>` → `unescape_output_bytes` → `Inner.pane_outputs` 累积 → `flush()` 里按 pane 合并连续段 → `decode_pane_outputs`（会话编码）→ `tmux-pane-output-<sid>` 事件 `{ paneId, data, replay: false }` → `TmuxPaneTerminal` 订阅 → `term.write(data)`。日志：`tmux pane output forwarded { pane_id, bytes, chunks, replay }`。

### 6.3 新 pane 出现

`%layout-change` / `%window-pane-changed` → 重解析布局 → `mark_state_dirty` → 推快照 → React 渲染新 `TmuxPaneTerminal` → 挂载后 `requestTmuxPaneCapture(sessionId, paneId)` → 网关发三条 capture 命令 → 回放内容以 `%output` 形式回来（`replay: true`，日志里 `replay=true`）→ 写进该 xterm。

## 7. 加一个新通知（六步）

1. `protocol.rs`：`TmuxNotification` 加变体 + `parse_line` 加分支 + 单测（`%foo bar` 的边界）。
2. `gateway.rs`：在 `handle_notification`（或 `apply_*`）加分支，更新 `Inner`。
3. 影响快照 → `mark_state_dirty()`（否则前端看不到）。
4. 需要新事件 → `TmuxGatewaySink` 加默认方法 + `AppGatewaySink` 实现（事件名 `tmux-<what>-<sid>`）+ `src/lib/tmuxGateway.ts` 加封装。
5. 前端消费（新组件或现有组件加分支）。
6. `integration_tests.rs` 加真 tmux 断言——**没有真 tmux 断言的协议分支不算做完**。

## 8. 测试

| 命令 | 覆盖 |
| --- | --- |
| `cargo test --lib core::tmux` | 32 个：协议解析、布局、编码、合并、命令应答、以及 `integration_tests` 里起真 tmux 的全链路 |
| `npx vitest run src/components/terminal/tmux src/lib/tmux` | 前端 tmux 相关（视图、宿主、窗口列表、输入、缩放） |
| `npx vitest run` | 全量（约 926 个 / 145 文件） |
| `npx tsc --noEmit` / `pnpm lint` / `pnpm i18n:check` | 类型、风格、语言包键序 |

`integration_tests.rs` 需要本机有 tmux 与可用的 PTY；它用一个独立 socket（`tmux -L <名字>`），不会碰用户的 tmux server——但**别对它跑 `tmux kill-server`**（会杀掉你自己的会话，见验收手册的注意事项）。

## 9. 调试（用户在 Windows 上只看得到日志）

日志文件：Linux `~/.local/share/com.kang.nyaterm/logs/nyaterm-diagnostics.<日期>.jsonl`；portable 包在解压目录的 `data/logs/` 下。按关键字过滤：

| 关键字 | 说明 |
| --- | --- |
| `terminal output coalescer created` | 每会话一行；`gateway_attached` 说明网关有没有挂上 |
| `tmux control mode detected; gateway engaged` | 检测到 `ESC P 1000 p`，进入控制模式 |
| `tmux gateway state published` | 每次推快照；`windows` / `panes` / `ids` 能看出是不是空的 |
| `tmux pane output forwarded` | `pane_id` / `bytes` / `chunks` / `replay` |
| `tmux gateway message` | `%message`（tmux ≥3.3 才有） |
| `tmux sent an unparsable layout string` | 布局串解析失败（粘上原文） |
| `tmux protocol line exceeded the buffer cap; discarding` | 单行超过上限，协议流已不可信 |
| `tmux control-mode client exited` | `%exit`，退回普通终端 |

排查顺序建议：先看有没有 `detected` 行；有就看 `state published` 的 `panes` 数量是否等于 tmux 里的 `#{window_panes}`；再对单个 pane 找 `output forwarded` 有没有 `pane_id`；`bytes` 有但屏幕空 → 看 `replay` 与前端控制台。

## 10. 已知限制（刻意不做，别当 bug 修）

1. **capture 回放与实时输出的先后竞争**：回放请求发出后、内容回来前，pane 可能又产生了新输出。客户端没有正确解法（丢回放会丢整屏，缓存重放会重复），顺序错乱是概率性的、以屏幕少量行重复/缺失表现。要根治得改 tmux 侧或加序列号确认。
2. **前缀键是前端模拟的**，不是 tmux 的 prefix：只映射了 `d c n p % " z x [ : ;`，其余组合不拦截（宁可漏，不错吞）。
3. **`%message` 只有 tmux ≥3.3 发**；3.2 上永远不会出现，不是没实现。
4. **`tmux -CC` 总是新建会话**；要接管已有会话得敲 `tmux -CC attach -t <名字>`（这是 tmux 的行为，不是 app 的）。
5. **窗口列表 / 缩放下拖拽不生效**：tmux 缩放时 pane 尺寸由 tmux 决定，拖拽只发 `resize-pane`，会立刻被 tmux 还原。

## 11. 发布与打包

- 产物可辨识版本：`src-tauri/src/observability.rs` 的 `BUILD_TAG`（出现在日志 `app.start` 行）。改完代码要发测试包，先改这个再构建——交叉编译十几分钟，别构建完再改。
- Windows 包：`scripts/build-windows-test-package.sh`。三个必须满足的条件写在脚本头部注释里：`--features tauri/custom-protocol`（否则前端资源不内嵌，白屏）、crt-static 用 target 级 `--config`（全局 `RUSTFLAGS` 会破坏 host 侧 proc-macro 构建）、ConPTY 资源先由 `pnpm prepare:conpty` 准备好（Linux 上手动解包时按 `src-tauri/conpty-package.json` 的布局与 sha256 核对）。
- 分支与远端：工作分支 `spike/tmux-cc-gateway-detection`，推到自己的 fork `fork` remote；上游是 `origin`（`nyakang/nyaterm`）。合上游后注意 `SessionCommand::Write` 有 `raw: bool` 字段。
