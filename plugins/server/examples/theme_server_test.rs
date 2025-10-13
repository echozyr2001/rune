//! Test theme server handlers

use axum::http::{HeaderMap, Method};
use rune_core::event::InMemoryEventBus;
use rune_server::handlers::{ThemeApiHandler, ThemeAssetHandler, ThemeInfoHandler};
use rune_server::{HttpHandler, HttpRequest};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🌐 Testing theme server handlers");

    // Create event bus
    let event_bus = Arc::new(InMemoryEventBus::new());

    // Test theme asset handler
    println!("\n📁 Testing theme asset handler...");
    let theme_asset_handler =
        ThemeAssetHandler::with_event_bus("/themes".to_string(), event_bus.clone());

    // Test theme list request
    let list_request = HttpRequest {
        method: Method::GET,
        path: "/themes".to_string(),
        query_params: HashMap::new(),
        headers: HeaderMap::new(),
        body: Vec::new(),
        path_params: HashMap::new(),
    };

    let response = theme_asset_handler.handle(list_request).await?;
    println!("✅ Theme list response status: {:?}", response.status);

    // Test theme CSS request
    let css_request = HttpRequest {
        method: Method::GET,
        path: "/themes/catppuccin-mocha/css".to_string(),
        query_params: HashMap::new(),
        headers: HeaderMap::new(),
        body: Vec::new(),
        path_params: HashMap::new(),
    };

    let css_response = theme_asset_handler.handle(css_request).await?;
    println!("✅ Theme CSS response status: {:?}", css_response.status);
    println!("📄 CSS content length: {} bytes", css_response.body.len());

    // Test theme metadata request
    let metadata_request = HttpRequest {
        method: Method::GET,
        path: "/themes/catppuccin-mocha/metadata".to_string(),
        query_params: HashMap::new(),
        headers: HeaderMap::new(),
        body: Vec::new(),
        path_params: HashMap::new(),
    };

    let metadata_response = theme_asset_handler.handle(metadata_request).await?;
    println!(
        "✅ Theme metadata response status: {:?}",
        metadata_response.status
    );

    // Test theme API handler
    println!("\n🔧 Testing theme API handler...");
    let theme_api_handler = ThemeApiHandler::new("/api/theme".to_string(), event_bus.clone());

    // Test theme switch request
    let switch_request = HttpRequest {
        method: Method::POST,
        path: "/api/theme".to_string(),
        query_params: HashMap::new(),
        headers: HeaderMap::new(),
        body: r#"{"theme": "catppuccin-latte"}"#.as_bytes().to_vec(),
        path_params: HashMap::new(),
    };

    let switch_response = theme_api_handler.handle(switch_request).await?;
    println!(
        "✅ Theme switch response status: {:?}",
        switch_response.status
    );

    if let Ok(response_text) = String::from_utf8(switch_response.body.clone()) {
        println!("📝 Switch response: {}", response_text);
    }

    // Test theme info handler
    println!("\n📊 Testing theme info handler...");
    let theme_info_handler = ThemeInfoHandler::new("/api/theme".to_string(), event_bus.clone());

    let info_request = HttpRequest {
        method: Method::GET,
        path: "/api/theme".to_string(),
        query_params: HashMap::new(),
        headers: HeaderMap::new(),
        body: Vec::new(),
        path_params: HashMap::new(),
    };

    let info_response = theme_info_handler.handle(info_request).await?;
    println!("✅ Theme info response status: {:?}", info_response.status);

    if let Ok(response_text) = String::from_utf8(info_response.body.clone()) {
        println!("📝 Info response: {}", response_text);
    }

    println!("\n🎉 Theme server handlers test completed successfully!");

    Ok(())
}
