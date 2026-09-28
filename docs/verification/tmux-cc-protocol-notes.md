# tmux control mode (`tmux -CC`): verified protocol facts

Facts below were measured against real tmux (3.2a) with a throwaway pty harness
(`python3 -c` fork/pty/select) and against the gateway integration test. They are
the non-obvious parts that cost debugging time.

## The window layout's leaf number is the pane's **id** number, not `pane_index`

```
$ tmux list-windows -a -F '#{session_name} #{window_id} layout=#{window_layout}'
1  @1 layout=b25e,80x24,0,0,1
by @0 layout=b25d,80x24,0,0,0
multi @2 layout=5881,80x24,0,0{40x24,0,0,2,39x24,41,0[39x12,41,0,3,39x11,41,13,4]}

$ tmux list-panes -a -F '#{window_id} idx=#{pane_index} id=#{pane_id}'
@1 idx=0 id=%1
@0 idx=0 id=%0
@2 idx=0 id=%2
@2 idx=1 id=%3
@2 idx=2 id=%4
```

Window `@1` has `pane_index=0` **and** pane id `%1`, and its layout ends in `1`;
the three panes of `@2` are layout numbers `2,3,4`. So the layout number is the
`N` of the pane's `%N` id. Keying `list-panes` output by `#{pane_index}` (an
earlier version did) silently fails for every window created after the first one,
which is exactly "pane has no id": the frontend renders it empty, cannot route
input to it, and cannot ask for its screen.

Regression coverage: `core::tmux::integration_tests` creates a *second* window on
purpose. A fresh tmux server has pane index == pane number for its first window,
so a test that only ever touches one window passes on the coincidence.

## Control mode streams only new output

`%output pane-id value` is documented as "a window pane produced output". A
control client that attaches to a session with existing content (or whose pane
views mount after the first prompt was drawn) receives nothing and shows an empty
pane. `refresh-client -C WxH` only yields `%layout-change`, and `send-keys -R`
yields an empty block — neither replays the screen.

Recovering it requires asking tmux:

```
display-message -p -t %0 'NYATERM-CAPTURE-BEGIN:#{pane_id}'
capture-pane -p -e -t %0
display-message -p -t %0 'NYATERM-CAPTURE-END:#{pane_id}'
```

Three separate lines, not one `;`-chained line: tmux 3.2 answers a chained line
with `%error parse error: syntax error` whenever `capture-pane` is followed by
another command. Each command gets its own `%begin`/`%end` block, so the screen is
bracketed by tags rather than correlated by block order. Captured lines are
octal-escaped exactly like `%output` data and need the same unescaping.

The replay is delivered as ordinary pane output, prefixed with `ESC[2J ESC[H`, so
a pane view asks for it once on mount and is idempotent if it remounts.

## `\t` in a `-F` format survives as a real tab

The gateway builds formats with Rust `\t`, which is a real tab, and tmux emits a
real tab. Passing a literal backslash-`t` (as a shell-written probe does when the
quoting never unescapes) makes tmux emit the two characters verbatim — a probe
artifact that can look like a gateway bug.
