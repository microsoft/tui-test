from __future__ import annotations

import asyncio
import json
import os
from typing import Any, Dict, Optional, Tuple

from . import _config as cfg
from .errors import DaemonError

_Streams = Tuple[asyncio.StreamReader, asyncio.StreamWriter]


async def _open(session: str, home: Optional[str]) -> _Streams:
    path = cfg.socket_path(session, home)
    if cfg.IS_WINDOWS:
        loop = asyncio.get_running_loop()
        create = getattr(loop, "create_pipe_connection", None)
        if create is None:
            raise DaemonError(
                "named-pipe client requires the Proactor event loop on Windows "
                "(the default since Python 3.8)"
            )
        reader = asyncio.StreamReader()
        protocol = asyncio.StreamReaderProtocol(reader)
        transport, _ = await create(lambda: protocol, path)
        writer = asyncio.StreamWriter(transport, protocol, reader, loop)
        return reader, writer
    return await asyncio.open_unix_connection(path)


async def _close(writer: asyncio.StreamWriter) -> None:
    writer.close()
    try:
        await writer.wait_closed()
    except Exception:
        pass


async def can_connect(session: str, home: Optional[str]) -> bool:
    try:
        _, writer = await _open(session, home)
    except (FileNotFoundError, ConnectionRefusedError, OSError):
        return False
    await _close(writer)
    return True


async def ensure_daemon(session: str, home: Optional[str], binary: str) -> None:
    if await can_connect(session, home):
        return
    env = dict(os.environ)
    if home:
        env["SHELL_USE_HOME"] = home
    try:
        proc = await asyncio.create_subprocess_exec(
            binary,
            "--session",
            session,
            "daemon",
            "start",
            stdout=asyncio.subprocess.DEVNULL,
            stderr=asyncio.subprocess.DEVNULL,
            env=env,
        )
    except FileNotFoundError:
        raise DaemonError(
            f"could not find the '{binary}' binary on PATH; "
            "set SHELL_USE_BIN or pass binary="
        )
    await proc.wait()
    for _ in range(100):
        if await can_connect(session, home):
            return
        await asyncio.sleep(0.05)
    raise DaemonError(f"daemon for session '{session}' did not become ready")


async def request(
    session: str,
    home: Optional[str],
    binary: str,
    payload: Dict[str, Any],
    *,
    autostart: bool = True,
) -> Dict[str, Any]:
    if autostart:
        await ensure_daemon(session, home, binary)
    try:
        reader, writer = await _open(session, home)
    except (FileNotFoundError, ConnectionRefusedError, OSError) as e:
        raise DaemonError(f"could not connect to session '{session}': {e}")
    try:
        writer.write(json.dumps(payload).encode("utf-8") + b"\n")
        await writer.drain()
        line = await reader.readline()
    finally:
        await _close(writer)
    if not line:
        raise DaemonError("daemon closed the connection without responding")
    try:
        return json.loads(line.decode("utf-8"))
    except json.JSONDecodeError as e:
        raise DaemonError(f"invalid response from daemon: {e}")
