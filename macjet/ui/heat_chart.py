"""
MacJet — Heat Chart Widget
Real-time CPU/GPU/Energy timeline using textual-plot.
"""
from __future__ import annotations

from collections import deque
from textual.widget import Widget
from textual.app import ComposeResult

try:
    from textual_plot import PlotWidget, PlotExtPlot
    HAS_PLOT = True
except ImportError:
    try:
        from textual_plotext import PlotextPlot
        HAS_PLOT = True
    except ImportError:
        HAS_PLOT = False


class HeatChart(Widget):
    """60-second rolling history chart for CPU, GPU, and Energy."""

    DEFAULT_CSS = """
    HeatChart {
        height: 10;
        background: #0d1117;
        border-bottom: solid #30363d;
        padding: 0 1;
    }
    HeatChart .no-chart {
        color: #8b949e;
        text-style: italic;
        content-align: center middle;
        height: 100%;
    }
    """

    HISTORY_SIZE = 60  # 60 data points ≈ 60 seconds at 1s interval

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self._cpu_history: deque[float] = deque(maxlen=self.HISTORY_SIZE)
        self._gpu_history: deque[float] = deque(maxlen=self.HISTORY_SIZE)
        self._energy_history: deque[float] = deque(maxlen=self.HISTORY_SIZE)
        self._plot_widget = None

        # Initialize with zeros
        for _ in range(self.HISTORY_SIZE):
            self._cpu_history.append(0)
            self._gpu_history.append(0)
            self._energy_history.append(0)

    def compose(self) -> ComposeResult:
        if HAS_PLOT:
            try:
                from textual_plotext import PlotextPlot
                self._plot_widget = PlotextPlot()
                yield self._plot_widget
            except Exception:
                from textual.widgets import Static
                yield Static("  📊 Charts require textual-plot or textual-plotext", classes="no-chart")
        else:
            from textual.widgets import Static
            yield Static("  📊 Install textual-plotext for charts: pip install textual-plotext", classes="no-chart")

    def push_data(self, cpu: float, gpu: float = 0.0, energy: float = 0.0):
        """Push new data point to the history."""
        self._cpu_history.append(cpu)
        self._gpu_history.append(gpu)
        self._energy_history.append(energy)
        self._redraw()

    def _redraw(self):
        """Redraw the chart with current history data."""
        if not self._plot_widget:
            return

        try:
            plt = self._plot_widget.plt
            plt.clear_data()
            plt.clear_figure()

            # Theme
            plt.theme("dark")
            plt.canvas_color((13, 17, 23))  # #0d1117
            plt.axes_color((13, 17, 23))
            plt.ticks_color((139, 148, 158))  # #8b949e

            x = list(range(len(self._cpu_history)))
            cpu_data = list(self._cpu_history)
            gpu_data = list(self._gpu_history)

            # CPU line — electric blue
            plt.plot(x, cpu_data, label="CPU", color=(88, 166, 255))

            # GPU line — purple (only if we have data)
            if any(v > 0 for v in gpu_data):
                plt.plot(x, gpu_data, label="GPU", color=(188, 140, 255))

            plt.ylim(0, 100)
            plt.ylabel("Usage %")
            plt.xlabel("")
            plt.title("60s Heat Timeline")

            self._plot_widget.refresh()

        except Exception:
            pass
