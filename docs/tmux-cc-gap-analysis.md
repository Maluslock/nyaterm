# tmux -CC：现状与 WindTerm 的差距

分支 `spike/tmux-cc-gateway-detection`，2026-09-28。

证据说明：本仓这一列来自代码与真机验证（Linux + 真 SSH + tmux 3.2a）。
WindTerm 的 tmux 集成**不在其开源仓内**（`WindTerm/src` 只有 libssh / Onigmo / Protocols / Pty /
Utility / Widgets，README 仅一句 “Supports tmux integration”），所以那一列基于其文档与使用者
体验，不是代码级比对。

## 已经对齐（核心闭环）

| 能力                     | 我们的实现                                                                                                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| 输入 `tmux -CC` 自动接管 | 检测 DCS `ESC P 1000 p`（7 字节，无 ST）；协议流在 coalescer 里就被吃掉，不落到 xterm（否则 xterm 会永久停在 DCS 状态）                            |
| tmux 窗口 → 窗口切换条   | 位于 pane 区域**下方**（WindTerm 同款位置），点击 / `Alt+[`、`Alt+]` 切换，`+` 新建，`×` 关闭                                                      |
| tmux pane → 原生分屏     | 按 `#{window_layout}` 镜像几何，每个 pane 一个本地 xterm；点击选中，`Alt+方向键` 按方向 `select-pane`                                              |
| 键盘输入                 | `send-keys -H -t %N` 逐字节发送（控制字符 / ESC 序列 / UTF-8 原样到达）                                                                            |
| 客户端尺寸               | 容器实测后 `refresh-client -C WxH`，布局由 tmux 决定（含 gutter 宽度扣除）                                                                         |
| tmux 前缀键              | 控制模式下前缀到不了 tmux 客户端（按键是发给 pane 的），因此视图模拟前缀：`Ctrl-b d/c/n/p`、`%`、`"`、`[`；未模拟的组合原样转发给 pane             |
| 分离 / 退出              | `%exit` 后网关停用并清空状态（同一会话可再次 `tmux -CC`），剩余字节交还终端；普通终端始终挂在视图下（inert），提示符不丢；视图消失时把焦点交回终端 |
| pane 画面回放            | `capture-pane -p -e` 回放当前屏幕（BEGIN/END 标签框定），解决"attach 到已有内容 / 提示符先于视图画出"导致的空 pane                                 |
| 附带能力                 | pane 里同样有语法高亮、行号栏、命令补全（nyaterm 自有能力）                                                                                        |

## 还差的（按价值/工作量排序）

### A. 可点的操作面（已补齐）

分屏按钮（`-h`/`-v`）、窗口重命名（双击窗口条）、缩放、`select-layout` 下拉、分离、
`Ctrl-b :` 命令行（带预设与 ↑/↓ 历史、回答与 `%error` 回显）、pane 右键菜单
（查找 / 分左右 / 分上下 / 缩放 / 上下交换 / 独立成窗 / 关闭）。窗口条还会画 bell 与 activity 标记。

仍缺：窗口列表浮层（`choose-tree` 的自绘版）——窗口条已经能点，浮层只是长列表下的便利。

### B. 拖分隔条改 pane 大小（已完成）

分隔条把像素位移换算成单元格后发 `resize-pane -t %N -y/-x <n>`，布局仍以 tmux 为准回填；
分隔条可 Tab 聚焦，方向键按格调整（`aria-valuenow` 报告比例）。
真机验证：向上拖 43px 后 `%0` 15→13 行、向下拖回 15/14，镜像布局同步跟随。

### C. 窗口状态可视化（已完成）

- 窗口条有 bell / activity / zoomed（`resize-pane -Z` 按钮按下态）
- `%message` 现在随快照下发（带递增序号，重复消息也算新消息），在窗口条上方淡出显示 6 秒
  ——注意 tmux 3.2a 二进制里没有 `%message` 字符串，本机无法造出该通知；3.3+ 才发
- 仍不画 tmux 状态行（WindTerm 可显示）

### D. pane 内的搜索（已完成）

