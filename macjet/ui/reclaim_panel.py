"""
MacJet — Reclaim Panel (Kill List)
Scored recommendation view showing which processes to kill and why.
"""

from __future__ import annotations

from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import DataTable, Static

from ..collectors.metrics_history import ReclaimCandidate


# ─── Color ramps for score ───────────────────────────
def _score_color(score: int) -> str:
    if score >= 80:
        return "#FF4D6D"
    elif score >= 60:
        return "#FF8A4C"
    elif score >= 40:
        return "#FDBA35"
    elif score >= 20:
        return "#A78BFA"
    return "#7F8DB3"


def _risk_badge(risk: str) -> str:
    colors = {
        "safe": "[#32D583]SAFE[/]",
        "review": "[#FDBA35]REVIEW[/]",
        "danger": "[#FF4D6D]DANGER[/]",
    }
    return colors.get(risk, risk)


def _format_mem(mb: float) -> str:
    if mb >= 1024:
        return f"{mb / 1024:.1f}GB"
    return f"{mb:.0f}MB"


class ReclaimPanel(Widget):
    """Kill List panel showing scored process recommendations."""

    DEFAULT_CSS = """
    ReclaimPanel {
        height: 1fr;
        background: #0A0F1E;
    }
    ReclaimPanel #reclaim-toolbar {
        height: 1;
        background: #10182B;
        border-bottom: solid #1A2540;
        padding: 0 1;
        color: #7F8DB3;
    }
    ReclaimPanel DataTable {
        height: 1fr;
    }
    """

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self._table: DataTable | None = None
        self._candidates: list[ReclaimCandidate] = []
        self._row_keys: list[str] = []

    def compose(self) -> ComposeResult:
        yield Static(
            "  RECLAIM  [#7F8DB3]Scored recommendations • q:Quit App • k:Kill • K:Force Kill[/]",
            id="reclaim-toolbar",
        )
        table = DataTable(id="reclaim-table", cursor_type="row")
        table.add_column("", key="rail", width=1)
        table.add_column("Score", key="score", width=6)
        table.add_column("App", key="app", width=24)
        table.add_column("Reclaim", key="reclaim", width=16)
        table.add_column("Risk", key="risk", width=8)
        table.add_column("Reason", key="reason", width=36)
        table.add_column("Action", key="action", width=14)
        self._table = table
        yield table

    def update_data(self, candidates: list[ReclaimCandidate]):
        """Update the reclaim table with scored candidates."""
        if not self._table:
            return

        self._candidates = candidates
        table = self._table

        # Save cursor
        saved_row = table.cursor_row

        table.clear()
        self._row_keys = []

        for candidate in candidates:
            if candidate.score < 5:
                continue  # Skip very low scores

            # Severity rail
            sc = _score_color(candidate.score)
            rail = f"[{sc}]█[/]"

            # Score
            score_str = f"[{sc}]{candidate.score:3d}[/]"

            # App name
            app_str = f"{candidate.icon} {candidate.app_name}"
            if len(app_str) > 23:
                app_str = app_str[:22] + "…"

            # Reclaim estimate
            cpu_str = f"{candidate.reclaim_cpu:.0f}%"
            mem_str = _format_mem(candidate.reclaim_mem_mb)
            reclaim_str = f"~{cpu_str} / {mem_str}"

            # Risk badge
            risk_str = _risk_badge(candidate.risk)

            # Reason (truncate)
            reason = candidate.reason
            if len(reason) > 35:
                reason = reason[:34] + "…"

            # Action
            action_str = candidate.suggested_action

            row_key = f"reclaim-{candidate.group_key}"
            table.add_row(
                rail,
                score_str,
                app_str,
                reclaim_str,
                risk_str,
                reason,
                action_str,
                key=row_key,
            )
            self._row_keys.append(row_key)

        # Restore cursor
        if saved_row < len(self._row_keys):
            table.move_cursor(row=saved_row)

    def get_selected_group_key(self) -> str | None:
        """Get the group key of the currently selected reclaim candidate."""
        if not self._table or not self._row_keys:
            return None
        cursor_row = self._table.cursor_row
        if 0 <= cursor_row < len(self._row_keys):
            key = self._row_keys[cursor_row]
            if key.startswith("reclaim-"):
                return key[8:]
        return None

    def get_selected_candidate(self) -> ReclaimCandidate | None:
        """Get the currently selected ReclaimCandidate."""
        if not self._table or not self._candidates:
            return None
        cursor_row = self._table.cursor_row
        # Candidates and rows should be 1:1 (excluding <5 scores)
        visible = [c for c in self._candidates if c.score >= 5]
        if 0 <= cursor_row < len(visible):
            return visible[cursor_row]
        return None
