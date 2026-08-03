from __future__ import annotations

import atexit
import os
import re
import secrets
import shutil
import tempfile
import threading
from typing import Optional, Set

_session_counter = 0
_session_counter_lock = threading.Lock()


def unique_session(prefix: Optional[str] = None) -> str:
    global _session_counter
    with _session_counter_lock:
        _session_counter += 1
        counter = _session_counter
    base = re.sub(r"[^A-Za-z0-9_-]", "-", prefix or "shell-use")
    suffix = "-{}-{}-{}".format(os.getpid(), secrets.token_hex(4), counter)
    max_prefix = 64 - len(suffix)
    if max_prefix < 1:
        return (base + suffix)[:64]
    return (base[:max_prefix] + suffix)[:64]


_temp_homes = set()  # type: Set[str]
_temp_homes_lock = threading.Lock()
_sweeper_registered = False


def _register_sweeper() -> None:
    global _sweeper_registered
    if _sweeper_registered:
        return
    _sweeper_registered = True
    atexit.register(_sweep_temp_homes)


def provision_temp_home() -> str:
    """Create and register a private temp directory to use as a daemon home."""
    path = tempfile.mkdtemp(prefix="shell-use-")
    with _temp_homes_lock:
        _temp_homes.add(path)
    _register_sweeper()
    return path


def remove_temp_home(path: str) -> None:
    """Best-effort removal of a previously provisioned temp home."""
    with _temp_homes_lock:
        _temp_homes.discard(path)
    shutil.rmtree(path, ignore_errors=True)


def _sweep_temp_homes() -> None:
    with _temp_homes_lock:
        paths = list(_temp_homes)
        _temp_homes.clear()
    for path in paths:
        shutil.rmtree(path, ignore_errors=True)
