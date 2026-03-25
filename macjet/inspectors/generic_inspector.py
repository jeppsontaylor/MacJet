"""
MacJet — Generic Inspector
Parses command-line arguments for node, python, java, ruby, go, etc.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Optional


@dataclass
class GenericContext:
    label: str = ""
    exe_path: str = ""
    script_path: str = ""
    args: str = ""
    cwd: str = ""
    confidence: str = "exact"


class GenericInspector:
    """Parses cmdline args for common CLI tools."""

    _SCRIPT_RUNTIMES = {
        "node": "node",
        "Node": "node",
        "python": "python",
        "python3": "python",
        "Python": "python",
        "ruby": "ruby",
        "Ruby": "ruby",
        "perl": "perl",
        "java": "java",
        "go": "go",
        "deno": "deno",
        "bun": "bun",
        "php": "php",
        "rust": "cargo",
    }

    def inspect(
        self, process_name: str, cmdline: list[str], cwd: str = "", exe: str = ""
    ) -> Optional[GenericContext]:
        """Extract context from process command line."""
        runtime = self._match_runtime(process_name)
        if not runtime and not cmdline:
            return None

        ctx = GenericContext(
            exe_path=exe,
            cwd=cwd,
        )

        if runtime:
            ctx = self._parse_runtime(runtime, cmdline, cwd)
        elif cmdline:
            # Unknown process — show first meaningful arg
            ctx.label = " ".join(cmdline[:3])
            if len(cmdline) > 3:
                ctx.label += " ..."
            ctx.confidence = "exact"

        return ctx

    def _match_runtime(self, name: str) -> Optional[str]:
        for pattern, runtime in self._SCRIPT_RUNTIMES.items():
            if pattern in name:
                return runtime
        return None

    def _parse_runtime(self, runtime: str, cmdline: list[str], cwd: str) -> GenericContext:
        """Parse runtime-specific cmdline patterns."""
        ctx = GenericContext(cwd=cwd, confidence="exact")

        if runtime in ("node", "deno", "bun"):
            ctx = self._parse_node(cmdline, cwd, runtime)
        elif runtime == "python":
            ctx = self._parse_python(cmdline, cwd)
        elif runtime == "java":
            ctx = self._parse_java(cmdline, cwd)
        elif runtime in ("ruby", "perl", "php"):
            ctx = self._parse_scripting(cmdline, cwd, runtime)
        else:
            ctx.label = " ".join(cmdline[:3])

        return ctx

    def _parse_node(self, cmdline: list[str], cwd: str, runtime: str = "node") -> GenericContext:
        ctx = GenericContext(cwd=cwd, confidence="exact")
        # Skip flags, find the script arg
        for arg in cmdline[1:] if len(cmdline) > 1 else []:
            if arg.startswith("-"):
                continue
            # Could be a path or a module name
            ctx.script_path = arg
            # Make relative to cwd if possible
            p = Path(arg)
            if not p.is_absolute() and cwd:
                display = arg
            elif p.is_absolute():
                display = (
                    f"~/{p.relative_to(Path.home())}"
                    if str(p).startswith(str(Path.home()))
                    else arg
                )
            else:
                display = arg
            ctx.label = f"{runtime} {display}"
            return ctx

        ctx.label = runtime
        ctx.confidence = "grouped"
        return ctx

    def _parse_python(self, cmdline: list[str], cwd: str) -> GenericContext:
        ctx = GenericContext(cwd=cwd, confidence="exact")

        i = 1
        while i < len(cmdline):
            arg = cmdline[i]
            if arg == "-m" and i + 1 < len(cmdline):
                ctx.label = f"python -m {cmdline[i+1]}"
                ctx.script_path = cmdline[i + 1]
                return ctx
            elif arg == "-c":
                ctx.label = "python -c <inline>"
                return ctx
            elif not arg.startswith("-"):
                p = Path(arg)
                display = arg
                if p.is_absolute() and str(p).startswith(str(Path.home())):
                    display = f"~/{p.relative_to(Path.home())}"
                ctx.label = f"python {display}"
                ctx.script_path = arg
                return ctx
            i += 1

        ctx.label = "python"
        ctx.confidence = "grouped"
        return ctx

    def _parse_java(self, cmdline: list[str], cwd: str) -> GenericContext:
        ctx = GenericContext(cwd=cwd, confidence="exact")

        for i, arg in enumerate(cmdline):
            if arg == "-jar" and i + 1 < len(cmdline):
                jar = Path(cmdline[i + 1]).name
                ctx.label = f"java -jar {jar}"
                ctx.script_path = cmdline[i + 1]
                return ctx

        # Look for main class (last non-flag argument)
        for arg in reversed(cmdline):
            if not arg.startswith("-") and "." in arg and "/" not in arg:
                ctx.label = f"java {arg}"
                return ctx

        ctx.label = "java"
        ctx.confidence = "grouped"
        return ctx

    def _parse_scripting(self, cmdline: list[str], cwd: str, runtime: str) -> GenericContext:
        ctx = GenericContext(cwd=cwd, confidence="exact")

        for arg in cmdline[1:] if len(cmdline) > 1 else []:
            if not arg.startswith("-"):
                ctx.label = f"{runtime} {Path(arg).name}"
                ctx.script_path = arg
                return ctx

        ctx.label = runtime
        ctx.confidence = "grouped"
        return ctx
