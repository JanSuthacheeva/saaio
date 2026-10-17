# Records flow ui for the README: flow-ui.png and flow-ui.gif.
#
# Prepare it first with docs/media/demo-setup.sh and follow what it prints. Then, from the
# repository's root, with the flows running and flow ui detached:
#
#   FLOW_DEMO_STATE=<state> FLOW_DEMO_REPO=<repo> vhs docs/media/demo.tape
#
# The tape attaches a new flow ui, which takes over from any other.

Output docs/media/flow-ui.gif

Require flow

Set Shell "bash"
Set FontFamily "FiraCode Nerd Font Mono"
Set FontSize 15
Set Width 1600
Set Height 900
Set Padding 0
Set Framerate 20
Set TypingSpeed 0
Set Theme { "name": "flow", "background": "#141418", "foreground": "#c8c8d0", "cursor": "#c8c8d0", "selection": "#26262e", "black": "#141418", "red": "#f0717f", "green": "#93c46d", "yellow": "#e5b45a", "blue": "#7aa7ff", "magenta": "#b392f0", "cyan": "#56c2c2", "white": "#c8c8d0", "brightBlack": "#6c6c78", "brightRed": "#f0717f", "brightGreen": "#93c46d", "brightYellow": "#e5b45a", "brightBlue": "#7aa7ff", "brightMagenta": "#b392f0", "brightCyan": "#56c2c2", "brightWhite": "#f0f0f4" }

Hide
Type `source "$FLOW_DEMO_STATE/env" && cd "$FLOW_DEMO_REPO" && clear && flow ui`
Enter
Sleep 4s
Show

Sleep 2s
Screenshot docs/media/flow-ui.png

# The peek: what the first two workers did lately.
Ctrl+Space
Type "j"
Sleep 2.5s
Ctrl+Space
Type "j"
Sleep 2.5s

# Into that worker, then the board, then back to the master.
Ctrl+Space
Enter
Sleep 3s
Ctrl+Space
Type "o"
Sleep 2.5s
Type "l"
Sleep 2s
Type "l"
Sleep 2.5s
Escape
Sleep 1s
Ctrl+Space
Type "0"
Sleep 2.5s

Hide
Ctrl+Space
Type "d"
Sleep 1s
