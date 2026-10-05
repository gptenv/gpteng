# gpteng

`gpteng` is the native desktop client and process host for the `libgpteng`
engine library. Running it without arguments opens the desktop window.

```sh
cargo run
cargo run -- --mcp-stdio
cargo run -- --mcp-http 127.0.0.1:8000
cargo run -- --webmcp
cargo run -- --desktop --mcp-http --webmcp
```

Modes can be combined. When an MCP mode is selected, the desktop window is
suppressed unless `--desktop` is also present. The HTTP listener hosts
Streamable HTTP MCP at `/mcp`; `--webmcp` also hosts a browser tool producer at
`/webmcp` backed by the same engine tool catalog and dispatcher.
