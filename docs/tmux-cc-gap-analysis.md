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

### A. 可点的操作面窄（纯前端，几小时级）
现在 UI 只能切窗口 / 新建窗口 / 关闭窗口 / 分离。后端 `tmux_gateway_command` 已能执行任意 tmux
命令，所以缺的全是前端门面：

- 分屏按钮 / pane 右键菜单（`split-window`、`kill-pane`）
- 窗口重命名（`rename-window`）、移动窗口（`move-window`）
- 窗口列表浮层（`choose-tree` 或自绘列表）
- 一个 `:` 命令输入框，用来跑任意 tmux 命令

### B. 拖分隔条改 pane 大小（需要布局数学，半天级）
分隔条目前是"镜子"：只能靠调整客户端尺寸整体缩放，拖某一条分隔线不会只改相邻两个 pane。
要做需要像素↔单元格换算 + `resize-pane -L/-R/-U/-D <n>`，并对 `%layout-change` 做防抖回填。

### C. 窗口状态可视化（打磨级）
- 窗口切换条上没有 bell / activity / zoomed 标记（要查 `#{window_flags}`）
- `%message` 现在只进 debug 日志，没有像 tmux 状态行那样显示出来
- 完全不画 tmux 状态行（WindTerm 可显示）

### D. pane 内的搜索与复制模式 UI（打磨级）
pane 是本地 xterm，本地回滚缓冲可用（滚轮能翻），但没有搜索框；`Ctrl-b [` 只能进 tmux 自己的
copy-mode，没有键位提示或 UI 包装。

### E. 与 nyaterm 自身能力的联动
- tmux pane 不参与"同步输入 / 广播"分组（普通终端的 `syncGroups` 到不了这里）
- 录制 / 转写、崩溃或重连后自动回到 tmux 视图：未接
- 会话恢复（`ui.open_tabs`）只恢复 SSH 会话，不恢复 tmux 视图

### F. 入口体验
没有"检测到你敲了 `tmux -CC` 就提示/自动进入"或菜单项（此前约定不着急）。

## 建议

1. 先做 A：一次投入就能明显补平"能点的操作"，且不碰协议与布局。
2. B 单独排期（唯一需要动几何换算的部分）。
3. C/D/E/F 按需打磨。
