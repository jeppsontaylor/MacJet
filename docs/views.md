# MacJet Views Guide

MacJet organizes data into 5 purpose-built "Flight Deck" views. You can switch between them using keys `1` through `5` or by hitting `Tab`.

## Navigation and controls (summary)

| Key | Action |
|-----|--------|
| `1`–`5` | Switch view (Apps / Tree / Pressure / Energy / Reclaim) |
| `Tab` | Cycle views |
| `↑` `↓` | Move selection |
| `Enter` | Expand / collapse group or role bucket |
| `s` | Cycle sort (CPU / Memory / Name / PID) |
| `/` | Filter by name; `Esc` clears |
| `h` | Toggle system processes |
| `k` / `K` | SIGTERM / SIGKILL selected |
| `z` | Suspend / resume |
| `w` | Inspector context |
| `?` | Help |
| `q` | Quit |

For the full table, see the [README](../README.md#-keybindings).

## `1` — Apps View (Default)
**Focus:** Application-level visibility. Collapses threads into parent App row.

## `2` — Tree View
**Focus:** Raw process hierarchy. Replicates exact `htop` style structure.

## `3` — Pressure View
**Focus:** Finding memory leaks. Sorts the entire system strictly by Resident Set Size (RSS) footprint.

## `4` — Energy View
**Focus:** Battery life and thermal throttling. Includes Apple `powermetrics` integration (Temp, Impact %, Wakeups).

## `5` — Reclaim View (Intelligent Kill List)
**Focus:** One-click problem solving.
Instead of hunting for the problem, MacJet lists it. The Reclaim Scorer evaluates every process against a 100-point rubric and presents actionable targets with risk bands (🟢 Safe, 🟡 Review, 🔴 Danger).
