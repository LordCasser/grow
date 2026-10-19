use std::path::PathBuf;

use agent_client_protocol::schema::v1 as acp;
use mcp::servers::validate_mcp_server_names;

fn http(name: &str, url: &str) -> acp::McpServer {
    acp::McpServer::Http(acp::McpServerHttp::new(name.to_owned(), url.to_owned()).headers(vec![]))
}

#[test]
fn validate_mcp_server_names_rejects_duplicates_across_endpoint_and_transport() {
    let duplicate_urls = vec![
        http("shared", "https://first.example/mcp"),
        http("shared", "https://second.example/mcp"),
    ];
    let duplicate_transports = vec![
        http("shared", "https://shared.example/mcp"),
        acp::McpServer::Stdio(acp::McpServerStdio::new(
            "shared".to_owned(),
            PathBuf::from("server"),
        )),
    ];

    for servers in [&duplicate_urls, &duplicate_transports] {
        assert_eq!(
            validate_mcp_server_names(servers).unwrap_err(),
            "duplicate MCP server name 'shared'"
        );
    }
}
