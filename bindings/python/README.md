# tui-test for Python

Control, inspect, test, and record terminal apps from Python 3.8 or newer.
The wheel includes the native engine; the CLI is not required.

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

See the [Python API](https://tui-test.dev/docs/api/python/) for all methods,
options, cancellation behavior, and source builds.
Start with [writing tests](https://tui-test.dev/docs/getting-started/writing-tests/)
or the [locator guide](https://tui-test.dev/docs/guides/locators/).
