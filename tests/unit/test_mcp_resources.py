import pytest
from unittest.mock import Mock, patch
from macjet.mcp.models import (
    SystemOverview,
    ProcessListResult,
    ProcessDetail,
    ChromeTabsResult,
    EnergyReport,
)
from macjet.mcp.resources import (
    resource_system_overview,
    resource_processes_top,
    resource_process_by_name,
    resource_chrome_tabs,
    resource_energy_report,
    resource_audit_log,
)

@pytest.fixture
def mock_deps():
    return {
        "proc_collector": Mock(),
        "energy_collector": Mock(),
        "chrome_mapper": Mock(),
        "cache": Mock(),
    }

@pytest.mark.asyncio
@patch("macjet.mcp.resources.handle_get_system_overview")
async def test_resource_system_overview(mock_handler, mock_deps):
    mock_model = SystemOverview(
        cpu_percent=10.0,
        memory_used_gb=8.0,
        memory_total_gb=16.0,
        memory_percent=50.0,
        thermal_pressure="nominal",
        top_process="foo",
        top_cpu_percent=5.0,
        process_count=100,
        verdict="OK"
    )
    mock_handler.return_value = mock_model
    
    result = await resource_system_overview(**mock_deps)
    assert isinstance(result, str)
    assert "cpu_percent" in result
    assert "10.0" in result
    mock_handler.assert_called_once_with(
        mock_deps["proc_collector"],
        mock_deps["energy_collector"],
        mock_deps["chrome_mapper"],
        mock_deps["cache"]
    )

@pytest.mark.asyncio
@patch("macjet.mcp.resources.handle_list_processes")
async def test_resource_processes_top(mock_handler, mock_deps):
    mock_model = ProcessListResult(
        groups=[],
        total_groups=0,
        sort_by="cpu",
    )
    mock_handler.return_value = mock_model
    
    result = await resource_processes_top(**mock_deps)
    assert isinstance(result, str)
    assert "groups" in result
    mock_handler.assert_called_once_with(
        mock_deps["proc_collector"],
        mock_deps["energy_collector"],
        mock_deps["chrome_mapper"],
        mock_deps["cache"],
        limit=25
    )

@pytest.mark.asyncio
@patch("macjet.mcp.resources.handle_get_process_detail")
async def test_resource_process_by_name(mock_handler, mock_deps):
    mock_model = ProcessDetail(
        name="testapp",
        total_cpu=5.0,
        total_memory_mb=100.0,
        process_count=1,
        children=[]
    )
    mock_handler.return_value = mock_model
    
    result = await resource_process_by_name(**mock_deps, name="testapp")
    assert isinstance(result, str)
    assert "testapp" in result
    mock_handler.assert_called_once_with(
        mock_deps["proc_collector"],
        mock_deps["energy_collector"],
        mock_deps["chrome_mapper"],
        mock_deps["cache"],
        name="testapp"
    )

@pytest.mark.asyncio
@patch("macjet.mcp.resources.handle_get_chrome_tabs")
async def test_resource_chrome_tabs(mock_handler):
    mock_model = ChromeTabsResult(tabs=[], total_tabs=0, cdp_connected=True)
    mock_handler.return_value = mock_model
    mock_mapper = Mock()
    
    result = await resource_chrome_tabs(chrome_mapper=mock_mapper)
    assert isinstance(result, str)
    mock_handler.assert_called_once_with(mock_mapper)

@pytest.mark.asyncio
@patch("macjet.mcp.resources.handle_get_energy_report")
async def test_resource_energy_report(mock_handler):
    mock_model = EnergyReport(available=False, entries=[])
    mock_handler.return_value = mock_model
    mock_collector = Mock()
    
    result = await resource_energy_report(energy_collector=mock_collector)
    assert isinstance(result, str)
    mock_handler.assert_called_once_with(mock_collector)

@pytest.mark.asyncio
@patch("macjet.mcp.safety.get_audit_log")
async def test_resource_audit_log(mock_safety):
    mock_safety.return_value = "Audit log content"
    result = await resource_audit_log()
    assert result == "Audit log content"
    mock_safety.assert_called_once_with(limit=50)
