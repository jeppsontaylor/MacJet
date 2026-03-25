import asyncio
import json
import pytest
from unittest.mock import AsyncMock, patch
from macjet.inspectors.container_inspector import ContainerInspector, ContainerInfo


def test_parse_mem():
    assert ContainerInspector._parse_mem("1024.0MiB") == 1024.0
    assert ContainerInspector._parse_mem("2.5GiB") == 2.5 * 1024
    assert ContainerInspector._parse_mem("512.0KiB") == 512.0 / 1024
    assert ContainerInspector._parse_mem("1048576B") == 1048576 / (1024 * 1024)
    assert ContainerInspector._parse_mem("Invalid") == 0.0


def test_find_container_for_process():
    inspector = ContainerInspector()
    c1 = ContainerInfo(name="redis-cache")
    c2 = ContainerInfo(name="postgres-db")
    inspector._containers = [c1, c2]

    assert inspector.find_container_for_process("redis") == c1
    assert inspector.find_container_for_process("POSTGRES") == c2
    assert inspector.find_container_for_process("nginx") is None

    # Partial match
    assert inspector.find_container_for_process("redis-cache-server").name == "redis-cache"


@pytest.mark.asyncio
async def test_inspect_docker_unavailable():
    inspector = ContainerInspector()
    inspector._docker_available = False
    
    result = await inspector.inspect()
    assert result == []

@pytest.mark.asyncio
@patch("macjet.inspectors.container_inspector.asyncio.create_subprocess_exec")
async def test_query_docker_stats_success(mock_exec):
    mock_proc = AsyncMock()
    mock_proc.returncode = 0
    
    # Mock output of docker stats
    mock_output = [
        json.dumps({"name": "redis", "id": "123", "cpu": "1.5%", "mem_usage": "100MiB / 2GiB", "net": "1kB / 2kB", "status": "running"}),
        json.dumps({"name": "postgres", "id": "456", "cpu": "3.5%", "mem_usage": "500MiB / 4GiB", "net": "10kB / 20kB", "status": "running"}),
        "", # Empty line
        "invalid json", # Bad line
    ]
    mock_proc.communicate.return_value = ("\n".join(mock_output).encode(), b"")
    mock_exec.return_value = mock_proc

    # In Python 3.13, wait_for acts on the coroutine. We patch communicate to return directly if awaited.
    
    inspector = ContainerInspector()
    containers = await inspector.inspect()
    
    assert len(containers) == 2
    assert inspector._docker_available is True
    assert inspector.containers == containers

    assert containers[0].name == "redis"
    assert containers[0].cpu_percent == 1.5
    assert containers[0].memory_mb == 100.0
    assert containers[0].net_input == "1kB"
    assert containers[0].net_output == "2kB"

    assert containers[1].name == "postgres"
    assert containers[1].cpu_percent == 3.5
    assert containers[1].memory_mb == 500.0


@pytest.mark.asyncio
@patch("macjet.inspectors.container_inspector.asyncio.create_subprocess_exec")
async def test_query_docker_stats_failure(mock_exec):
    mock_proc = AsyncMock()
    mock_proc.returncode = 1
    mock_proc.communicate.return_value = (b"", b"error")
    mock_exec.return_value = mock_proc

    inspector = ContainerInspector()
    containers = await inspector.inspect()
    
    assert len(containers) == 0
    assert inspector._docker_available is False


@pytest.mark.asyncio
@patch("macjet.inspectors.container_inspector.asyncio.create_subprocess_exec")
async def test_query_docker_stats_exception(mock_exec):
    mock_exec.side_effect = FileNotFoundError("docker not found")

    inspector = ContainerInspector()
    containers = await inspector.inspect()
    
    assert len(containers) == 0
    assert inspector._docker_available is False
