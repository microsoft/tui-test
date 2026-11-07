# tui-test for Go

Control, inspect, test, and record terminal apps from Go 1.26 or newer.
Published modules include the native engine and do not require Rust, cgo, or the CLI.

```sh
go get github.com/microsoft/tui-test/bindings/go@v0.1.1
go run github.com/microsoft/tui-test/bindings/go/examples/smoke
```

Supported targets are Linux and macOS on `amd64` and `arm64`, and Windows on
`amd64`. Linux requires glibc 2.28+ or musl 1.2+; PureGo still uses the system
dynamic loader.

See the [Go API](https://tui-test.dev/docs/api/go/) for examples, all methods,
native engine caching, cross-compilation, and test helpers.
For source checkouts, see [building from source](https://tui-test.dev/docs/contributing/#build-from-source).
