# MacJet VHS Tape
# 
# Usage: 
# 1. Run `sudo -v` in your terminal to cache your sudo credentials.
# 2. Run `vhs scripts/demo.tape`
#
# This will automatically record a demo GIF and capture the 3 required screenshots
# (view_apps.png, view_energy.png, view_reclaim.png) into the assets/ folder.

Output assets/macjet_demo.gif
Set Shell zsh
Set FontSize 13
Set Width 1600
Set Height 900
Set Theme "Molokai"
Set Framerate 30

# Launch MacJet
Type "sudo cargo run --release"
Sleep 500ms
Enter

# Give it time to boot and collect initial stats
Sleep 3s

# Apps View (View 1)
Screenshot assets/view_apps.png
Sleep 1s

# Navigate down and expand a group
Down
Down
Down
Sleep 500ms
Enter
Sleep 1.5s

# Tree View (View 2)
Type "2"
Sleep 1.5s

# Pressure View (View 3)
Type "3"
Sleep 1.5s

# Energy View (View 4 - Requires Sudo)
Type "4"
Sleep 2.5s
Screenshot assets/view_energy.png

# Reclaim View (View 5)
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
