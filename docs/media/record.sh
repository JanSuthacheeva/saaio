#!/usr/bin/env bash
# Records saaio in a real terminal for the README: saaio-ui.png and saaio-ui.gif (macOS).
#
#   docs/media/record.sh open <state> <repo>   the UI on the scratch store, in an Alacritty window
#   docs/media/record.sh capture               the picture, then a GIF of moving through the UI
#
# Prepare <state> and <repo> with docs/media/demo-setup.sh first. After `open`, ask the master to
# start the flows demo-setup.sh printed and answer the agents' first-run questions in their panes.
# Run `capture` once the workers stand in different places. The agents run with your own logins;
# demo-setup.sh keeps your Claude Code plugins and MCP servers out, but check every frame for
# anything personal before you commit.
#
# The UI runs in a private tmux server (status line off), which the Alacritty window attaches to
# and this script sends keys through. Needs Alacritty, tmux, ffmpeg, swift, Menlo, and Screen
# Recording permission for the terminal you run it from.
set -euo pipefail

media=$(cd "$(dirname "$0")" && pwd)
socket=saaio-demo
tmux=(tmux -L "$socket")

# The demo window's id and bounds: "<id> <x> <y> <width> <height>".
window() {
    swift - <<'EOF'
import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowName as String] as? String) == "saaio-demo" {
    let b = w[kCGWindowBounds as String] as! [String: Any]
    print(w[kCGWindowNumber as String]!, b["X"]!, b["Y"]!, b["Width"]!, b["Height"]!)
}
EOF
}

open_ui() {
    local state=$1 repo=$2
    "${tmux[@]}" -f /dev/null new-session -d -s demo -x 150 -y 42 \
        "env -u SAAIO_FLOW_ID -u SAAIO_FLOW_NAME -u SAAIO_SLOT -u SAAIO_REPO -u SAAIO_WORKTREE -u SAAIO_MASTER \
         -u CLAUDECODE -u CODEX_THREAD_ID \
         bash -c 'source \"$state/env\"; cd \"$repo\"; clear; saaio; exec bash'"
    "${tmux[@]}" set -g status off
    "${tmux[@]}" set -g escape-time 0
    "${tmux[@]}" set -as terminal-features ",alacritty*:RGB,xterm-256color:RGB"
    /Applications/Alacritty.app/Contents/MacOS/alacritty --config-file "$media/alacritty.toml" \
        -e tmux -L "$socket" attach -t demo >/dev/null 2>&1 &
    sleep 2
    # A tiling window manager would resize the window while the UI draws; let it float.
    if command -v aerospace >/dev/null; then
        aerospace layout floating --window-id "$(window | cut -d' ' -f1)"
    fi
}

# The recording films the screen where the window is, so nothing may cover it.
raise() {
    if command -v aerospace >/dev/null; then
        aerospace focus --window-id "$1"
    else
        osascript -e 'tell application "Alacritty" to activate'
    fi
    sleep 0.5
}

capture() {
    local id x y w h
    raise "$(window | cut -d' ' -f1)"
    read -r id x y w h < <(window)
    key() { "${tmux[@]}" send-keys -t demo "$@"; }
    prefixed() { key C-Space; key "$@"; }

    screencapture -x -o -l "$id" "$media/saaio-ui.png"
    # ffmpeg records the mouse pointer anyway; park it in the corner, outside the window.
    swift - <<<'import CoreGraphics; CGWarpMouseCursorPosition(CGPoint(x: 0, y: 0))'

    local video
    video=$(mktemp -d)/saaio-ui.mp4
    ffmpeg -loglevel error -y -f avfoundation -capture_cursor 0 -pixel_format nv12 -framerate 30 -i "Capture screen 0:none" \
        -vf "crop=$w:$h:$x:$y" -t 21 -pix_fmt yuv420p "$video" &
    local recording=$!
    sleep 3
    prefixed j; sleep 2.5          # the peek over the first worker
    prefixed j; sleep 2.5          # and the next
    prefixed Enter; sleep 3        # into that worker
    prefixed o; sleep 2.5          # the board
    key l; sleep 2
    key l; sleep 2.5
    key Escape; sleep 1
    prefixed 0                     # back to the master
    wait "$recording"
    if [ "$(window)" != "$id $x $y $w $h" ]; then
        echo "the window moved while recording; no GIF written, run capture again" >&2
        exit 1
    fi
    ffmpeg -loglevel error -y -i "$video" \
        -vf "fps=15,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
        "$media/saaio-ui.gif"
}

case ${1:-} in
    open) open_ui "${2:?state}" "${3:?repo}" ;;
    capture) capture ;;
    *) echo "usage: record.sh open <state> <repo> | record.sh capture" >&2; exit 2 ;;
esac
