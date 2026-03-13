use anyhow::{bail, Context, Result};
use log::{debug, error, info, warn};
use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

use crate::types::{
    AuthData, ChatCompletionsRequest, ChatCompletionsResponse, ChatResponseMessage, Choice,
    ContentItem, ModelInfo, ModelsListResponse, ResponseItem, ResponsesApiRequest, Usage,
};

const CODEX_CLIENT_VERSION: &str = "1.0.0";

#[derive(Clone)]
pub struct ProxyServer {
    client: Client,
    auth_data: AuthData,
}

impl ProxyServer {
    pub async fn new(auth_path: &str) -> Result<Self> {
        let auth_path = resolve_auth_path(auth_path)?;
        let auth_content = tokio::fs::read_to_string(&auth_path)
            .await
            .context("Failed to read auth.json")?;

        let auth_data: AuthData =
            serde_json::from_str(&auth_content).context("Failed to parse auth.json")?;

        let client = Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self { client, auth_data })
    }

    fn convert_chat_to_responses(&self, chat_req: ChatCompletionsRequest) -> ResponsesApiRequest {
        let mut input = Vec::new();
        let mut system_parts: Vec<String> = Vec::new();

        for msg in chat_req.messages {
            let content_text = match &msg.content {
                Value::String(text) => text.clone(),
                Value::Array(items) => items
                    .iter()
                    .filter_map(|item| {
                        if let Some(object) = item.as_object() {
                            object
                                .get("text")
                                .and_then(|text| text.as_str())
                                .map(ToOwned::to_owned)
                        } else {
                            item.as_str().map(ToOwned::to_owned)
                        }
                    })
                    .collect::<Vec<String>>()
                    .join(" "),
                _ => msg.content.to_string(),
            };

            // System messages go into `instructions`, not `input`
            if msg.role == "system" || msg.role == "developer" {
                if !content_text.is_empty() {
                    system_parts.push(content_text);
                }
                continue;
            }

            let content_item = if msg.role == "assistant" {
                ContentItem::OutputText { text: content_text }
            } else {
                ContentItem::InputText { text: content_text }
            };

            input.push(ResponseItem::Message {
                id: None,
                role: msg.role,
                content: vec![content_item],
            });
        }

        let instructions = if system_parts.is_empty() {
            "You are a helpful AI assistant.".to_string()
        } else {
            system_parts.join("\n\n")
        };

        info!(
            target: "proxy",
            "convert.instructions length={} input_messages={}",
            instructions.len(),
            input.len()
        );

        // Convert tools from Chat Completions format to Responses API format
        // Chat: {type: "function", function: {name, description, parameters}}
        // Responses: {type: "function", name, description, parameters}
        let tools: Vec<Value> = chat_req
            .tools
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tool| {
                let obj = tool.as_object()?;
                let tool_type = obj.get("type")?.as_str()?.to_string();
                if tool_type == "function" {
                    let func = obj.get("function")?.as_object()?;
                    let mut converted = serde_json::Map::new();
                    converted.insert("type".to_string(), Value::String(tool_type));
                    if let Some(name) = func.get("name") {
                        converted.insert("name".to_string(), name.clone());
                    }
                    if let Some(desc) = func.get("description") {
                        converted.insert("description".to_string(), desc.clone());
                    }
                    if let Some(params) = func.get("parameters") {
                        converted.insert("parameters".to_string(), params.clone());
                    }
                    if let Some(strict) = func.get("strict") {
                        converted.insert("strict".to_string(), strict.clone());
                    }
                    Some(Value::Object(converted))
                } else {
                    Some(tool)
                }
            })
            .collect();

        info!(
            target: "proxy",
            "convert.tools count={}",
            tools.len()
        );

        ResponsesApiRequest {
            model: chat_req.model,
            instructions,
            input,
            tools,
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            reasoning: None,
            store: false,
            stream: true,
            include: vec![],
        }
    }

    pub async fn proxy_request(
        &self,
        chat_req: ChatCompletionsRequest,
    ) -> Result<ChatCompletionsResponse> {
        let model = chat_req.model.clone();
        let stream = chat_req.stream.unwrap_or(false);

        info!(
            target: "proxy",
            "chat.request model={} stream={} mode=live",
            model,
            stream
        );
        debug!(
            target: "proxy",
            "chat.request.tools count={}",
            chat_req.tools.as_ref().map_or(0, Vec::len)
        );

        self.proxy_request_live(chat_req).await
    }

    pub async fn proxy_models(&self) -> Result<ModelsListResponse> {
        info!(
            target: "proxy",
            "backend.models.request provider=chatgpt endpoint=/backend-api/codex/models client_version={}",
            CODEX_CLIENT_VERSION
        );

        let mut request_builder = self
            .client
            .get(format!(
                "https://chatgpt.com/backend-api/codex/models?client_version={}",
                CODEX_CLIENT_VERSION
            ))
            .header("Accept", "application/json")
            .header("Accept-Encoding", "identity")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Referer", "https://chatgpt.com/")
            .header("Origin", "https://chatgpt.com")
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-origin")
            .header("Cache-Control", "no-cache")
            .header("Pragma", "no-cache")
            .header("DNT", "1")
            .header("originator", "codex_cli_rs");

        if let Some(tokens) = &self.auth_data.tokens {
            request_builder =
                request_builder.header("Authorization", format!("Bearer {}", tokens.access_token));
            request_builder = request_builder.header("chatgpt-account-id", &tokens.account_id);
        } else if let Some(api_key) = &self.auth_data.api_key {
            request_builder =
                request_builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = request_builder
            .send()
            .await
            .context("Failed to request models from ChatGPT backend")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(
                target: "proxy",
                "backend.models.response_error status={} body={}",
                status,
                truncate_for_log(&body, 512)
            );

            bail!("Upstream models endpoint returned HTTP {}", status);
        }

        let body = response
            .text()
            .await
            .context("Failed to read models response from ChatGPT backend")?;

        let payload = serde_json::from_str::<Value>(&body).with_context(|| {
            format!(
                "Failed to parse models response from ChatGPT backend body={} ",
                truncate_for_log(&body, 512)
            )
        })?;

        let models = payload
            .get("models")
            .and_then(Value::as_array)
            .context("Upstream models response missing models array")?;

        let created = chrono::Utc::now().timestamp();
        let data = models
            .iter()
            .filter_map(|model| model.get("slug").and_then(Value::as_str))
            .map(|slug| ModelInfo {
                id: slug.to_string(),
                object: "model".to_string(),
                created,
                owned_by: "openai".to_string(),
            })
            .collect::<Vec<_>>();

        if data.is_empty() {
            bail!("Upstream models response did not contain any usable model slugs");
        }

        info!(
            target: "proxy",
            "backend.models.response_ok count={} client_version={}",
            data.len(),
            CODEX_CLIENT_VERSION
        );

        Ok(ModelsListResponse {
            object: "list".to_string(),
            data,
        })
    }

    async fn proxy_request_live(
        &self,
        chat_req: ChatCompletionsRequest,
    ) -> Result<ChatCompletionsResponse> {
        let responses_req = self.convert_chat_to_responses(chat_req);

        info!(
            target: "proxy",
            "backend.request provider=chatgpt endpoint=/backend-api/codex/responses model={}",
            responses_req.model
        );

        let mut request_builder = self
            .client
            .post("https://chatgpt.com/backend-api/codex/responses")
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("Accept-Encoding", "identity")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Referer", "https://chatgpt.com/")
            .header("Origin", "https://chatgpt.com")
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-origin")
            .header("Cache-Control", "no-cache")
            .header("Pragma", "no-cache")
            .header("DNT", "1")
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "codex_cli_rs");

        if let Some(tokens) = &self.auth_data.tokens {
            request_builder =
                request_builder.header("Authorization", format!("Bearer {}", tokens.access_token));
            request_builder = request_builder.header("chatgpt-account-id", &tokens.account_id);
        } else if let Some(api_key) = &self.auth_data.api_key {
            request_builder =
                request_builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let session_id = Uuid::new_v4();
        request_builder = request_builder.header("session_id", session_id.to_string());

        let response = request_builder
            .json(&responses_req)
            .send()
            .await
            .context("Failed to send request to ChatGPT backend")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(
                target: "proxy",
                "backend.response_error status={} body={}",
                status,
                truncate_for_log(&body, 512)
            );

            bail!("Upstream backend returned HTTP {}", status);
        }

        let mut response_content = String::new();
        let mut saw_output_text_delta = false;
        let response_text = response.text().await?;

        for line in response_text.lines() {
            if let Some(json_data) = line.strip_prefix("data: ") {
                if json_data == "[DONE]" {
                    break;
                }

                if let Ok(event) = serde_json::from_str::<serde_json::Value>(json_data) {
                    if let Some(event_type) = event.get("type").and_then(|value| value.as_str()) {
                        match event_type {
                            "response.output_text.delta" => {
                                if let Some(delta) =
                                    event.get("delta").and_then(|value| value.as_str())
                                {
                                    saw_output_text_delta = true;
                                    response_content.push_str(delta);
                                }
                            }
                            "response.output_item.done" => {
                                if saw_output_text_delta {
                                    continue;
                                }

                                if let Some(item) = event.get("item") {
                                    if let Some(content_items) =
                                        item.get("content").and_then(|value| value.as_array())
                                    {
                                        for content_item in content_items {
                                            if let Some(text) = content_item
                                                .get("text")
                                                .and_then(|value| value.as_str())
                                            {
                                                response_content.push_str(text);
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if response_content.is_empty() {
            warn!(
                target: "proxy",
                "backend.response_empty model={}",
                responses_req.model
            );
            response_content = "I apologize, but I couldn't process your request due to a backend API format issue. The proxy is receiving your request correctly but needs format refinement.".to_string();
        }

        info!(
            target: "proxy",
            "backend.response_ok model={} content_chars={}",
            responses_req.model,
            response_content.chars().count()
        );

        Ok(build_chat_response(
            responses_req.model.clone(),
            response_content,
            Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        ))
    }
}

fn resolve_auth_path(auth_path: &str) -> Result<String> {
    if auth_path.starts_with("~/") {
        let home = std::env::var("HOME").context("HOME environment variable not set")?;
        Ok(auth_path.replacen('~', &home, 1))
    } else {
        Ok(auth_path.to_string())
    }
}

fn build_chat_response(model: String, content: String, usage: Usage) -> ChatCompletionsResponse {
    ChatCompletionsResponse {
        id: format!("chatcmpl-{}", Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model,
        choices: vec![Choice {
            index: 0,
            message: ChatResponseMessage {
                role: "assistant".to_string(),
                content,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(usage),
    }
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
