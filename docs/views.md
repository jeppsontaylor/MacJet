# MacJet Views Guide

MacJet organizes data into 5 purpose-built "Flight Deck" views, eliminating the clutter of traditional task managers. You can switch between them using keys `1` through `5` or by hitting `Tab`.

## `1` — Apps View (Default)
**Focus:** Application-level visibility.

Instead of showing 30 different "Chrome Helper" threads, MacJet collapses them into a single coherent **Google Chrome** row. Expanding the row (using `Enter`) reveals semantic groups:
- Renderer Tabs
- GPU Process
- Utility Services

### What the columns mean:
- **Severity Rail**: Left-most color strip (Green/Yellow/Red) indicating immediate concern level.
- **Name**: Application name and child count.
- **CPU**: 60-second smoothed CPU average.
- **Mem**: Total aggregated physical memory footprint (RSS).
- **Trend**: 60-second micro-sparkline of CPU activity.

## `2` — Tree View
**Focus:** Raw process hierarchy.

Replicates the precision of `htop`, laying out exact parent-child inheritance. Ideal for diagnosing shell pipelines, Docker containers, or runaway bash scripts.

## `3` — Pressure View
**Focus:** Finding memory leaks.

Sorts the entire system strictly by Resident Set Size (RSS) footprint. The sparklines in this view automatically convert from tracking "CPU load" to tracking "Memory growth." If the line is trending slowly upwards, you have identified a memory leak.

## `4` — Energy View
**Focus:** Battery life and thermal throttling.

*(Note: Requires MacJet to be run as root via `sudo ./macjet.sh`)*.
Powered directly by macOS `powermetrics`.

### Unique columns:
- **Temp**: Estimated core temperature impact.
- **Impact %**: MacJet's calculated percentage of power drain.
- **Wakeups**: Number of times the process has interrupted CPU sleep states. High wakeups drain battery fast.

## `5` — Reclaim View (Intelligent Kill List)
**Focus:** One-click problem solving.

Instead of hunting for the problem, JetsMonitor lists it. The Reclaim Scorer evaluates every process against a 100-point rubric and presents actionable targets.

### Scoring Rubric:
- Has it been maxing out a core for 30s? (+30 points)
- Is it consuming more than 2GB of RAM? (+25 points)
- Is the RAM rapidly growing? (+15 points)
- Is it a background (non-GUI) app? (+15 points)
- Is it spawning rapidly (Process Storm)? (+10 points)
- Is it waking up the CPU constantly? (+5 points)

Each entry is given a risk band (🟢 Safe, 🟡 Review, 🔴 Danger) and a recommended action (Kill, Suspend, Profile). Hit `w` to open the Inspector and click the suggested action!
