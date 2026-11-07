# tui-test

Control, inspect, test, and record real shell sessions and full-screen terminal
apps on Windows, Linux, and macOS. Use the CLI or the in-process Rust, Python,
JavaScript, and Go APIs for terminal automation, application tests, and AI agents.

[Documentation](https://tui-test.dev/docs/getting-started/overview/) |
[Installation](https://tui-test.dev/docs/install/) |
[Writing tests](https://tui-test.dev/docs/getting-started/writing-tests/) |
[CLI reference](https://tui-test.dev/docs/reference/cli/)

## Installation

macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/microsoft/tui-test/main/install/install.sh | sh
```

Windows:

```powershell
irm https://raw.githubusercontent.com/microsoft/tui-test/main/install/install.ps1 | iex
```

Or install with Homebrew:

```sh
brew tap microsoft/tui-test https://github.com/microsoft/tui-test
brew install tui-test
```

Platform archives are available from [GitHub Releases](https://github.com/microsoft/tui-test/releases).

| Language | Install | Reference |
| --- | --- | --- |
| Rust 1.90+ | `cargo add tui-test-rs@0.1.1` | [Rust](https://tui-test.dev/docs/api/rust/) |
| Python 3.8+ | `pip install tui-test` | [Python](https://tui-test.dev/docs/api/python/) |
| Node.js 20+ | `npm install @microsoft/tui-test` | [JavaScript and TypeScript](https://tui-test.dev/docs/api/javascript/) |
| Go 1.26+ | `go get github.com/microsoft/tui-test/bindings/go@v0.1.1` | [Go](https://tui-test.dev/docs/api/go/) |

Library sessions do not require the CLI. See the
[installation guide](https://tui-test.dev/docs/install/) for native runtime
requirements and recording features.

## Quick start

```sh
tui-test open
tui-test submit "echo hello"
tui-test wait command
tui-test expect output "hello"
tui-test screenshot -o result.svg
tui-test close
```

Continue with [your first session](https://tui-test.dev/docs/getting-started/first-session/),
[locators](https://tui-test.dev/docs/guides/locators/), or
[configuration](https://tui-test.dev/docs/guides/configuration/).
The docs also cover [recordings](https://tui-test.dev/docs/tools/recordings/),
[tapes](https://tui-test.dev/docs/tools/tapes/),
[trace reports](https://tui-test.dev/docs/tools/trace-viewer/), and
[AI agent commands](https://tui-test.dev/docs/tools/ai-agents/).

## Contributing

See the [contributor guide](https://tui-test.dev/docs/contributing/) for builds,
checks, binding generation, and documentation changes.

Most contributions require a Contributor License Agreement (CLA) declaring that
you have the right to grant us the rights to use your contribution.
See https://cla.opensource.microsoft.com for details. The CLA bot will tell you
whether an agreement is needed when you open a pull request. You only need to
complete it once across repositories using our CLA.

This project follows the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
See the [FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or contact
[opencode@microsoft.com](mailto:opencode@microsoft.com) with questions.
Use [GitHub Issues](https://github.com/microsoft/tui-test/issues) for bugs and
feature requests, and [GitHub Discussions](https://github.com/microsoft/tui-test/discussions)
for help. Report security vulnerabilities as described in [SECURITY.md](SECURITY.md).

## Trademarks

This project may contain trademarks or logos for projects, products, or services.
Authorized use of Microsoft trademarks or logos is subject to
[Microsoft's Trademark & Brand Guidelines](https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks/usage/general).
Use of Microsoft trademarks or logos in modified versions of this project must
not cause confusion or imply Microsoft sponsorship.
Third-party trademarks and logos are subject to their owners' policies.
