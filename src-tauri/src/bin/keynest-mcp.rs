//! keynest-mcp: KeyNest の MCP サーバー(stdio)。
//! 登録例: `claude mcp add keynest -- <path>/keynest-mcp`
//! 実装は keynest_lib::mcp にある。

fn main() -> std::process::ExitCode {
    keynest_lib::mcp::main()
}
