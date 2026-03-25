import time
import pytest
from unittest.mock import patch, MagicMock
from macjet.collectors.network_collector import (
    NetSnapshot,
    NetworkCollector,
    format_bytes_per_s,
    format_bytes,
)

def test_format_bytes_per_s():
    assert format_bytes_per_s(500) == "500 B/s"
    assert format_bytes_per_s(1024) == "1.0 KB/s"
    assert format_bytes_per_s(1500) == "1.5 KB/s"
    assert format_bytes_per_s(1024 * 1024) == "1.0 MB/s"
    assert format_bytes_per_s(1.5 * 1024 * 1024) == "1.5 MB/s"
    assert format_bytes_per_s(1024 * 1024 * 1024) == "1.0 GB/s"
    assert format_bytes_per_s(2.3 * 1024 * 1024 * 1024) == "2.3 GB/s"

def test_format_bytes():
    assert format_bytes(500) == "500 B"
    assert format_bytes(1024) == "1.0 KB"
    assert format_bytes(1500) == "1.5 KB"
    assert format_bytes(1024 * 1024) == "1.0 MB"
    assert format_bytes(1.5 * 1024 * 1024) == "1.5 MB"
    assert format_bytes(1024 * 1024 * 1024) == "1.0 GB"
    assert format_bytes(2.3 * 1024 * 1024 * 1024) == "2.3 GB"


@patch("macjet.collectors.network_collector.psutil.net_io_counters")
@patch("macjet.collectors.network_collector.time.time")
def test_network_collector_first_run(mock_time, mock_net):
    # Setup mocks
    mock_time.return_value = 1000.0
    mock_net.return_value = MagicMock(bytes_sent=10000, bytes_recv=20000)
    
    collector = NetworkCollector()
    snapshot = collector._collect_sync()
    
    assert snapshot.bytes_sent == 10000
    assert snapshot.bytes_recv == 20000
    assert snapshot.bytes_sent_per_s == 0.0  # First run, no prev values
    assert snapshot.bytes_recv_per_s == 0.0
    assert snapshot.timestamp == 1000.0
    
    assert collector.latest == snapshot

@patch("macjet.collectors.network_collector.psutil.net_io_counters")
@patch("macjet.collectors.network_collector.time.time")
def test_network_collector_second_run(mock_time, mock_net):
    collector = NetworkCollector()
    
    # First run (t=1000s)
    mock_time.return_value = 1000.0
    mock_net.return_value = MagicMock(bytes_sent=10000, bytes_recv=20000)
    collector._collect_sync()
    
    # Second run (t=1002s, +2 seconds dt)
    mock_time.return_value = 1002.0
    mock_net.return_value = MagicMock(bytes_sent=11000, bytes_recv=24000)
    snapshot = collector._collect_sync()
    
    assert snapshot.bytes_sent == 11000
    assert snapshot.bytes_recv == 24000
    # Sent 1000 bytes over 2s -> 500 B/s
    assert snapshot.bytes_sent_per_s == 500.0
    # Recv 4000 bytes over 2s -> 2000 B/s
    assert snapshot.bytes_recv_per_s == 2000.0
    assert snapshot.timestamp == 1002.0

@patch("macjet.collectors.network_collector.psutil.net_io_counters")
@patch("macjet.collectors.network_collector.time.time")
def test_network_collector_fast_dt_fix(mock_time, mock_net):
    """Test dt<=0 being floored to 1.0s"""
    collector = NetworkCollector()
    
    # First run (t=1000s)
    mock_time.return_value = 1000.0
    mock_net.return_value = MagicMock(bytes_sent=10000, bytes_recv=20000)
    collector._collect_sync()
    
    # Second run immediately! dt=0
    mock_time.return_value = 1000.0
    mock_net.return_value = MagicMock(bytes_sent=11000, bytes_recv=24000)
    snapshot = collector._collect_sync()
    
    # dt=0 should be corrected to dt=1.0, so:
    # Sent 1000 bytes over 1s -> 1000 B/s
    assert snapshot.bytes_sent_per_s == 1000.0

@pytest.mark.asyncio
@patch("macjet.collectors.network_collector.psutil.net_io_counters")
async def test_network_collector_async_collect(mock_net):
    mock_net.return_value = MagicMock(bytes_sent=5000, bytes_recv=5000)
    collector = NetworkCollector()
    
    snapshot = await collector.collect()
    assert snapshot.bytes_sent == 5000
