use std::sync::Arc;

use crate::{
    LlamaCppProvider, LlmProvider, OpenAiCompatibleProvider, OpenRouterExtras, OpenRouterProvider,
    ProviderCapabilities, resolve_secret,
};

pub fn build_provider_from_config(
    c: &shared::ProviderConfig,
) -> anyhow::Result<Arc<dyn LlmProvider>> {
    let caps = ProviderCapabilities {
        supports_streaming: c.supports_streaming,
        supports_json_mode: c.supports_json_mode,
        max_context_tokens: c.max_context_tokens,
        request_timeout_seconds: c.request_timeout_seconds,
        stream_idle_timeout_seconds: c.stream_idle_timeout_seconds,
        max_retries: c.max_retries,
        ..ProviderCapabilities::default()
    };
    let api_key =
        resolve_secret(c.api_key.as_deref()).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    match c.provider_type.as_str() {
        "" | "openai_compatible" => Ok(Arc::new(
            OpenAiCompatibleProvider::new(
                c.name.clone(),
                c.base_url.clone(),
                api_key,
                c.model.clone(),
                caps,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        )),
        "llama_cpp" => Ok(Arc::new(
            LlamaCppProvider::new(
                c.name.clone(),
                c.base_url.clone(),
                api_key,
                c.model.clone(),
                caps,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        )),
        "openrouter" => {
            let extras = OpenRouterExtras {
                http_referer: c.http_referer.clone(),
                x_title: c.x_title.clone(),
                provider_routing: c.provider_routing.clone(),
                include_usage: c.include_usage,
            };
            Ok(Arc::new(
                OpenRouterProvider::new(c.base_url.clone(), api_key, c.model.clone(), caps, extras)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?,
            ))
        }
        other => anyhow::bail!("unknown provider_type '{other}'"),
    }
}
