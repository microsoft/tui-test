from __future__ import annotations

import os
import re
import secrets
import threading
from typing import Optional

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
