# MacJet VHS Tape
#
# Launch without sudo so VHS (separate PTY) never blocks on Password:. MacJet runs in
# basic mode; energy view has limited data vs `sudo macjet`. For full metrics on your
# machine only, see docs/vhs-demo-recording.md (optional sudo + tape edit).
#
# From repo root: ./scripts/record-demo.sh
#
# Outputs: assets/macjet_demo.gif, view_apps.png, view_energy.png, view_reclaim.png

Output assets/macjet_demo.gif
Set Shell zsh
Set FontSize 22
Set Width 1680
Set Height 900
Set Theme "Molokai"
Set Framerate 30
# VHS default padding is 60px — adds a gray frame around the terminal; flush to edges.
Set Padding 0

# Crossterm strips all color if NO_COLOR is set (common in dev shells). ttyd/VHS also
# needs a true-color-capable term for RGB (38;2 / 48;2) from MacJet's theme.
Env COLORTERM "truecolor"
Env TERM "xterm-256color"
Env NO_COLOR ""

# Launch via wrapper (matches README-style onboarding; still from repo root)
Type "./macjet.sh"
Enter
Sleep 2s

# Give it time to boot and collect initial stats
Sleep 3s

# Processes view: filter to Google Chrome, then expand (Enter) to show drill-down
Type "/"
Type "Google Chrome"
Enter
Sleep 1s
Enter
Sleep 1s

# Scroll through Chrome children so tab / renderer names are visible in the GIF
Down
Sleep 1.5s
Down
Sleep 1.5s
Down
Sleep 1.5s
Down
Sleep 1.5s

# Hero still: Processes with Chrome expanded (inspector updates per row)
Screenshot assets/view_apps.png
Sleep 1s

# Kill list (2), Energy (3), Network (4), Predict (5)
Type "2"
Sleep 1.5s

Type "3"
Sleep 2.5s
Screenshot assets/view_energy.png

Type "4"
Sleep 1.5s

Type "5"
Sleep 2.5s
Screenshot assets/view_reclaim.png
Down
Down
Sleep 1.5s

# Back to Apps
Type "1"
Sleep 1.5s

# Quit
Type "q"
Sleep 1s
