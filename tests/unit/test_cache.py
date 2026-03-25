"""
Tests for AsyncTTLCache — TTL expiry, thundering herd protection, invalidation.
"""
from __future__ import annotations

import asyncio
import time
from unittest.mock import AsyncMock, patch

import pytest

from macjet.mcp.cache import AsyncTTLCache


@pytest.mark.asyncio
class TestAsyncTTLCache:
    """Verify cache hit/miss behavior, TTL expiry, and invalidation."""

    async def test_cache_hit_returns_same_value(self):
        cache = AsyncTTLCache(ttl=10.0)
        factory = AsyncMock(return_value={"data": "result"})

        first = await cache.get("key", factory)
        second = await cache.get("key", factory)

        assert first == second
        factory.assert_called_once()  # Only computed once

    async def test_cache_miss_after_ttl_expiry(self):
        cache = AsyncTTLCache(ttl=0.1)
        call_count = 0

        async def factory():
            nonlocal call_count
            call_count += 1
            return call_count

        first = await cache.get("key", factory)
        await asyncio.sleep(0.15)  # Wait for TTL to expire
        second = await cache.get("key", factory)

        assert first == 1
        assert second == 2

    async def test_different_keys_cached_independently(self):
        cache = AsyncTTLCache(ttl=10.0)
        factory_a = AsyncMock(return_value="a")
        factory_b = AsyncMock(return_value="b")

        result_a = await cache.get("key_a", factory_a)
        result_b = await cache.get("key_b", factory_b)

        assert result_a == "a"
        assert result_b == "b"
        factory_a.assert_called_once()
        factory_b.assert_called_once()

    async def test_invalidate_specific_key(self):
        cache = AsyncTTLCache(ttl=10.0)
        factory = AsyncMock(return_value="value")

        await cache.get("key", factory)
        cache.invalidate("key")
        await cache.get("key", factory)

        assert factory.call_count == 2

    async def test_invalidate_all_keys(self):
        cache = AsyncTTLCache(ttl=10.0)
        factory_a = AsyncMock(return_value="a")
        factory_b = AsyncMock(return_value="b")

        await cache.get("a", factory_a)
        await cache.get("b", factory_b)
        cache.invalidate()  # Clear all
        await cache.get("a", factory_a)
        await cache.get("b", factory_b)

        assert factory_a.call_count == 2
        assert factory_b.call_count == 2
