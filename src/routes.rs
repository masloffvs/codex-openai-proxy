use bytes::Bytes;
use log::{debug, error, info, warn};
use serde_json::{json, Value};
use warp::{Filter, Reply};

use crate::proxy::ProxyServer;
use crate::types::{ChatCompletionsRequest, ChatCompletionsResponse};

type RouteResponse = Result<warp::reply::Response, warp::Rejection>;

pub fn create_routes(
    proxy: ProxyServer,
) -> impl Filter<Extract = impl Reply, Error = warp::Rejection> + Clone {
    let proxy_filter = warp::any().map(move || proxy.clone());

    warp::any()
        .and(warp::method())
        .and(warp::path::full())
        .and(warp::header::headers_cloned())
        .and(warp::body::bytes())
        .and(proxy_filter)
        .and_then(universal_request_handler)
        .with(build_cors())
}

fn build_cors() -> warp::cors::Builder {
    warp::cors()
        .allow_any_origin()
        .allow_headers(vec![
            "authorization",
            "content-type",
            "accept",
            "accept-encoding",
            "x-stainless-arch",
            "x-stainless-lang",
            "x-stainless-os",
            "x-stainless-package-version",
            "x-stainless-retry-count",
            "x-stainless-runtime",
            "x-stainless-runtime-version",
            "x-stainless-timeout",
        ])
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
}

async fn universal_request_handler(
    method: warp::http::Method,
    path: warp::path::FullPath,
    headers: warp::http::HeaderMap,
    body: Bytes,
    proxy: ProxyServer,
) -> RouteResponse {
    let path_str = path.as_str();

    log_request(&method, path_str, &headers, body.len());

    match (method.as_str(), path_str) {
        ("GET", "/health") => Ok(warp::reply::json(&json!({
            "status": "ok",
            "service": "codex-openai-proxy"
        }))
        .into_response()),
        ("GET", "/models") | ("GET", "/v1/models") => handle_models_request(path_str, proxy).await,
        ("POST", "/chat/completions") | ("POST", "/v1/chat/completions") => {
            handle_chat_request(path_str, &headers, body, proxy).await
        }
        _ => {
            warn!(
                target: "http",
                "request.unmatched method={} path={} status=404",
                method,
                path_str
            );
            Ok(
                warp::reply::with_status("Not found", warp::http::StatusCode::NOT_FOUND)
                    .into_response(),
            )
        }
    }
}

async fn handle_models_request(path_str: &str, proxy: ProxyServer) -> RouteResponse {
    info!(
        target: "models",
        "request.matched route=models method=GET path={}",
        path_str
    );

    match proxy.proxy_models().await {
        Ok(response) => {
            info!(
                target: "models",
                "response.completed path={} status=200 models={}",
                path_str,
                response.data.len()
            );
            Ok(warp::reply::json(&response).into_response())
        }
        Err(error) => {
            error!(
                target: "models",
                "response.failed path={} status=502 error={:#}",
                path_str,
                error
            );
            Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "error": {
                        "message": format!("Upstream models error: {}", error),
                        "type": "upstream_error",
                        "code": "bad_gateway"
                    }
                })),
                warp::http::StatusCode::BAD_GATEWAY,
            )
            .into_response())
        }
    }
}

async fn handle_chat_request(
    path_str: &str,
    headers: &warp::http::HeaderMap,
    body: Bytes,
    proxy: ProxyServer,
) -> RouteResponse {
    info!(
        target: "chat",
        "request.matched route=chat_completions method=POST path={} body_bytes={}",
        path_str,
        body.len()
    );
    log_curl_debug(path_str, headers, &body);

    let chat_req: ChatCompletionsRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => {
            warn!(
                target: "chat",
                "request.invalid_json path={} error={}",
                path_str,
                error
            );
            return Ok(warp::reply::with_status(
                "Invalid JSON",
                warp::http::StatusCode::BAD_REQUEST,
            )
            .into_response());
        }
    };

    let model = chat_req.model.clone();
    let message_count = chat_req.messages.len();
    let stream = chat_req.stream.unwrap_or(false);

    info!(
        target: "chat",
        "request.parsed path={} model={} messages={} stream={}",
        path_str,
        model,
        message_count,
        stream
    );

    log_message_summary(&chat_req);

    match proxy.proxy_request(chat_req).await {
        Ok(response) => {
            if stream {
                let content_preview = response
                    .choices
                    .first()
                    .map(|choice| truncate_for_log(&choice.message.content, 100))
                    .unwrap_or_default();

                info!(
                    target: "chat",
                    "response.streaming path={} model={} status=200",
                    path_str,
                    model
                );
                debug!(
                    target: "chat",
                    "response.streaming.preview value={:?}",
                    content_preview
                );

                Ok(build_streaming_response(&response))
            } else {
                info!(
                    target: "chat",
                    "response.completed path={} model={} status=200",
                    path_str,
                    model
                );
                Ok(warp::reply::json(&response).into_response())
            }
        }
        Err(error) => {
            error!(
                target: "chat",
                "response.failed path={} model={} status=500 error={:#}",
                path_str,
                model,
                error
            );
            Ok(warp::reply::json(&json!({
                "error": {
                    "message": format!("Proxy error: {}", error),
                    "type": "proxy_error",
                    "code": "internal_error"
                }
            }))
            .into_response())
        }
    }
}

