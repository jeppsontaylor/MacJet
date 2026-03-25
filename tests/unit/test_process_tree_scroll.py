"""
Regression test for the process list scroll-snap-to-bottom bug.

Verifies that ProcessTree pins cursor to row 0 on initial data loads
(when user hasn't interacted), and preserves user position after interaction.
"""

from unittest.mock import MagicMock, patch, PropertyMock

from macjet.ui.process_tree import ProcessTree, _cpu_color, _mem_color, _severity_rail, _format_mem
from macjet.collectors.process_collector import ProcessGroup, ProcessInfo


def _make_group(name: str, n: int = 1) -> ProcessGroup:
    """Helper to create a ProcessGroup with n dummy processes."""
    procs = []
    for i in range(n):
        procs.append(ProcessInfo(
            pid=1000 + i,
            name=f"{name}-{i}",
            cpu_percent=float(i),
            memory_mb=float(50 + i * 10),
            num_threads=2,
            confidence="exact",
        ))
    return ProcessGroup(
        name=name,
        icon="🟢",
        processes=procs,
        total_cpu=sum(p.cpu_percent for p in procs),
        total_memory_mb=sum(p.memory_mb for p in procs),
    )


def _make_groups(count: int = 30) -> dict[str, ProcessGroup]:
    """Create enough groups to overflow a terminal viewport."""
    groups = {}
    for i in range(count):
        name = f"App{i}"
        groups[name] = _make_group(name, n=1)
    return groups


class TestScrollPosition:
    """Regression tests for scroll-snap-to-bottom bug (GH fix)."""

    def test_user_moved_cursor_starts_false(self):
        tree = ProcessTree()
        assert tree._user_moved_cursor is False

    def test_toggle_selected_sets_user_moved(self):
        tree = ProcessTree()
        tree._user_moved_cursor = False
        # Mock a table so toggle_selected can run
        mock_table = MagicMock()
        mock_table.cursor_row = 0
        tree._table = mock_table
        tree._row_keys = ["group-test"]
        tree._current_groups = {"test": _make_group("test")}

        tree.toggle_selected()
        assert tree._user_moved_cursor is True

    def test_on_key_sets_user_moved_for_nav_keys(self):
        tree = ProcessTree()
        tree._user_moved_cursor = False

        for key in ("up", "down", "pageup", "pagedown", "home", "end", "j", "k"):
            tree._user_moved_cursor = False
            event = MagicMock()
            event.key = key
            tree.on_key(event)
            assert tree._user_moved_cursor is True, f"Key '{key}' should set _user_moved_cursor"

    def test_on_key_ignores_non_nav_keys(self):
        tree = ProcessTree()
        tree._user_moved_cursor = False

        for key in ("a", "b", "enter", "escape", "q"):
            event = MagicMock()
            event.key = key
            tree.on_key(event)
            assert tree._user_moved_cursor is False, f"Key '{key}' should NOT set _user_moved_cursor"

    def test_update_data_calls_scroll_home_when_idle(self):
        """When user hasn't interacted, update_data must pin to row 0 + scroll_home."""
        tree = ProcessTree()
        tree._user_moved_cursor = False

        mock_table = MagicMock()
        mock_table.cursor_row = 0
        tree._table = mock_table

        groups = _make_groups(30)
        tree.update_data(groups)

        # Should have called move_cursor(row=0) and scroll_home
        mock_table.move_cursor.assert_called_with(row=0, animate=False)
        mock_table.scroll_home.assert_called_once_with(animate=False)

    def test_update_data_restores_cursor_when_user_interacted(self):
        """When user has interacted, update_data must restore their cursor position."""
        tree = ProcessTree()
        tree._user_moved_cursor = True

        mock_table = MagicMock()
        mock_table.cursor_row = 5
        tree._table = mock_table

        # First populate row_keys so saved_cursor_key can be found
        tree._row_keys = [f"pid-{1000 + i}" for i in range(30)]

        groups = _make_groups(30)
        tree.update_data(groups)

        # Should NOT have called scroll_home
        mock_table.scroll_home.assert_not_called()

        # Should have called move_cursor with the restored position
        assert mock_table.move_cursor.called

    def test_update_data_clamps_on_shrink(self):
        """If table shrinks and old cursor is out of bounds, clamp to last row."""
        tree = ProcessTree()
        tree._user_moved_cursor = True

        mock_table = MagicMock()
        mock_table.cursor_row = 99  # Was on row 99 in old table
        tree._table = mock_table
        tree._row_keys = []  # Empty — will be rebuilt

        # Only 3 groups now
        groups = _make_groups(3)
        tree.update_data(groups)

        # Should clamp to last row (2)
        mock_table.move_cursor.assert_called_with(row=2, animate=False)


class TestHelpers:
    """Quick sanity checks on color/formatting helpers."""

    def test_cpu_color_ramp(self):
        assert _cpu_color(0) == "#22D3EE"
        assert _cpu_color(100) == "#FB7185"  # 100 > 80 → hot pink
        assert _cpu_color(999) == "#FB7185"

    def test_mem_color_ramp(self):
        assert _mem_color(50) == "#34D399"
        assert _mem_color(3000) == "#EF4444"

    def test_severity_rail(self):
        assert _severity_rail(0) == " "
        assert "█" in _severity_rail(200)

    def test_format_mem(self):
        assert _format_mem(512) == "512M"
        assert _format_mem(2048) == "2.0G"
