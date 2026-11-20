# tui-test for Python

Control, inspect, test, and record terminal apps from Python 3.8 or newer. Published wheels include the prebuilt native engine without requiring the `tui-test` CLI.

```sh
pip install tui-test
```

```python
import asyncio
import sys
from tui_test import TuiTest

async def main():
    async with TuiTest.ephemeral() as terminal:
        await terminal.run(sys.executable, "-c", "print('Ready')")
        await terminal.get_by_text("Ready").expect()
        await terminal.wait_exit()

asyncio.run(main())
```

See the [Python API reference](https://tui-test.dev/docs/api/python/) for all methods, test helpers, recordings, cancellation behavior, and source builds, or start with [writing and running tests](https://tui-test.dev/docs/getting-started/testing/) and [locators](https://tui-test.dev/docs/guides/locators/).