fn log_curl_debug(path_str: &str, headers: &warp::http::HeaderMap, body: &Bytes) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    debug!(
        target: "http.replay",
        "request.replay method=POST path={} body_bytes={}",
        path_str,
        body.len()
    );

    for (name, value) in headers {
        debug!(
            target: "http.replay",
            "request.replay.header name={} value={:?}",
            name,
            replay_header_value(name.as_str(), value)
        );
    }

    if let Ok(body_str) = std::str::from_utf8(body) {
        debug!(
            target: "http.replay",
            "request.replay.body_preview value={:?}",
            truncate_for_log(body_str, 1000)
        );
        debug!(
            target: "http.replay",
            "request.replay.curl command={:?}",
            build_curl_command(path_str, headers, body_str)
        );
    }
}

fn log_message_summary(chat_req: &ChatCompletionsRequest) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    debug!(
        target: "chat",
        "request.summary model={} messages={} tools={} stream={}",
        chat_req.model,
        chat_req.messages.len(),
        chat_req.tools.as_ref().map_or(0, Vec::len),
        chat_req.stream.unwrap_or(false)
    );

    for (index, message) in chat_req.messages.iter().enumerate() {
        debug!(
            target: "chat",
            "request.message index={} role={} content_preview={:?}",
            index,
            message.role,
            preview_content(&message.content)
        );
    }
}

fn preview_content(content: &Value) -> String {
    match content {
        Value::String(text) => truncate_for_log(text, 80),
        Value::Array(items) => format!("[array items={}]", items.len()),
        _ => format!("[{}]", truncate_for_log(&content.to_string(), 80)),
    }
}

fn build_streaming_response(response: &ChatCompletionsResponse) -> warp::reply::Response {
    let message = response
        .choices
        .first()
        .map(|choice| choice.message.content.as_str())
        .unwrap_or("");
    let finish_reason = response
        .choices
        .first()
        .and_then(|choice| choice.finish_reason.as_deref())
        .unwrap_or("stop");

    let sse_chunks = [
        format!(
            "data: {}\n\n",
            json!({
                "id": response.id,
                "object": "chat.completion.chunk",
                "created": response.created,
                "model": response.model,
                "choices": [{
                    "index": 0,
                    "delta": { "role": "assistant" },
                    "finish_reason": null
                }]
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "id": response.id,
                "object": "chat.completion.chunk",
                "created": response.created,
                "model": response.model,
                "choices": [{
                    "index": 0,
                    "delta": { "content": message },
                    "finish_reason": null
                }]
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "id": response.id,
                "object": "chat.completion.chunk",
                "created": response.created,
                "model": response.model,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason
                }]
            })
        ),
        "data: [DONE]\n\n".to_string(),
    ]
    .join("");

    let reply = warp::reply::with_header(sse_chunks, "content-type", "text/event-stream");
    let reply = warp::reply::with_header(reply, "cache-control", "no-cache");
    let reply = warp::reply::with_header(reply, "connection", "keep-alive");
    reply.into_response()
}

fn log_request(
    method: &warp::http::Method,
    path: &str,
    headers: &warp::http::HeaderMap,
    body_len: usize,
) {
    info!(
        target: "http",
        "request.received method={} path={} headers={} body_bytes={} client={}",
        method,
        path,
        headers.len(),
        body_len,
        detect_client(headers)
    );

    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    if let Some(user_agent) = header_value(headers, "user-agent") {
        debug!(
            target: "http",
            "request.user_agent method={} path={} value={:?}",
            method,
            path,
            user_agent
        );
    }

    for (name, value) in headers {
        debug!(
            target: "http",
            "request.header method={} path={} name={} value={:?}",
            method,
            path,
            name,
            sanitize_header_value(name.as_str(), value)
        );
    }
}

fn detect_client(headers: &warp::http::HeaderMap) -> &'static str {
    let user_agent = header_value(headers, "user-agent")
        .unwrap_or_default()
        .to_ascii_lowercase();

    if user_agent.contains("cline") {
        "cline"
    } else if user_agent.contains("vscode") {
        "vscode"
    } else if user_agent.contains("mozilla") || user_agent.contains("chrome") {
        "browser"
    } else if user_agent.is_empty() {
        "unknown"
    } else {
        "custom"
    }
}

fn header_value<'a>(headers: &'a warp::http::HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn sanitize_header_value(name: &str, value: &warp::http::HeaderValue) -> String {
    match name.to_ascii_lowercase().as_str() {
        "authorization" | "cookie" | "set-cookie" => "[redacted]".to_string(),
        _ => value
            .to_str()
            .map(|value| truncate_for_log(value, 256))
            .unwrap_or_else(|_| "[invalid utf-8]".to_string()),
    }
}

fn replay_header_value(name: &str, value: &warp::http::HeaderValue) -> String {
    if should_skip_replay_header(name) {
        "[skipped]".to_string()
    } else if name.eq_ignore_ascii_case("authorization") {
        "test-key".to_string()
    } else {
        value
            .to_str()
            .map(|value| truncate_for_log(value, 256))
            .unwrap_or_else(|_| "[invalid utf-8]".to_string())
    }
}

fn should_skip_replay_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("host") || name.to_ascii_lowercase().starts_with("x-forwarded")
}

fn build_curl_command(path_str: &str, headers: &warp::http::HeaderMap, body: &str) -> String {
    let mut command = format!("curl -X POST http://localhost:8080{}", path_str);

    for (name, value) in headers {
        if should_skip_replay_header(name.as_str()) {
            continue;
        }

        let header_value = replay_header_value(name.as_str(), value);
        command.push_str(&format!(
            " -H '{}'",
            shell_escape_single_quoted(&format!("{}: {}", name, header_value))
        ));
    }

    command.push_str(&format!(
        " -d '{}'",
        shell_escape_single_quoted(&truncate_for_log(body, 500))
    ));

    command
}

fn shell_escape_single_quoted(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();

    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}