pane 是本地 xterm，所以直接用 app 自己的搜索条与 SearchAddon：`Ctrl+Shift+F`
（即 `terminal.find` 绑定，可改键）或 pane 右键菜单打开，`Esc` 关闭，
支持大小写/整词/正则与命中计数，并复用"深度历史"（pane 的提交也会进会话历史）。
tmux 自己的 copy-mode（`Ctrl-b [`）仍然可用，但找东西不再必须进它。

### E. 与 nyaterm 自身能力的联动（未做）

- tmux pane 不参与"同步输入 / 广播"分组（普通终端的 `syncGroups` 到不了这里）
- 录制 / 转写、崩溃或重连后自动回到 tmux 视图：未接
- 会话恢复（`ui.open_tabs`）只恢复 SSH 会话，不恢复 tmux 视图

### F. 入口体验

敲 `tmux -CC` 会被自动识别并接管（已实现，无需菜单）；没有额外的提示或菜单项（此前约定不着急）。

## 2026-09-29 已补

- tmux 命令行（`Ctrl-b :` 或窗口条按钮）+ 命令回答/`%error` 回显
- app 级输入改投活动 pane（控制模式下写会话会被 tmux 当命令解析）
- `Ctrl-b` 后需要 Shift 的组合（`"`、`%`、`:`）修复：前缀期间忽略纯修饰键
- 分离/退出后焦点交还终端；普通终端在视图下持续挂着，提示符不丢
- 窗口条：bell / activity / zoomed 标记、双击重命名、缩放按钮、`select-layout` 下拉
- 分隔条可拖拽、可键盘调整（真机验证 15→13→15 行）
- pane 右键菜单（含"查找"），右键会像 tmux 一样先选中该 pane
- pane 内搜索（`Ctrl+Shift+F`）
- `%message` 随快照下发并在窗口条上方显示
- 终端实例被重建时 pane 画面会重新回放（此前会留下空白 pane）

## 怎么验证（本机可复现）

Linux + Xvfb `:99` + 本机 sshd（127.0.0.1:2222）+ `pnpm dev`（1420），debug 版按 deep link 打开：

```bash
cd src-tauri && DISPLAY=:99 ./target/debug/nyaterm \
  'nyaterm://connect/ssh?host=127.0.0.1&port=2222&username=user'
```

要点：X 窗口在 (160,100)、内容 1280x800，所以**前端坐标 + (160,100) 才是 xdotool 的屏幕坐标**；
拖拽/右键这类指针验证必须先换算，否则事件落在别的元素上（曾因此误判"拖拽无效"）。
`capture-pane` 之类的 tmux 查询用另一个 socket，别对 app 正在用的默认 socket 跑
`tmux kill-server`（会连带杀掉 app 的 control client）。

### 分支与上游（2026-09-30）

本分支已推到 fork：**https://github.com/Maluslock/nyaterm**（分支
`spike/tmux-cc-gateway-detection`，`main` 与上游 `1ef75d1bb` 一致）；本地 remote 名 `fork`。
给上游发 PR 的地址：
`https://github.com/nyakang/nyaterm/compare/main...Maluslock:nyaterm:spike/tmux-cc-gateway-detection`。

本机没有外网出口（`.73:7897` 的 mihomo 节点也全挂），但 `.73` **直连 github 是通的**，
所以取上游要借道：`ssh -D 127.0.0.1:1080 192.168.170.73`，然后
`git -c http.proxy=socks5h://127.0.0.1:1080 fetch origin`（cargo 依赖用
`CARGO_HTTP_PROXY` 同值；NuGet 会 302 到 nuget.azure.cn，curl 要带 `-L`）。

已把上游 `origin/main` 合并进本分支（`b72d2f7b` → `1ef75d1bb`，42 个提交、153 个文件，
版本号随上游到 v1.2.12），**零冲突**（重叠只有 `.gitignore`、4 个语言包、`core/mod.rs`、
`ssh/io.rs`、`lib.rs`）。合并后新增两件事要记：

- 上游给 `SessionCommand::Write` 加了 `raw: bool`（`terminal_session/mod.rs`
  的 `prepare_terminal_write_input`）：`raw=true` 跳过退格重映射与按会话编码转换。
  网关写回 tmux 的控制命令行必须是 `raw: true`，否则配了 GBK 的会话会把命令重编码。
