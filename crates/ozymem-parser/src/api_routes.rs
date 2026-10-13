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
    let prefix_re = Regex::new(
        r#"(?i)APIRouter\s*\([^)]*prefix\s*=\s*["']([^"']+)["']"#
    ).unwrap();
    let file_prefix = prefix_re.captures(source).and_then(|c| c.get(1)).map(|m| m.as_str().trim_end_matches('/').to_string()).unwrap_or_default();

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
            let subpath = dec_caps.get(2).map(|m| m.as_str()).unwrap_or("/");
            let path = if !file_prefix.is_empty() {
                if subpath.starts_with('/') {
                    format!("{}{}", file_prefix, subpath)
                } else {
                    format!("{}/{}", file_prefix, subpath)
                }
            } else {
                subpath.to_string()
            };
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
        SupportedLanguage::JavaScript | SupportedLanguage::TypeScript | SupportedLanguage::TypeScriptReact => parse_express_routes(source, file_path),
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

    #[test]
    fn test_parse_frontend_endpoint_consumers() {
        let tsx_code = r#"
        import React, { useEffect, useState } from 'react';
        import useSWR from 'swr';
        import { apiClient } from '@/lib/api';

        export function VerificacionForm() {
            const { data } = useSWR('/api/sig/sctr?status=active', fetcher);
            const [loading, setLoading] = useState(false);

            async function handleSubmit() {
                setLoading(true);
                const res = await fetch("/api/probetas", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ codigo: "P-101" })
                });
                const result = await apiClient.post("/api/clientes/clasificar", { id: 12 });
            }

            async function handleInspect(id: string) {
                const sample = await fetch(`/api/samples/${id}/verify`);
            }
        }
        "#;

        let consumers = parse_frontend_endpoint_consumers(tsx_code, "components/VerificacionForm.tsx");
        assert_eq!(consumers.len(), 4);

        // 1. useSWR
        assert_eq!(consumers[0].raw_endpoint, "/api/sig/sctr?status=active");
        assert_eq!(consumers[0].canonical_path, "/api/sig/sctr");
        assert_eq!(consumers[0].http_method, "GET");
        assert_eq!(consumers[0].caller_symbol.as_deref(), Some("VerificacionForm"));

        // 2. fetch POST
        assert_eq!(consumers[1].raw_endpoint, "/api/probetas");
        assert_eq!(consumers[1].canonical_path, "/api/probetas");
        assert_eq!(consumers[1].http_method, "POST");

        // 3. apiClient POST
        assert_eq!(consumers[2].raw_endpoint, "/api/clientes/clasificar");
        assert_eq!(consumers[2].canonical_path, "/api/clientes/clasificar");
        assert_eq!(consumers[2].http_method, "POST");

        // 4. dynamic template fetch
        assert_eq!(consumers[3].canonical_path, "/api/samples/:id/verify");
        assert_eq!(consumers[3].http_method, "GET");
    }
}

/// Represents an extracted frontend call to an API endpoint (e.g. Next.js fetch, axios, SWR, useQuery).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointConsumer {
    pub file_path: String,
    pub line_number: usize,
    pub raw_endpoint: String,
    pub canonical_path: String,
    pub http_method: String,
    pub caller_symbol: Option<String>,
}

/// Normalizes an endpoint path to its canonical form (strips queries, normalizes dynamic params).
pub fn canonicalize_endpoint_path(raw: &str) -> String {
    let clean = raw.trim();
    let path_only = clean.split('?').next().unwrap_or(clean);
    let path_only = path_only.split('#').next().unwrap_or(path_only);

    let relative_path = if let Some(pos) = path_only.find("/api/") {
        &path_only[pos..]
    } else {
        path_only
    };

    let re_template = Regex::new(r#"\$\{[^}]+\}"#).unwrap();
    let normalized = re_template.replace_all(relative_path, ":id");

    normalized.trim_end_matches('/').to_string()
}

/// Parses frontend TypeScript / Next.js / React source code for API endpoint consumption.
pub fn parse_frontend_endpoint_consumers(source: &str, file_path: &str) -> Vec<EndpointConsumer> {
    let mut consumers = Vec::new();

    let fetch_re = Regex::new(
        r#"(?i)\bfetch\s*\(\s*["'`]((?:/api/|https?://[^/'"`]+/api/)[^"'`\s]+)["'`]"#
    ).unwrap();

    let client_re = Regex::new(
        r#"(?i)\b(?:axios|apiClient|api|httpClient)\.(get|post|put|delete|patch)\s*\(\s*["'`]((?:/api/|https?://[^/'"`]+/api/)[^"'`\s]+)["'`]"#
    ).unwrap();

    let swr_re = Regex::new(
        r#"(?i)\buseSWR\s*(?:<[^>]+>)?\s*\(\s*["'`]((?:/api/|https?://[^/'"`]+/api/)[^"'`\s]+)["'`]"#
    ).unwrap();

    let method_re = Regex::new(r#"(?i)method\s*:\s*["'](GET|POST|PUT|DELETE|PATCH)["']"#).unwrap();
    let component_re = Regex::new(r#"(?i)(?:export\s+)?(?:default\s+)?(?:function|const)\s+([A-Za-z0-9_]+)"#).unwrap();

    let lines: Vec<&str> = source.lines().collect();
    let mut current_symbol = None;

    for (idx, line) in lines.iter().enumerate() {
        if let Some(caps) = component_re.captures(line) {
            if let Some(name) = caps.get(1) {
                current_symbol = Some(name.as_str().to_string());
            }
        }

        // 1. fetch
        for caps in fetch_re.captures_iter(line) {
            if let Some(raw_match) = caps.get(1) {
                let raw_endpoint = raw_match.as_str().to_string();
                let canonical_path = canonicalize_endpoint_path(&raw_endpoint);

                let mut http_method = "GET".to_string();
                let search_window = lines[idx..std::cmp::min(idx + 5, lines.len())].join(" ");
                if let Some(m_caps) = method_re.captures(&search_window) {
                    if let Some(m) = m_caps.get(1) {
                        http_method = m.as_str().to_uppercase();
                    }
                }

                consumers.push(EndpointConsumer {
                    file_path: file_path.to_string(),
                    line_number: idx + 1,
                    raw_endpoint,
                    canonical_path,
                    http_method,
                    caller_symbol: current_symbol.clone(),
                });
            }
        }

        // 2. axios / apiClient
        for caps in client_re.captures_iter(line) {
            let method = caps.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_else(|| "GET".into());
            if let Some(raw_match) = caps.get(2) {
                let raw_endpoint = raw_match.as_str().to_string();
                let canonical_path = canonicalize_endpoint_path(&raw_endpoint);

                consumers.push(EndpointConsumer {
                    file_path: file_path.to_string(),
                    line_number: idx + 1,
                    raw_endpoint,
                    canonical_path,
                    http_method: method,
                    caller_symbol: current_symbol.clone(),
                });
            }
        }

        // 3. useSWR
        for caps in swr_re.captures_iter(line) {
            if let Some(raw_match) = caps.get(1) {
                let raw_endpoint = raw_match.as_str().to_string();
                let canonical_path = canonicalize_endpoint_path(&raw_endpoint);

                consumers.push(EndpointConsumer {
                    file_path: file_path.to_string(),
                    line_number: idx + 1,
                    raw_endpoint,
                    canonical_path,
                    http_method: "GET".to_string(),
                    caller_symbol: current_symbol.clone(),
                });
            }
        }
    }

    consumers
}
