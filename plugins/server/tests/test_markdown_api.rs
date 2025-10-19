use axum::http::Method;
use rune_server::simple_live_editor::MarkdownRenderHandler;
use rune_server::{HttpHandler, HttpRequest, HttpResponse};

#[tokio::test]
async fn test_markdown_render_api() {
    let handler = MarkdownRenderHandler::new();

    // Test POST request with markdown content
    let request_body = serde_json::json!({
        "markdown": "# Hello World\n\nThis is a **test** with *emphasis*."
    });

    let request = HttpRequest {
        method: Method::POST,
        path: "/api/render-markdown".to_string(),
        query_params: std::collections::HashMap::new(),
        headers: axum::http::HeaderMap::new(),
        body: serde_json::to_vec(&request_body).unwrap(),
        path_params: std::collections::HashMap::new(),
    };

    let response = handler.handle(request).await.unwrap();

    // Parse the response body
    let response_data: serde_json::Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response_data["success"], true);
    assert!(response_data["html"].as_str().unwrap().contains("<h1>"));
    assert!(response_data["html"]
        .as_str()
        .unwrap()
        .contains("Hello World"));
    assert!(response_data["html"]
        .as_str()
        .unwrap()
        .contains("<strong>test</strong>"));
    assert!(response_data["html"]
        .as_str()
        .unwrap()
        .contains("<em>emphasis</em>"));
}

#[tokio::test]
async fn test_markdown_render_api_invalid_method() {
    let handler = MarkdownRenderHandler::new();

    let request = HttpRequest {
        method: Method::GET,
        path: "/api/render-markdown".to_string(),
        query_params: std::collections::HashMap::new(),
        headers: axum::http::HeaderMap::new(),
        body: Vec::new(),
        path_params: std::collections::HashMap::new(),
    };

    let response = handler.handle(request).await.unwrap();

    // Should return method not allowed error
    assert_eq!(response.status, axum::http::StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn test_markdown_render_api_invalid_json() {
    let handler = MarkdownRenderHandler::new();

    let request = HttpRequest {
        method: Method::POST,
        path: "/api/render-markdown".to_string(),
        query_params: std::collections::HashMap::new(),
        headers: axum::http::HeaderMap::new(),
        body: b"invalid json".to_vec(),
        path_params: std::collections::HashMap::new(),
    };

    let response = handler.handle(request).await.unwrap();

    // Parse the response body
    let response_data: serde_json::Value = serde_json::from_slice(&response.body).unwrap();

    assert_eq!(response_data["success"], false);
    assert!(response_data["error"].is_string());
}