- 上游把 ConPTY 运行时变成随包资源（`bundle.resources`），由 `pnpm prepare:conpty`
  （PowerShell）从 NuGet 拉 `microsoft.windows.console.conpty` 并按 sha256 校验。
  Linux 上交叉编译前要手动放好 `src-tauri/resources/windows/conpty/{x64,arm64}/…`，
  否则 cargo 构建在 `resource path … doesn't exist` 处失败（构建脚本已加前置检查与打包拷贝）。
- 上游 `main` 自己有 **19 个失败测试**（与并发/时序、cloud-sync 哈希、telnet 口令解析、
  录制输出格式有关）；本分支合并后失败名单与之逐字一致，即合并没引入回归。

### 验证时的坑（都是踩过的）

| 坑                  | 现象                                         | 规矩                                                                                                      |
| ------------------- | -------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| 坐标空间不同        | 点在分隔条上却毫无反应                       | 前端坐标 + 窗口位置 (160,100) = xdotool 屏幕坐标；先 `xdotool getwindowgeometry` 确认，别照抄上一次的数值 |
| 抢 tmux 默认 socket | app 的 tmux 视图突然 `%exit`，会话丢失       | 探测一律 `tmux -L <私有名>`；只有确认 app 没在用时才碰默认 socket                                         |
| `pkill -f` 自匹配   | 把自己的 shell / 后台任务一起杀掉（SIGTERM） | 模式加方括号（`vite.j[s]`），或先 `pgrep` 拿 PID、确认命令行、在**另一条**命令里 kill                     |
| 后台进程占住 stdout | 起服务的脚本一直不返回，工具调用超时         | 长驻进程用 `setsid ... >日志 2>&1 </dev/null &`，就绪用轮询端口判断，不用 `sleep`                         |
| 中途重启长构建      | 只为改一行常量就重跑 15 分钟交叉编译         | 影响产物的常量（如 `BUILD_TAG`）在**开跑前**改好；构建期间只做只读验证                                    |
| 从记忆里改文件      | `edit` 反复被拒（未读/已变），或补丁断言失败 | 改动前 `read` 目标片段；`read` 与 `edit` 之间不要跑 `prettier --write`；机械改写用脚本时也必须先重读      |

## 2026-10-13 review 批次：修掉的缺陷与已知限制

一轮对着代码逐文件 review（前端 + 后端各一条线）后修掉的东西，以及**刻意不修**的东西。
每条都附了"为什么会错"，方便以后回看不要又改回去。

### 前端（`src/components/terminal/tmux/`）

| 缺陷                                                                            | 为什么会错                                                                                                                 | 现在怎么做                                                                                                                                                                                           |
| ------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 分隔条拖到窗口外松手后，此后**再也拖不动**、而且不按键也一直在 resize           | 拖拽状态放在模块级 `let dividerDragActive`，收尾只挂 `pointerup`；指针在窗口外松开时事件收不到，标记永久为真且监听一直挂着 | `endActiveDividerDrag`（类型是「收尾函数或 null」）存当前收尾函数，新拖拽先收尾旧的；`onMove` 里发现 `buttons === 0` 就当松手；收尾同时摘 pointermove/mousemove/pointerup/mouseup/pointercancel/blur |
| 切 pane / 换布局后，旧 pane 的 scrollback、alt-screen 模式、搜索框"串"到新 pane | `<TmuxPaneTerminal>` 没有 `key`，React 按位置复用组件，xterm 实例被沿用                                                    | 给叶子节点加 `key={node.pane.id ?? \`leaf-${node.pane.index}\`}`（注意 `TmuxPane.index` 才有数字，leaf 节点本身没有）                                                                                |
| `refresh-client -C` 每次 cell metrics 变化都发，tmux 反复重排并回推全量快照     | 上报前不比较尺寸；而且首次上报经常早于 cell metrics 到达，之后不会补报                                                     | `lastClientSizeRef` + `reportClientSizeRef`：尺寸没变直接 return，`handleCellMetrics` 末尾补报一次，换会话时把记录清空                                                                               |
| 重命名/命令/查找输入框里按 `Ctrl-b`、`Alt` 组合被 tmux 前缀逻辑抢走             | window 上 capture 阶段无差别拦截                                                                                           | 焦点在 INPUT/TEXTAREA 且不是 `xterm-helper-textarea` 时不拦截                                                                                                                                        |
| 命令行（`Ctrl-b :`）关掉后键盘没回到 pane                                       | 关闭只是把输入框卸载，焦点落到 body                                                                                        | 记录命令行开关，关闭时 focus `[data-tmux-pane="…"] textarea.xterm-helper-textarea`                                                                                                                   |
| 窗口列表在别的客户端 kill-window 后，光标停在已不存在的行，回车变空操作         | `selected` 只在初始化时 clamp                                                                                              | `rows.length` 变化时 clamp；`onChoose(index)` 改成按索引回调，避免每次按键重挂 capture 监听                                                                                                          |

