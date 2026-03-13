use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Debug)]
pub struct ModelsListResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Serialize, Debug)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub stream: Option<bool>,
    pub tools: Option<Vec<Value>>,
    pub tool_choice: Option<Value>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}

#[derive(Serialize, Debug)]
pub struct ChatCompletionsResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Serialize, Debug)]
pub struct Choice {
    pub index: i32,
    pub message: ChatResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ChatResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Debug)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

#[derive(Serialize, Debug)]
pub struct ResponsesApiRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<ResponseItem>,
    pub tools: Vec<Value>,
    pub tool_choice: String,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<Value>,
    pub store: bool,
    pub stream: bool,
    pub include: Vec<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseItem {
    Message {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        role: String,
        content: Vec<ContentItem>,
    },
}

#[derive(Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentItem {
    InputText { text: String },
}

#[derive(Deserialize, Debug, Clone)]
pub struct AuthData {
    #[serde(rename = "OPENAI_API_KEY")]
    pub api_key: Option<String>,
    pub tokens: Option<TokenData>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct TokenData {
    pub access_token: String,
    pub account_id: String,
    pub refresh_token: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct ResponsesApiResponse {
    pub response: Option<ResponseOutput>,
    pub id: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct ResponseOutput {
    pub content: Option<Vec<ResponseContentItem>>,
    pub role: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct ResponseContentItem {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
}
