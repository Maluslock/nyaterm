# tmux -CC 验收手册（Windows 测试包 · 2026-09-30）

这份是给你手动过一遍用的。每条用例都写了：**怎么操作 → 应该看到什么 → 怎么客观判定
（在 tmux 侧跑什么命令 / 日志里看哪一行）**。判定用 tmux 命令的主要原因是：界面看起来
"没反应"时，先分清是 nyaterm 没画对，还是 tmux 根本没执行。

包内构建标识：`tmux-cc-test-2026-09-30`
本分支已合并上游 `1ef75d1bb`（v1.2.12）。

---

## 0. 准备

**0.1 解压与启动**

- 整个文件夹一起解压：`nyaterm.exe`、`nyaterm-mcp.exe`、`portable.flag`、`conpty\`、`README.txt`
- 双击 `nyaterm.exe`。日志在 `解压目录\data\logs\nyaterm-diagnostics.<日期>.jsonl`
- 确认第一行 `NyaTerm starting` 里 `"build": "tmux-cc-test-2026-09-30"`

**0.2 一个能折腾的 tmux 会话（在服务器上）**

```bash
tmux -V                     # 建议 3.0+；3.3+ 才有 %message
tmux new-session -d -s demo # 造一个带内容的会话，用来测 attach 回放
tmux send-keys -t demo 'echo PREEXISTING-MARKER' Enter
tmux split-window -t demo -v
tmux send-keys -t demo 'echo LOWER-PANE-MARKER' Enter
```

**0.3 弄一个"对照窗口"**

另开一个普通 SSH 标签（或本地终端 `ssh` 到同一台机器）。所有"客观判定"都在这里跑 tmux
命令。注意：**别在对照窗口里对 app 正在用的那个 tmux 服务器跑 `tmux kill-server`**——
那会把 app 的 control client 一起杀掉（想清干净就先退出 app）。

**0.4 两个万能判定命令**

```bash
# 窗口状态：编号、名字、缩放、响铃、活动
tmux list-windows -a -F '#{window_index}:#{window_name} zoom=#{window_zoomed_flag} bell=#{window_bell_flag} act=#{window_activity_flag}'
# pane 状态：id、尺寸、是否活动
tmux list-panes -a -F '#{window_index}.#{pane_index} #{pane_id} #{pane_width}x#{pane_height} active=#{pane_active}'
```

**0.5 日志里会出现的行（后面反复引用）**

| 日志 message | 含义 |
| --- | --- |
| `terminal output coalescer created`（`gateway_attached`） | `true` = 网关已挂上；`false` = 程序内部没取到状态（算 bug） |
| `tmux control mode detected; gateway engaged` | 识别到控制模式并接管，界面应该已经切换 |
| `tmux gateway state published`（带 `windows`/`panes`） | 状态推给界面，切换窗口/pane 时会连续出现 |
| `tmux pane output forwarded`（`replay=true`） | pane 画面回放，attach 后每个 pane 应在第一次出现 |
| `tmux control-mode client exited` | 分离或 tmux 退出 |
| `tmux control-mode marker missing` | 收到了控制模式数据但缺 `ESC P1000p` 标记（probe=dcs/protocol）→ 大概率远端 tmux 太老 |

---

## 1. 进入控制模式

### 1.1 新会话：`tmux -CC`
- **操作**：在 SSH 终端里输入 `tmux -CC` 回车
- **期望**：界面自动切成"上方 pane 区 + 下方窗口条"，窗口条上出现 `0: bash` 之类；原终端被压在下面但没丢
- **判定**：日志出现 `gateway engaged`；窗口条窗口名与 `tmux list-windows` 一致
- **注意**：`tmux -CC` 永远是**新建**一个会话（tmux 的行为，不是我们的）

### 1.2 接管已有会话：`tmux -CC attach -t demo`
- **操作**：输入 `tmux -CC attach -t demo`
- **期望**：直接进入控制模式，**并且两个 pane 里已经有内容**（`PREEXISTING-MARKER` /
  `LOWER-PANE-MARKER`），不是空白
- **判定**：日志里每个 pane 各有一条 `tmux pane output forwarded` 且 `replay=true`
- **这是最容易出问题的一条**（控制模式只推新输出，历史画面必须靠 `capture-pane` 回放），
  如果空白，把日志里这两个 pane 的 `replay` 字段发我

### 1.3 不该误识别的情况
- **操作**：普通执行 `printf '%%output %%0 hi\n'`（或 `echo '%begin 1 2 3'`）
- **期望**：正常当终端输出打印，**不进入** tmux 视图
- **判定**：日志没有 `gateway engaged`

---

## 2. pane 镜像与画面

### 2.1 分屏几何镜像
- **操作**：在 app 的窗口条上点"左右分屏"或"上下分屏"，或在命令行里 `split-window -h/-v`
- **期望**：界面立刻多出一个 pane，方向、比例与 tmux 一致；新 pane 是 tmux 的活动 pane（有高亮边框）
- **判定**：`tmux list-panes -a -F '#{pane_id} #{pane_width}x#{pane_height}'` 与界面里每个
  pane 的格子数一致（pane 内 xterm 的列/行就是 tmux 给的 `pane_width/pane_height`）

### 2.2 多窗口
- **操作**：`Ctrl+b c` 新建窗口，或点窗口条上的 `+`
- **期望**：窗口条多一个标签并自动切过去；每个窗口各自的分屏结构正确
- **判定**：`tmux list-windows -a -F '#{window_index}:#{window_name}'`；日志 `state published` 里 `windows=N`

### 2.3 全屏程序
- **操作**：在 pane 里跑 `top`、`vim`、`htop`
- **期望**：全屏程序正常铺满该 pane、按键（方向键/ESC/Ctrl 组合）都进得去；退出后回到 shell
- **判定**：在该 pane 里按键能操作程序即可；出问题看 pane 是否只有部分刷新

### 2.4 pane 增删同步
- **操作**：在对照窗口里 `tmux split-window -t demo`、`tmux kill-pane -t demo`
- **期望**：界面 1 秒内跟着增删，剩下的 pane 重新排布

---

## 3. 输入

### 3.1 常规键入
- **操作**：点进某个 pane，输入 `echo hello-测试`、按方向键、Ctrl+C、Ctrl+D
- **期望**：本地回显 + 远端执行；中文正常；Ctrl+C 能打断 `sleep 100`
- **判定**：`tmux capture-pane -p -t %N` 里能看到这行

### 3.2 逐字节发送（关键）
- **操作**：在 `vim` 里按 `Ctrl+[`、`Ctrl+W`、插入模式下打中文；在 `top` 里按方向键
- **期望**：全部正常（我们是用 `send-keys -H` 逐字节发的，控制字符/ESC 序列/UTF-8 原样到达）
- **注意**：这一条如果在某个程序里失灵，请记下程序名 + 具体按键

### 3.3 粘贴
- **操作**：在 pane 里粘贴一段多行文本（比如 `for i in 1 2 3; do echo $i; done`）
- **期望**：内容进到 pane 的 shell，不是被 tmux 当成自己的命令

### 3.4 应用级输入改投 pane（重要回归点）
- **操作**：用底部"快捷命令"面板发一条 `ls -la`（或开启同步输入后打字）
- **期望**：这条内容进入 **tmux 当前活动 pane** 的 shell 并执行
- **反例（旧版 bug）**：以前会被 tmux 当自己的命令，报 `parse error: usage: list-sessions`
- **判定**：pane 里出现 `ls -la` 的输出；日志无 `parse error`

### 3.5 pane 内的补全 / 历史
- **操作**：在 pane 里敲几个字符（设置里若开了"命令建议"）
- **期望**：建议弹窗、Tab/回车选中；上方向键能翻 shell 历史（shell 自己的）
- **说明**：这些是 nyaterm 自带能力，tmux pane 里同样有效

### 3.6 行号 / 时间戳
- **操作**：设置里打开"显示行号 / 时间戳"
- **期望**：pane 左侧出现 gutter，新输出的行带上编号/时间

---

## 4. 窗口条（pane 区下方）

逐项点一遍，每项都给出"对照命令"：

| 控件 | 操作 | 期望 | 对照 |
| --- | --- | --- | --- |
| ◀ ▶ | 点上/下一个 | 切换活动窗口 | `tmux list-windows -F '#{window_index} active=#{window_active}'` |
| 窗口标签 | 单击 | 切到该窗口 | 同上 |
| 窗口标签 | **双击** | 变成可编辑输入框，回车改名 | `tmux list-windows -F '#{window_name}'` |
| 窗口标签 × | 点 | 关闭该窗口 | 窗口数减少 |
| ＋ | 点 | 新建窗口 | `tmux list-windows` 多一个 |
| ⛶ 缩放 | 点 | 当前 pane 放大/还原（按钮按下态跟着变） | `tmux display-message -p '#{window_zoomed_flag}'` 1↔0 |
| 左右分屏 | 点 | `split-window -h` | `tmux list-panes` 多一个，且左右排列 |
| 上下分屏 | 点 | `split-window -v` | 上下排列 |
| 布局下拉 | 选"平铺/主窗在上/均匀横排/均匀竖排" | 布局改变 | `tmux display-message -p '#{window_layout}'` 变化 |
| ⏻ 分离 | 点 | 回到 SSH 主终端 | 日志 `control-mode client exited` |
| ☰ 列表 | 点 | 打开窗口/窗格列表（见第 9 节） | — |
| ▣ 命令行 | 点 | 打开 tmux 命令行（见第 8 节） | — |

### 4.4 响铃 / 活动标记
- **操作**：切到窗口 0，在**另一个**窗口里跑 `sleep 2; printf '\a'`
- **期望**：那个窗口的名字左边出现 🔔（响铃）；有普通新输出时出现 ●（活动）
- **判定**：`tmux list-windows -F '#{window_index} bell=#{window_bell_flag} act=#{window_activity_flag}'`

---

## 5. 分隔条拖拽（这版新增，重点测）

### 5.1 鼠标拖
- **操作**：把鼠标移到两个 pane 之间的细线上，按住左键上下（或左右）拖
- **期望**：分隔线跟着鼠标走，且 **只有相邻两个 pane 变大小**，其它 pane 不动
- **判定**：拖之前/之后各跑一次
  `tmux list-panes -F '#{pane_id}:#{pane_width}x#{pane_height}'`，相邻两个的数字应反方向变化
- **注意**：界面是 tmux 的镜像，松手后以 tmux 的布局为准（可能回弹 1 格，正常）

### 5.2 键盘调
- **操作**：用 `Tab` 把焦点移到分隔条（会高亮），按 ↑/↓（横条）或 ←/→（竖条）
- **期望**：每按一次，相邻 pane 差 1 格
- **判定**：同 5.1

### 5.3 边界
- **操作**：一直拖到最小尺寸
- **期望**：tmux 拒绝时不再变小，界面不崩、不闪烁
- **说明**：pane 有 zoom 时拖拽可能无效（tmux 的语义），先 `Ctrl+b z` 还原再试

---

## 6. pane 右键菜单

### 6.1 右键会先选中该 pane（关键回归点）
- **操作**：右键点击**非活动**的那个 pane
- **期望**：该 pane 立刻变成活动 pane（出现高亮边框），菜单出现在鼠标位置
- **判定**：菜单的"查找""关闭 pane"等操作只影响这个 pane；
  `tmux list-panes -F '#{pane_id} active=#{pane_active}'` 里活动 pane 变成它
- **背景**：WebKitGTK/WebView2 的右键不触发 mousedown，之前会导致"菜单作用在别的 pane 上"

### 6.2 菜单项逐条
| 菜单项 | 期望 | 对照 |
| --- | --- | --- |
| 查找… | 该 pane 出现搜索条（见第 7 节） | — |
| 左右分屏 / 上下分屏 | 在该 pane 位置分屏 | `tmux list-panes` |
| 放大 / 还原 pane | 同 ⛶ | `#{window_zoomed_flag}` |
| 与上一个 / 下一个 pane 交换 | 两个 pane 内容互换 | `tmux list-panes` 顺序 |
| 拆成独立窗口 | 该 pane 变成新窗口 | `tmux list-windows` 多一个 |
| 关闭 pane | 该 pane 消失 | `tmux list-panes` 少一个 |

---

## 7. pane 内查找（这版新增，重点测）

- **操作**：点进一个 pane（先 `echo nyaterm-find-probe`），按 **Ctrl+Shift+F**
  （即设置里的"终端查找"键，可改键）
- **期望**：该 pane **右上角**出现搜索条：两个模式页签（当前缓冲 / 深度历史）、
  输入框、`Aa`、`Word`、`.*` 开关、命中计数
- **输入 `find-probe`**：命中的文本高亮，计数显示 `1/2` 这种
- **Enter**：下一个命中；**Shift+Enter**：上一个；**Esc**：关闭并回到终端
- **大小写 / 整词 / 正则**：举例
  - `Aa` 关：`find-probe` 和 `FIND-PROBE` 都命中；开：只命中大小写一致的
  - `Word` 开：`probe` 不再命中 `find-probe` 里的一段
  - `.*` 开：输入 `find-.+probe` 能命中
- **深度历史**页签：搜的是这台会话的命令历史（pane 里执行过的命令也会进历史）
- **注意**：搜索的是**本地 xterm 的回滚缓冲**，所以只有 app 里显示过的内容才搜得到；
  想搜远端完整历史用 tmux 命令行跑 `capture-pane -p -S -2000`

---

## 8. tmux 命令行

### 8.1 打开
- **操作**：`Ctrl+b` 然后 `:`（冒号；中文输入法下的全角 `：` 也行），或点窗口条的 ▣
- **期望**：底部出现输入框，上面一排预设按钮
- **注意**：命令行打开时键盘归输入框，不会被 pane 抢走

### 8.2 预设按钮（点一下就知道效果）
`capture-pane -p -S -200` · `split-window -h` · `split-window -v` · `resize-pane -Z` ·
`select-layout tiled` · `synchronize-panes on` · `synchronize-panes off` · `rename-window ` ·
`break-pane` · `list-windows -F '#{window_index}: #{window_name}'`

### 8.3 历史
- **操作**：依次跑 `list-windows`、`display-message -p '#{client_width}'`，然后用 ↑/↓ 翻
- **期望**：能翻出刚才的命令，回车重跑

### 8.4 回答与报错回显
- **操作**：跑 `list-windows -F '#{window_index}: #{window_name}'` → 输入框下方显示回答
- **操作**：跑 `nyaterm-not-a-command` → 显示 tmux 的 `%error`（`parse error: ...`）
- **判定**：这两类信息都来自 tmux 本身，说明命令确实在 tmux 侧执行了

### 8.5 只有命令行能做的场景（值得试）
- `synchronize-panes on` → 在任一 pane 打字，同窗口所有 pane 同步（对照 `tmux show -gv synchronize-panes`）
- `select-layout main-horizontal` → 布局重组
- `swap-pane -U` / `-D` → 与右键菜单同效，但可指定 `-t %N`
- `rename-window 新名字` / `move-window -t 5`
- `resize-pane -t %1 -y 20` → 精确指定某 pane 行数

---

## 9. 窗口 / 窗格列表（这版新增）

- **操作**：`Ctrl+b` 然后 `w`，或点窗口条最右的 ☰
- **期望**：中间出现半透明列表，按窗口分组，每个窗口下面列出它的 pane（id + 尺寸），
  光标**停在当前 pane 那一行**（不是当前窗口行）
- **操作**：↑/↓（或 j/k）移动 → **Enter** 切换 → **Esc** 关闭 → 点遮罩空白也关闭
- **判定**：切到窗口行 → `tmux list-windows -F '#{window_index} active=#{window_active}'`；
  切到 pane 行 → `tmux list-panes -F '#{pane_id} active=#{pane_active}'`
- **提示**：列表里能看到 bell/zoomed 标记和 pane 尺寸，用来核对界面是否漏画

---

## 10. 前缀键与快捷键总表

控制模式下，你按的键是发给 **pane 里的程序**的，tmux 自己的前缀收不到，所以前缀由 nyaterm
模拟：

| 按键 | 动作 | 对照 |
| --- | --- | --- |
| `Ctrl+b` `d` | 分离（回 SSH 主终端） | 会话仍在，`tmux ls` 显示 detached |
| `Ctrl+b` `c` / `n` / `p` | 新建 / 下一个 / 上一个窗口 | `tmux list-windows` |
| `Ctrl+b` `%` | 左右分屏 | `split-window -h` |
| `Ctrl+b` `"` | 上下分屏 | `split-window -v` |
| `Ctrl+b` `z` | 放大 / 还原当前 pane | `#{window_zoomed_flag}` |
| `Ctrl+b` `x` | 关闭当前 pane | `tmux list-panes` |
| `Ctrl+b` `[` | 进入 tmux 自己的 copy-mode | copy-mode 提示出现在 pane 内 |
| `Ctrl+b` `w` | 打开窗口/窗格列表 | — |
| `Ctrl+b` `:` 或 `;` | 打开 tmux 命令行 | — |
| `Ctrl+Shift+F` | 在 pane 内查找 | — |
| `Alt+←/→/↑/↓` | 按方向切换 pane | `#{pane_active}` |
| `Alt+[` / `Alt+]` | 上一个 / 下一个窗口 | `#{window_active}` |
| 鼠标点 pane | 选中该 pane | `#{pane_active}` |
| 鼠标右键 pane | 选中并打开菜单 | 同 6.1 |

**没有模拟的前缀组合**：会原样转发给 pane 里的程序。如果你习惯用 `Ctrl+b` 后接别的键
（例如自定义绑定），请告诉我具体组合，我按需补。

---

## 11. 分离与退出

### 11.1 `Ctrl+b d`
- **期望**：回到 SSH 主终端，提示符还在（终端一直挂在视图下面，不丢输出）
- **判定**：日志 `control-mode client exited`；状态里 `windows=0`
- **重点**：退出后**焦点**应落在终端上，直接打字就能用（不用先点一下）

### 11.2 在 pane 里 `exit` / `tmux kill-server`
- **期望**：同 11.1，自动切回主终端
- **注意**：用 `kill-server` 测的话，会话就没了（正常）

### 11.3 再次进入
- **操作**：回到主终端后再输一次 `tmux -CC`（或 `attach -t demo`）
- **期望**：能重新接管，窗口/pane 状态是新的，不是上一次的残留

---

## 12. tmux 状态行消息（`%message`）

- **背景**：控制模式不画状态行，tmux 会把状态行消息以 `%message` 通知客户端，我们在窗口条上方
  显示 6 秒后淡出
- **版本要求**：**tmux 3.2a 及以下根本不发这个消息**（它的二进制里没有 `%message` 字样）。
  只有 tmux **3.3+** 才有；3.2a 上验证不了属于正常
- **在 3.3+ 上怎么造**：让 tmux 自己弹消息，例如 `Ctrl+b :` 跑
  `display-message "probe from tmux"`，或触发 `no next window` 这类状态行提示
- **期望**：窗口条上方短暂出现该文本

---

## 13. 已知限制（这些是设计取舍，不是 bug）

1. **不画 tmux 状态行**（底部那条 `[0] 0:bash*`）——窗口条承担了它的职责
2. **pane 不参与 nyaterm 的"同步输入分组"**——要同步请用 tmux 自己的
   `synchronize-panes on`（命令行预设里有）
3. **录制 / 崩溃恢复 / 会话恢复不包含 tmux 视图**：恢复正常只针对 SSH 会话
4. **`%message` 需要 tmux 3.3+**（见第 12 节）
5. **窗口不提升为 nyaterm 标签页**（此前约定：tmux 窗口留在窗口条里）
6. pane 内的搜索只搜本地回滚缓冲（默认 5000 行），不是远端全部 scrollback

---

## 14. 出问题怎么反馈（按这个格式最省事）

1. `tmux -V` 的输出
2. 复现步骤（点哪里 / 按什么键 / 什么顺序）
3. 期望 vs 实际（截图最好）
4. 日志：`解压目录\data\logs\nyaterm-diagnostics.<日期>.jsonl` 里对应时间段的几行；
   至少包含这几条（有就发）：`NyaTerm starting`、含 `gateway` 或 `tmux` 的 error/warn 行
5. 如果某个 tmux 命令没生效，附上日志里的 `%error` 文本

如果只想快速定位"是不是 tmux 时代理/网络问题"：日志里搜 `marker missing` 和
`gateway_attached`。

---

## 附录 A：一页打勾清单

```
[ ] 1.1 tmux -CC 自动接管
[ ] 1.2 attach 已有会话，画面回放（两个 pane 都有内容）
[ ] 2.1 分屏几何与 tmux 一致
[ ] 2.3 vim/top 全屏程序可用
[ ] 3.1 中文 / 方向键 / Ctrl+C
[ ] 3.4 快捷命令进 pane（不再是 parse error）
[ ] 3.5 pane 内补全与历史
[ ] 4 窗口条：切换 / 双击改名 / 新建 / 关闭 / 缩放 / 分屏 / 布局 / 分离
[ ] 4.4 bell 与 activity 标记
[ ] 5.1 拖分隔条只改相邻两个 pane
[ ] 5.2 Tab+方向键按格调整
[ ] 6.1 右键先选中该 pane
[ ] 6.2 右键菜单八项
[ ] 7 Ctrl+Shift+F 查找（计数/高亮/Aa/Word/正则/Esc）
[ ] 8 命令行：预设 / 历史 / 回答 / %error
[ ] 9 Ctrl+b w 列表，Enter 切换
[ ] 10 前缀键与 Alt 组合
[ ] 11.1 Ctrl+b d 分离后焦点在终端
[ ] 11.3 再次 tmux -CC 能重新接管
[ ] 12 %message（需 tmux 3.3+）
```

## 附录 B：tmux 侧对照命令速查

```bash
tmux list-windows -a -F '#{window_index}:#{window_name} zoom=#{window_zoomed_flag} bell=#{window_bell_flag} act=#{window_activity_flag}'
tmux list-panes  -a -F '#{window_index}.#{pane_index} #{pane_id} #{pane_width}x#{pane_height} active=#{pane_active}'
tmux display-message -p '#{window_layout}'         # 布局串
tmux display-message -p '#{window_zoomed_flag}'    # 缩放
tmux capture-pane -p -t %0                         # 某 pane 的当前画面
tmux capture-pane -p -S -2000 -t %0                # 含历史
tmux show -gv synchronize-panes                    # 同步输入开关
tmux list-clients -F '#{client_name} #{client_width}x#{client_height}'   # 客户端尺寸（应与 pane 容器一致）
```