### 后端（`src-tauri/src/core/tmux/`）

- **pane 字节必须按会话编码解**：网关原来用 `String::from_utf8_lossy` 处理 `%output`，
  GBK 会话的中文全变 U+FFFD（普通终端路径是 `TerminalOutputDecoder::new(&encoding)`）。
  现在 `protocol.rs` 提供 `unescape_output_bytes() -> Vec<u8>`，`TmuxNotification::Output.data`
  是 `Vec<u8>`，网关内部按 pane 持有 `TerminalOutputDecoder`；`TmuxGateway::set_encoding()`
  在 SSH / 本地 / 串口 / 远端登录四条会话路径上都会接到真正的编码（改了编码会清掉流状态）。
  capture-pane 的回放也走同一条解码路径（先拼字节再解码）。
- **同一 pane 的连续输出合并**：一屏刷新会打出很多条 `%output`，逐条推给前端会造成大量小写入。
  网关按 pane 合并同批数据、跨 pane 保持顺序后再一次性下发（`chunks` 字段写进日志）。

### 已知限制（**不要**当成 bug 去"修"）

- **capture 回放与实时输出的顺序竞争**：回放请求发出后、响应回来前，新输出可能先到。
  客户端没有正确解：丢掉回放会缺整屏，缓冲重放会重复。当前实现保证"至少不丢"，偶发重复行。
- **`%message` 通知**：tmux 3.3 才引入（本机 3.2a 二进制里没有这个字符串），
  3.2 上永远不会出现；收到时按 6 秒淡出显示。
- **`tmux -CC` 总是新建会话**：要接已有会话必须 `tmux -CC attach -t <name>`。
- **缩放（`resize-pane -Z`）状态下拖分隔条无效**：tmux 不接受被缩放布局的尺寸调整。
- **`pnpm lint` 有 1 条既存 warning**（`src/components/terminal/CommandSuggestions.tsx:82`
  的 `useExhaustiveDependencies`），与 tmux 无关。

### 本轮验证

- `cargo test --lib core::tmux`：**32 passed**（含真 tmux 的集成用例 `gateway_drives_a_real_tmux_control_client`）。
- `npx vitest run`：**926 passed / 145 files**；`npx tsc --noEmit` 干净；`pnpm i18n:check` 干净。
- GUI 冒烟在本沙箱跑不了：WebKitGTK 的 webview 起不来（Tauri 窗口停在 10x10 未映射，截图全黑），
  与本次改动无关；真机验收仍走 `docs/tmux-cc-test-plan.md` + Windows 包。
- Windows 包这一轮**没能重打**：cargo-xwin 每次都要重写自己的
  `~/.cache/cargo-xwin/cmake/clang-cl/override.cmake`，而本沙箱只把工作目录挂成可写、
  其余文件系统只读（`XDG_CACHE_HOME` / `HOME` 指到工作目录也没用，它会解析到 `/home/user/.cache`）。
  前端产物已构建成功，卡在 `cargo xwin build` 的 CMake toolchain 这一步；在正常机器上跑
  `scripts/build-windows-test-package.sh` 不受影响。
