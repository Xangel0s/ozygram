use crate::SupportedLanguage;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Represents an extracted HTTP API endpoint route definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiRouteDefinition {
    pub method: String,
    pub path: String,
    pub handler_name: String,
    pub file_path: String,
    pub line_number: usize,
    pub dto_model: Option<String>,
    pub framework: String,
}

/// Parses source code for Python/FastAPI/Flask API routes.
pub fn parse_fastapi_routes(source: &str, file_path: &str) -> Vec<ApiRouteDefinition> {
    let mut routes = Vec::new();
    let decorator_re = Regex::new(
        r#"(?i)@(?:app|router|api_router)\.(get|post|put|delete|patch|options|head)\s*\(\s*["']([^"']+)["']"#
    ).unwrap();
    let func_re = Regex::new(
        r#"(?i)(?:async\s+)?(?:def|fn)\s+([a-zA-Z0-9_]+)\s*\(([^)]*)\)"#
    ).unwrap();
    let dto_re = Regex::new(
        r#"(?i):\s*([A-Z][a-zA-Z0-9_]+(?:Schema|DTO|Create|Update|Request|In|Model)?)"#
    ).unwrap();

    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(dec_caps) = decorator_re.captures(line) {
            let method = dec_caps.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_else(|| "GET".into());
            let path = dec_caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_else(|| "/".into());
            let line_number = i + 1;

            // Search next 5 lines for function definition
            let mut handler_name = String::from("handler");
            let mut dto_model = None;

            for j in (i + 1)..std::cmp::min(i + 6, lines.len()) {
                if let Some(fn_caps) = func_re.captures(lines[j]) {
                    if let Some(name) = fn_caps.get(1) {
                        handler_name = name.as_str().to_string();
                    }
                    if let Some(params) = fn_caps.get(2) {
                        let param_str = params.as_str();
                        for dto_cap in dto_re.captures_iter(param_str) {
                            if let Some(dto) = dto_cap.get(1) {
                                let dto_name = dto.as_str();
                                if dto_name != "Request" && dto_name != "Response" && dto_name != "Session" && dto_name != "Depends" {
                                    dto_model = Some(dto_name.to_string());
                                    break;
                                }
                            }
                        }
                    }
                    break;
                }
            }

            routes.push(ApiRouteDefinition {
                method,
                path,
                handler_name,
                file_path: file_path.to_string(),
                line_number,
                dto_model,
                framework: "FastAPI".into(),
            });
        }
        i += 1;
    }

    routes
}

/// Parses source code for Node/Express/Next.js API routes.
pub fn parse_express_routes(source: &str, file_path: &str) -> Vec<ApiRouteDefinition> {
    let mut routes = Vec::new();
    let route_re = Regex::new(
        r#"(?i)(?:app|router)\.(get|post|put|delete|patch|all)\s*\(\s*["']([^"']+)["'](?:\s*,\s*([a-zA-Z0-9_]+))?"#
    ).unwrap();

    for (idx, line) in source.lines().enumerate() {
        if let Some(caps) = route_re.captures(line) {
            let method = caps.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_else(|| "GET".into());
            let path = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_else(|| "/".into());
            let handler_name = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_else(|| "anonymousHandler".into());

            routes.push(ApiRouteDefinition {
                method,
                path,
                handler_name,
                file_path: file_path.to_string(),
                line_number: idx + 1,
                dto_model: None,
                framework: "Express".into(),
            });
        }
    }

    routes
}

/// Parses source code for Rust/Axum/Actix API routes.
pub fn parse_axum_routes(source: &str, file_path: &str) -> Vec<ApiRouteDefinition> {
    let mut routes = Vec::new();
    let axum_re = Regex::new(
        r#"(?i)\.route\s*\(\s*["']([^"']+)["']\s*,\s*(get|post|put|delete|patch)\s*\(\s*([a-zA-Z0-9_:]+)\s*\)\s*\)"#
    ).unwrap();

    for (idx, line) in source.lines().enumerate() {
        if let Some(caps) = axum_re.captures(line) {
            let path = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_else(|| "/".into());
            let method = caps.get(2).map(|m| m.as_str().to_uppercase()).unwrap_or_else(|| "GET".into());
            let handler_name = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_else(|| "handler".into());

            routes.push(ApiRouteDefinition {
                method,
                path,
                handler_name,
                file_path: file_path.to_string(),
                line_number: idx + 1,
                dto_model: None,
                framework: "Axum".into(),
            });
        }
    }

    routes
}

/// Dispatches route parsing based on detected language or content.
pub fn parse_api_routes(source: &str, file_path: &str, lang: SupportedLanguage) -> Vec<ApiRouteDefinition> {
    match lang {
        SupportedLanguage::Python => parse_fastapi_routes(source, file_path),
        SupportedLanguage::JavaScript | SupportedLanguage::TypeScriptReact => parse_express_routes(source, file_path),
        SupportedLanguage::Rust => parse_axum_routes(source, file_path),
        _ => {
            // Fallback heuristics: try fastapi then express
            let mut res = parse_fastapi_routes(source, file_path);
            if res.is_empty() {
                res = parse_express_routes(source, file_path);
            }
            res
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_fastapi_routes() {
        let code = r#"
        from fastapi import APIRouter
        router = APIRouter()

        @router.get("/api/v1/users")
        async fn list_users():
            return []

        @router.post("/api/v1/users/create")
        async fn create_user(payload: UserCreateDTO, db: Session):
            return payload
        "#;

        let routes = parse_fastapi_routes(code, "routers/users.py");
        assert_eq!(routes.len(), 2);

        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/api/v1/users");
        assert_eq!(routes[0].handler_name, "list_users");
        assert_eq!(routes[0].dto_model, None);

        assert_eq!(routes[1].method, "POST");
        assert_eq!(routes[1].path, "/api/v1/users/create");
        assert_eq!(routes[1].handler_name, "create_user");
        assert_eq!(routes[1].dto_model, Some("UserCreateDTO".into()));
    }

    #[test]
    fn test_parse_express_routes() {
        let code = r#"
        const express = require('express');
        const router = express.Router();

        router.get('/api/samples', getSamples);
        router.post('/api/samples/:id/verify', verifySample);
        "#;

        let routes = parse_express_routes(code, "routes/samples.js");
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/api/samples");
        assert_eq!(routes[0].handler_name, "getSamples");

        assert_eq!(routes[1].method, "POST");
        assert_eq!(routes[1].path, "/api/samples/:id/verify");
        assert_eq!(routes[1].handler_name, "verifySample");
    }

    #[test]
    fn test_parse_axum_routes() {
        let code = r#"
        let app = Router::new()
            .route("/health", get(health_check))
            .route("/api/v1/data", post(create_data));
        "#;

        let routes = parse_axum_routes(code, "src/main.rs");
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/health");
        assert_eq!(routes[0].handler_name, "health_check");

        assert_eq!(routes[1].method, "POST");
        assert_eq!(routes[1].path, "/api/v1/data");
        assert_eq!(routes[1].handler_name, "create_data");
    }
}
