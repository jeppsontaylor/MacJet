"""
MacJet MCP — Async TTL Cache for collector results.
Prevents agent burst calls from pegging the CPU with repeated psutil scans.
"""

from __future__ import annotations

import asyncio
import time
from typing import Any, Callable, Coroutine


class AsyncTTLCache:
    """Simple TTL cache for async functions.

    Usage:
        cache = AsyncTTLCache(ttl=2.0)
        result = await cache.get("processes", collector.collect)
    """

    def __init__(self, ttl: float = 2.0):
        self._ttl = ttl
        self._cache: dict[str, tuple[float, Any]] = {}
        self._locks: dict[str, asyncio.Lock] = {}

    async def get(self, key: str, factory: Callable[[], Coroutine[Any, Any, Any]]) -> Any:
        """Get a cached value or compute it if stale/missing."""
        now = time.monotonic()

        # Fast path: cache hit
        if key in self._cache:
            ts, value = self._cache[key]
            if now - ts < self._ttl:
                return value

        # Slow path: compute under lock to avoid thundering herd
        if key not in self._locks:
            self._locks[key] = asyncio.Lock()

        async with self._locks[key]:
            # Double-check after acquiring lock
            if key in self._cache:
                ts, value = self._cache[key]
                if now - ts < self._ttl:
                    return value

            value = await factory()
            self._cache[key] = (time.monotonic(), value)
            return value

    def invalidate(self, key: str | None = None) -> None:
        """Invalidate a specific key or all keys."""
        if key is None:
            self._cache.clear()
        else:
            self._cache.pop(key, None)
