import asyncio
import pytest
from unittest.mock import AsyncMock, patch, MagicMock
from pathlib import Path
import psutil

from macjet.inspectors.ide_inspector import IDEInspector, IDEContext


def test_match_ide():
    inspector = IDEInspector()
    assert inspector._match_ide("CodeHelper (Renderer)") == "VSCode"
    assert inspector._match_ide("Cursor Helper") == "Cursor"
    assert inspector._match_ide("Xcode") == "Xcode"
    assert inspector._match_ide("idea") == "IntelliJ IDEA"
    assert inspector._match_ide("pycharm") == "PyCharm"
    assert inspector._match_ide("Brave Browser") is None


@pytest.mark.asyncio
async def test_inspect_unmatched():
    inspector = IDEInspector()
    ctx = await inspector.inspect("Safari", [], 123)
    assert ctx is None


@pytest.mark.asyncio
async def test_inspect_vscode_folder_uri():
    inspector = IDEInspector()
    cmdline = ["/opt/cursor", "--folder-uri=file:///Users/dev/my-project", "--no-sandbox"]
    ctx = await inspector.inspect("Cursor", cmdline, 123)
    
    assert ctx is not None
    assert ctx.ide_name == "Cursor"
    assert ctx.project_path == "/Users/dev/my-project"
    assert ctx.project_name == "my-project"
    assert ctx.confidence == "exact"


@pytest.mark.asyncio
@patch("macjet.inspectors.ide_inspector.psutil.Process")
@patch("macjet.inspectors.ide_inspector.IDEInspector._get_window_title")
async def test_inspect_vscode_fallback(mock_get_title, mock_process):
    mock_proc = MagicMock()
    mock_proc.cwd.return_value = "/Users/dev/fallback-project"
    mock_process.return_value = mock_proc
    
    mock_get_title.return_value = "main.py — some-other-project"

    inspector = IDEInspector()
    ctx = await inspector.inspect("Code", [], 123)
    
    assert ctx is not None
    assert ctx.ide_name == "VSCode"
    assert ctx.project_path == "/Users/dev/fallback-project"
    assert ctx.project_name == "fallback-project" 
    assert ctx.confidence == "inferred"
    assert ctx.active_file == "main.py"
    assert ctx.window_title == "main.py — some-other-project"


@pytest.mark.asyncio
@patch("macjet.inspectors.ide_inspector.IDEInspector._get_window_title")
async def test_inspect_xcode(mock_get_title):
    mock_get_title.return_value = "MacJet — App.swift"
    
    inspector = IDEInspector()
    ctx = await inspector.inspect("Xcode", [], 123)
    
    assert ctx is not None
    assert ctx.ide_name == "Xcode"
    assert ctx.project_name == "MacJet"
    assert ctx.active_file == "App.swift"
    assert ctx.confidence == "window-exact"


@pytest.mark.asyncio
def test_inspect_jetbrains(tmp_path):
    # Create a real directory to mock a jetbrains project path
    proj_dir = tmp_path / "my-java-project"
    proj_dir.mkdir()
    
    cmdline = ["java", "-jar", "idea.jar", str(proj_dir)]
    
    inspector = IDEInspector()
    # Need to await
    import asyncio
    ctx = asyncio.run(inspector.inspect("idea", cmdline, 123))
    
    assert ctx is not None
    assert ctx.ide_name == "IntelliJ IDEA"
    assert ctx.project_path == str(proj_dir)
    assert ctx.project_name == "my-java-project"
    assert ctx.confidence == "exact"


@pytest.mark.asyncio
@patch("macjet.inspectors.ide_inspector.asyncio.create_subprocess_exec")
async def test_get_window_title_success(mock_exec):
    mock_proc = AsyncMock()
    mock_proc.returncode = 0
    mock_proc.communicate.return_value = (b"My Window Title\n", b"")
    mock_exec.return_value = mock_proc
    
    inspector = IDEInspector()
    title = await inspector._get_window_title("Cursor")
    
    assert title == "My Window Title"

@pytest.mark.asyncio
@patch("macjet.inspectors.ide_inspector.asyncio.create_subprocess_exec")
async def test_get_window_title_failure(mock_exec):
    mock_proc = AsyncMock()
    mock_proc.returncode = 1
    mock_proc.communicate.return_value = (b"", b"Error")
    mock_exec.return_value = mock_proc
    
    inspector = IDEInspector()
    title = await inspector._get_window_title("Cursor")
    
    assert title == ""
