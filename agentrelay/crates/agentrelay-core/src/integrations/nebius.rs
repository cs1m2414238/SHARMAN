//! Nebius Token Factory adapter for grounded Nemotron context compression.

use std::{env, time::Duration};

use serde::{Deserialize, Serialize};

use crate::ports::{
    CompressedBullet, CompressionError, CompressionRequest, CompressionResponse, ContextCompressor,
};

const DEFAULT_BASE_URL: &str = "https://api.tokenfactory.nebius.com/v1";

pub struct NebiusNemotron {
    api_key: String,
    model: String,
    endpoint: String,
    agent: ureq::Agent,
}

impl NebiusNemotron {
    /// Compression is opt-in: both variables must be present, and the selected model must be an
    /// NVIDIA Nemotron model. Credentials are retained only in memory and are never persisted.
    pub fn from_env() -> Result<Option<Self>, String> {
        let api_key = env::var("NEBIUS_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let model = env::var("AGENTRELAY_NEMOTRON_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        match (api_key, model) {
            (None, None) => Ok(None),
            (None, Some(_)) => {
                Err("AGENTRELAY_NEMOTRON_MODEL is set, but NEBIUS_API_KEY is missing".into())
            }
            (Some(_), None) => {
                Err("NEBIUS_API_KEY is set, but AGENTRELAY_NEMOTRON_MODEL is missing".into())
            }
            (Some(api_key), Some(model)) => {
                let normalized_model = model.to_ascii_lowercase();
                if !normalized_model.starts_with("nvidia/")
                    || !normalized_model.contains("nemotron")
                {
                    return Err("AGENTRELAY_NEMOTRON_MODEL must be an NVIDIA Nemotron model ID (nvidia/...Nemotron...)".into());
                }
                let base_url = env::var("NEBIUS_BASE_URL")
                    .unwrap_or_else(|_| DEFAULT_BASE_URL.into())
                    .trim_end_matches('/')
                    .to_owned();
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(10))
                    .timeout_read(Duration::from_secs(30))
                    .timeout_write(Duration::from_secs(15))
                    .build();
                Ok(Some(Self {
                    api_key,
                    model,
                    endpoint: format!("{base_url}/chat/completions"),
                    agent,
                }))
            }
        }
    }
}

impl ContextCompressor for NebiusNemotron {
    fn model(&self) -> &str {
        &self.model
    }

    fn compress(
        &self,
        request: &CompressionRequest,
    ) -> Result<CompressionResponse, CompressionError> {
        let facts = serde_json::to_string(&request.facts)
            .map_err(|error| CompressionError::InvalidResponse(error.to_string()))?;
        let user_prompt = format!(
            "Project: {}\nContext profile: {}\nGrounded facts JSON:\n{}",
            request.project_name, request.profile, facts
        );
        let mut body = ChatRequest {
            model: &self.model,
            temperature: 0.1,
            max_tokens: request.max_output_tokens.clamp(128, 2_048),
            response_format: Some(ResponseFormat { kind: "json_object" }),
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: "Compress only the supplied grounded facts. Return a JSON object with a bullets array. Each bullet must contain text and one or more sourceKeys copied exactly from the facts. Never add facts, source keys, advice, or conclusions that are absent from the input.",
                },
                ChatMessage {
                    role: "user",
                    content: &user_prompt,
                },
            ],
        };
        let first_attempt = self
            .agent
            .post(&self.endpoint)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(&body);
        let response = match first_attempt {
            Ok(response) => response,
            // JSON mode is model-specific. Retry with prompt-enforced JSON for Nemotron models
            // without that capability; the local source validator still remains authoritative.
            Err(ureq::Error::Status(400, _)) => {
                body.response_format = None;
                self.agent
                    .post(&self.endpoint)
                    .set("Authorization", &format!("Bearer {}", self.api_key))
                    .set("Content-Type", "application/json")
                    .send_json(&body)
                    .map_err(|error| CompressionError::Request(safe_ureq_error(&error)))?
            }
            Err(error) => return Err(CompressionError::Request(safe_ureq_error(&error))),
        };
        let response: ChatResponse = response
            .into_json()
            .map_err(|error| CompressionError::InvalidResponse(error.to_string()))?;
        let content = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .ok_or_else(|| CompressionError::InvalidResponse("no completion choice".into()))?;
        let payload: SummaryPayload = serde_json::from_str(strip_code_fence(&content))
            .map_err(|error| CompressionError::InvalidResponse(error.to_string()))?;
        Ok(CompressionResponse {
            model: self.model.clone(),
            bullets: payload.bullets,
        })
    }
}

fn safe_ureq_error(error: &ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, _) => format!("Token Factory returned HTTP {code}"),
        ureq::Error::Transport(transport) => transport.to_string(),
    }
}

fn strip_code_fence(value: &str) -> &str {
    value
        .trim()
        .strip_prefix("```json")
        .or_else(|| value.trim().strip_prefix("```"))
        .and_then(|inner| inner.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or_else(|| value.trim())
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    temperature: f32,
    max_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat<'a>>,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Serialize)]
struct ResponseFormat<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryPayload {
    bullets: Vec<CompressedBullet>,
}
