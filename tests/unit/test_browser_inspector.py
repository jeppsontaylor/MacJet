import asyncio
import json
import pytest
from unittest.mock import AsyncMock, patch

from macjet.inspectors.browser_inspector import BrowserInspector, BrowserContext, TabInfo

@pytest.fixture
def inspector():
    return BrowserInspector()

@pytest.mark.asyncio
async def test_inspect_unsupported_browser(inspector):
    ctx = await inspector.inspect("UnknownBrowser")
    assert ctx is None

@pytest.mark.asyncio
@patch("macjet.inspectors.browser_inspector.BrowserInspector._try_cdp")
@patch("macjet.inspectors.browser_inspector.BrowserInspector._query_applescript")
async def test_inspect_chromium_cdp_success(mock_as, mock_cdp, inspector):
    mock_ctx = BrowserContext(app_name="Chrome (CDP)", tabs=[])
    mock_cdp.return_value = mock_ctx
    
    ctx = await inspector.inspect("Google Chrome Helper")
    assert ctx is mock_ctx
    mock_cdp.assert_called_once()
    mock_as.assert_not_called()

@pytest.mark.asyncio
@patch("macjet.inspectors.browser_inspector.BrowserInspector._try_cdp")
@patch("macjet.inspectors.browser_inspector.BrowserInspector._query_applescript")
async def test_inspect_chromium_fallback(mock_as, mock_cdp, inspector):
    mock_cdp.return_value = None
    mock_ctx = BrowserContext(app_name="Google Chrome", tabs=[])
    mock_as.return_value = mock_ctx
    
    ctx = await inspector.inspect("Brave Browser")
    assert ctx is mock_ctx
    mock_cdp.assert_called_once()
    mock_as.assert_called_once_with("Brave Browser")

@pytest.mark.asyncio
@patch("macjet.inspectors.browser_inspector.BrowserInspector._try_cdp")
@patch("macjet.inspectors.browser_inspector.BrowserInspector._query_applescript")
async def test_inspect_safari(mock_as, mock_cdp, inspector):
    mock_ctx = BrowserContext(app_name="Safari", tabs=[])
    mock_as.return_value = mock_ctx
    
    ctx = await inspector.inspect("Safari")
    assert ctx is mock_ctx
    mock_cdp.assert_not_called()  # Never tries CDP for Safari
    mock_as.assert_called_once_with("Safari")


@pytest.mark.asyncio
@patch("macjet.inspectors.browser_inspector.asyncio.create_subprocess_exec")
async def test_try_cdp_success(mock_exec, inspector):
    mock_proc = AsyncMock()
    mock_proc.returncode = 0
    
    mock_output = [
        {"type": "page", "title": "GitHub", "url": "https://github.com"},
        {"type": "background_page", "title": "Extension", "url": "chrome-extension://..."},
    ]
    mock_proc.communicate.return_value = (json.dumps(mock_output).encode(), b"")
    mock_exec.return_value = mock_proc
    
    ctx = await inspector._try_cdp()
    assert ctx is not None
    assert ctx.app_name == "Chrome (CDP)"
    assert len(ctx.tabs) == 1
    assert ctx.tabs[0].title == "GitHub"


@pytest.mark.asyncio
@patch("macjet.inspectors.browser_inspector.asyncio.create_subprocess_exec")
async def test_try_cdp_failure(mock_exec, inspector):
    mock_proc = AsyncMock()
    mock_proc.returncode = 7  # curl connection refused
    mock_proc.communicate.return_value = (b"", b"Failed to connect")
    mock_exec.return_value = mock_proc
    
    ctx = await inspector._try_cdp()
    assert ctx is None


@pytest.mark.asyncio
@patch("macjet.inspectors.browser_inspector.asyncio.create_subprocess_exec")
async def test_query_applescript_success(mock_exec, inspector):
    mock_proc = AsyncMock()
    mock_proc.returncode = 0
    
    # Format: window_idx \t title \t url \t isActive \n
    mock_output = "1\tTest Title\thttp://test.com\ttrue\n1\tOther Tab\thttp://other.com\tfalse\n"
    mock_proc.communicate.return_value = (mock_output.encode(), b"")
    mock_exec.return_value = mock_proc
    
    ctx = await inspector._query_applescript("Google Chrome")
    assert ctx is not None
    assert ctx.app_name == "Google Chrome"
    assert ctx.window_count == 1
    assert len(ctx.tabs) == 2
    assert ctx.active_tab is not None
    assert ctx.active_tab.title == "Test Title"

    # Should be cached
    assert inspector.get_cached("Google Chrome") is ctx


@pytest.mark.asyncio
@patch("macjet.inspectors.browser_inspector.asyncio.create_subprocess_exec")
async def test_query_applescript_failure(mock_exec, inspector):
    mock_proc = AsyncMock()
    mock_proc.returncode = 1
    mock_proc.communicate.return_value = (b"", b"AppleEvent timed out")
    mock_exec.return_value = mock_proc
    
    ctx = await inspector._query_applescript("Google Chrome")
    assert ctx is None


def test_get_cached(inspector):
    ctx = BrowserContext(app_name="Safari", tabs=[])
    inspector._cache["Safari"] = ctx
    
    assert inspector.get_cached("Safari Web Content") is ctx
    assert inspector.get_cached("Unknown") is None
