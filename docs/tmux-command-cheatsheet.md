# tmux 命令速查（给 nyaterm 的 tmux 命令行用）

`Ctrl-b` 然后 `:`（或窗口条右侧的终端按钮）打开命令行；输入下面任意一条，回车执行，
输出和 `%error` 会显示在输入框下面。

这些命令都在**真实 tmux 3.2a** 上逐条跑过（`OK`），语法可直接用。命令里的 `-t` 参数
可以省略，省略时作用在 tmux 的当前窗口/pane 上。

## 为什么需要它（在我们这套里）

| 想做的事 | 为什么不能用界面/快捷键 | 用这条命令 |
| --- | --- | --- |
| 把某个 pane 拉大/缩小 | 分隔条是 tmux 布局的"镜像"，拖动不改 tmux 的尺寸 | `resize-pane -R 20`（左/上/下是 `-L` / `-U` / `-D`） |
| 临时全屏看一个 pane | 没做 zoom 按钮 | `resize-pane -Z`（再执行一次还原） |
| 把乱掉的布局一键排齐 | 界面只镜像 tmux 的布局 | `select-layout tiled`（或 `even-horizontal` / `even-vertical` / `main-horizontal`） |
| 窗口切换条上一眼认出窗口 | tmux 默认窗口名是 shell 名 | `rename-window 部署` |
| 把一个 pane 拎成独立窗口 | 没做 break/join 按钮 | `break-pane` |
| 把两个 pane 合并回一个窗口 | 同上 | `join-pane -s %3 -t %1` |
| 多台/多目录同时执行同一条命令 | nyaterm 的"同步输入"只覆盖它自己的会话 | `set-window-option synchronize-panes on`（关掉用 `off`） |
| 抓当前 pane 最近 200 行输出 | 想复制一段刚跑完的日志 | `capture-pane -p -S -200`（结果显示在命令行下面，可直接选中复制） |
| 看清 tmux 的状态/几何 | 界面只显示它关心的部分 | `display-message -p '#{version} #{pane_width}x#{pane_height}'` |
| 列清单 | 切换条只显示窗口，不显示 pane id | `list-windows -F '#{window_index}: #{window_name}'`、`list-panes -a -F '#{window_id}.#{pane_index} #{pane_id} #{pane_current_command}'` |

## 已验证的常用命令

窗口 / 会话：

```
rename-window 名字
rename-session 名字
new-window -n 名字
new-window -c /some/dir            # 在指定目录开窗
move-window -t 5                   # 挪到 5 号位
move-window -r                     # 重新编号，消掉空号
kill-window
kill-session
list-sessions
detach-client                      # 等于 Ctrl-b d
```

分屏 / 尺寸 / 布局：

```
split-window -h                    # 左右分（-v 是上下）
split-window -h -l 40%             # 按百分比分
split-window -c '#{pane_current_path}'   # 新 pane 沿用当前目录
resize-pane -R 10                  # 也支持 -L / -U / -D
resize-pane -x 100                 # 直接给列数
resize-pane -Z                     # zoom 切换
select-layout tiled                # 也支持 even-horizontal / even-vertical / main-horizontal
```

pane 之间：

```
select-pane -t %2                  # 按 id 选中
select-pane -L                     # 也支持 -R / -U / -D
swap-pane -U                       # 和上一个 pane 交换（-D 是下一个）
break-pane                         # 当前 pane 变成新窗口
join-pane -s %3 -t %1              # 把 %3 合进 %1 所在窗口
kill-pane
respawn-pane -k                    # 重启 pane 里的进程（清理卡死的 shell）
```

查看 / 抓取：

```
capture-pane -p -S -200            # 最近 200 行，带颜色加 -e
display-message -p '#{pane_id} #{pane_current_path} #{pane_current_command}'
list-windows -F '#{window_index}: #{window_name}'
list-panes -a -F '#{window_id}.#{pane_index} #{pane_id}'
display-message -p '#{version}'
list-keys                          # 看 tmux 键位绑定（输出较长）
```

同步输入（多机批量运维很有用）：

```
set-window-option synchronize-panes on
set-window-option synchronize-panes off
```

## 不适合放在这里的

交互式命令需要自己的一套界面，我们的命令行是"发一行、看一段回显"，所以别用：
`command-prompt`、`choose-tree`（窗口/会话选择器）、`display-panes`（pane 编号浮层）、
`confirm-before`。

滚动历史也不建议走 `copy-mode`：pane 是本地 xterm，滚轮/滚动条本来就能翻，而且 pane 自己
有回滚缓冲。
