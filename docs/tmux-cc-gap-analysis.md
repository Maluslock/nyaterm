# tmux -CC：现状与 WindTerm 的差距

分支 `spike/tmux-cc-gateway-detection`，2026-09-28。

证据说明：本仓这一列来自代码与真机验证（Linux + 真 SSH + tmux 3.2a）。
WindTerm 的 tmux 集成**不在其开源仓内**（`WindTerm/src` 只有 libssh / Onigmo / Protocols / Pty /
Utility / Widgets，README 仅一句 “Supports tmux integration”），所以那一列基于其文档与使用者
体验，不是代码级比对。

## 已经对齐（核心闭环）

| 能力 | 我们的实现 |
| --- | --- |
| 输入 `tmux -CC` 自动接管 | 检测 DCS `ESC P 1000 p`（7 字节，无 ST）；协议流在 coalescer 里就被吃掉，不落到 xterm（否则 xterm 会永久停在 DCS 状态） |
| tmux 窗口 → 窗口切换条 | 位于 pane 区域**下方**（WindTerm 同款位置），点击 / `Alt+[`、`Alt+]` 切换，`+` 新建，`×` 关闭 |
| tmux pane → 原生分屏 | 按 `#{window_layout}` 镜像几何，每个 pane 一个本地 xterm；点击选中，`Alt+方向键` 按方向 `select-pane` |
| 键盘输入 | `send-keys -H -t %N` 逐字节发送（控制字符 / ESC 序列 / UTF-8 原样到达） |
| 客户端尺寸 | 容器实测后 `refresh-client -C WxH`，布局由 tmux 决定（含 gutter 宽度扣除） |
| tmux 前缀键 | 控制模式下前缀到不了 tmux 客户端（按键是发给 pane 的），因此视图模拟前缀：`Ctrl-b d/c/n/p`、`%`、`"`、`[`；未模拟的组合原样转发给 pane |
| 分离 / 退出 | `%exit` 后网关停用并清空状态（同一会话可再次 `tmux -CC`），剩余字节交还终端；普通终端始终挂在视图下（inert），提示符不丢；视图消失时把焦点交回终端 |
| pane 画面回放 | `capture-pane -p -e` 回放当前屏幕（BEGIN/END 标签框定），解决"attach 到已有内容 / 提示符先于视图画出"导致的空 pane |
| 附带能力 | pane 里同样有语法高亮、行号栏、命令补全（nyaterm 自有能力） |

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

| 坑 | 现象 | 规矩 |
| --- | --- | --- |
| 坐标空间不同 | 点在分隔条上却毫无反应 | 前端坐标 + 窗口位置 (160,100) = xdotool 屏幕坐标；先 `xdotool getwindowgeometry` 确认，别照抄上一次的数值 |
| 抢 tmux 默认 socket | app 的 tmux 视图突然 `%exit`，会话丢失 | 探测一律 `tmux -L <私有名>`；只有确认 app 没在用时才碰默认 socket |
| `pkill -f` 自匹配 | 把自己的 shell / 后台任务一起杀掉（SIGTERM） | 模式加方括号（`vite.j[s]`），或先 `pgrep` 拿 PID、确认命令行、在**另一条**命令里 kill |
| 后台进程占住 stdout | 起服务的脚本一直不返回，工具调用超时 | 长驻进程用 `setsid ... >日志 2>&1 </dev/null &`，就绪用轮询端口判断，不用 `sleep` |
| 中途重启长构建 | 只为改一行常量就重跑 15 分钟交叉编译 | 影响产物的常量（如 `BUILD_TAG`）在**开跑前**改好；构建期间只做只读验证 |
| 从记忆里改文件 | `edit` 反复被拒（未读/已变），或补丁断言失败 | 改动前 `read` 目标片段；`read` 与 `edit` 之间不要跑 `prettier --write`；机械改写用脚本时也必须先重读 |
