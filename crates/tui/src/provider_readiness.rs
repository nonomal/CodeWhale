//! Provider readiness row-model (foundation for the `/provider` dashboard, #3083).
//!
//! This module is a **pure data foundation**: it assembles, for each
//! [`ApiProvider`], a [`ProviderReadinessRow`] describing what the picker
//! *would* render — readiness state, the resolved model id and its provenance,
//! and the static capability/metadata (context window, max output, thinking,
//! tools/cache/streaming flags) already computed by
//! [`config::provider_capability`].
//!
//! It deliberately does **not** render anything and does **not** perform any
//! network I/O. Live reachability probes are out of scope here; the
//! [`ProviderReadiness::Unreachable`] / [`ProviderReadiness::AuthFailed`]
//! variants exist so a later caller can layer cached health on top without
//! changing this row shape. Keeping assembly separate from rendering is exactly
//! the "row model is testable without rendering the modal" acceptance criterion
//! from the issue.
//!
//! The picker (`crate::tui::provider_picker`) is intentionally left untouched
//! for now; this is the substrate it can consume later.

// Foundation-first (#3083): this row-model is built and tested ahead of its
// renderer. Until the `/provider` picker is rewired to consume it, the public
// surface has no non-test caller, so silence dead-code for this module only —
// matching the established crate idiom (e.g. `features.rs`). Remove when the
// picker consumes `provider_readiness_rows`.
#![allow(dead_code)]

use crate::config::{
    ApiProvider, Config, ProviderCapability, RequestPayloadMode, has_api_key_for,
    kimi_cli_credentials_present, model_completion_names_for_provider, provider_capability,
};

/// High-level readiness of a provider, independent of live network state.
///
/// Today only the credential-derived variants ([`Configured`],[`NeedsKey`],
/// [`OauthReady`],[`OptionalKeyLocal`]) are produced by
/// [`ProviderReadinessRow::for_provider`]. [`Unreachable`]/[`AuthFailed`]/
/// [`Unknown`] are reserved for a future cached-health layer so consumers can
/// match exhaustively without a breaking change.
///
/// [`Configured`]: ProviderReadiness::Configured
/// [`NeedsKey`]: ProviderReadiness::NeedsKey
/// [`OauthReady`]: ProviderReadiness::OauthReady
/// [`OptionalKeyLocal`]: ProviderReadiness::OptionalKeyLocal
/// [`Unreachable`]: ProviderReadiness::Unreachable
/// [`AuthFailed`]: ProviderReadiness::AuthFailed
/// [`Unknown`]: ProviderReadiness::Unknown
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderReadiness {
    /// A usable credential is present (env var, config key, or keyring).
    Configured,
    /// The provider needs a key and none was found.
    NeedsKey,
    /// Credentials come from a CLI OAuth login on disk (Codex / Kimi).
    OauthReady,
    /// Self-hosted / local provider that works without a key (key optional).
    OptionalKeyLocal,
    /// Reserved: a cached health probe found the endpoint unreachable.
    Unreachable,
    /// Reserved: a cached probe saw the credential rejected (401/403).
    AuthFailed,
    /// Reserved: readiness has not been determined.
    Unknown,
}

impl ProviderReadiness {
    /// A short, stable label suitable for a status chip.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::NeedsKey => "needs key",
            Self::OauthReady => "OAuth ready",
            Self::OptionalKeyLocal => "optional key (local)",
            Self::Unreachable => "unreachable",
            Self::AuthFailed => "auth failed",
            Self::Unknown => "unknown",
        }
    }

    /// Whether a turn could be dispatched to this provider right now based on
    /// credentials alone (ignoring live reachability).
    #[must_use]
    pub fn is_usable(self) -> bool {
        matches!(
            self,
            Self::Configured | Self::OauthReady | Self::OptionalKeyLocal
        )
    }
}

/// Where the resolved model id came from.
///
/// The foundation distinguishes a provider-scoped saved override
/// ([`Saved`]) from the catalog default ([`Default`]). [`Custom`] and
/// [`Catalog`] are reserved for when the live model catalog (#3072) and
/// pass-through custom ids are wired in.
///
/// [`Saved`]: ModelProvenance::Saved
/// [`Default`]: ModelProvenance::Default
/// [`Custom`]: ModelProvenance::Custom
/// [`Catalog`]: ModelProvenance::Catalog
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProvenance {
    /// Resolved from the provider's built-in default catalog list.
    Default,
    /// Resolved from a user-saved provider-scoped model override.
    Saved,
    /// A custom pass-through model id (reserved; needs catalog wiring).
    Custom,
    /// Selected from a live-hydrated provider catalog (reserved; #3072).
    Catalog,
    /// Provenance could not be determined / no model is known.
    Unknown,
}

impl ModelProvenance {
    /// A short, stable label suitable for a badge.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Saved => "saved",
            Self::Custom => "custom",
            Self::Catalog => "catalog",
            Self::Unknown => "unknown",
        }
    }
}

/// A single row in the provider readiness dashboard.
///
/// This is the row-model the `/provider` picker can render later. It is built
/// entirely from static config + capability data, so it is cheap and
/// deterministic to construct in a test.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProviderReadinessRow {
    /// Canonical provider identifier.
    pub provider: ApiProvider,
    /// Human-friendly provider label.
    pub display_name: &'static str,
    /// Whether this provider is the active one in the supplied config.
    pub is_active: bool,
    /// Whether a usable credential was found (env/config/keyring/oauth/local).
    pub has_key: bool,
    /// Credential-derived readiness (no live network probe).
    pub readiness: ProviderReadiness,
    /// The resolved model id that would be sent to this provider, if known.
    pub resolved_model: Option<String>,
    /// Where [`resolved_model`](Self::resolved_model) came from.
    pub model_provenance: ModelProvenance,
    /// Base URL hint when the provider config carries an explicit endpoint.
    ///
    /// Useful for self-hosted / custom-endpoint providers; `None` when the
    /// built-in default endpoint is in effect.
    pub base_url: Option<String>,
    /// Context window in tokens for the resolved model, when a model is known.
    pub context_window: Option<u32>,
    /// Official max output tokens for the resolved model, when known.
    pub max_output: Option<u32>,
    /// Whether the resolved provider+model supports thinking/reasoning.
    pub thinking_supported: bool,
    /// Whether the provider returns prompt-cache telemetry fields.
    pub cache_telemetry_supported: bool,
    /// Which request-payload dialect the provider speaks.
    pub request_payload_mode: RequestPayloadMode,
    /// Whether the provider streams responses. All currently-shipped providers
    /// stream, so this is `true` for every known provider; the field exists so
    /// the picker can render a "streaming" badge from the row model rather than
    /// hard-coding it.
    pub streaming_supported: bool,
    /// Whether this is a self-hosted / local-first provider (key optional).
    pub local: bool,
}

impl ProviderReadinessRow {
    /// Build the readiness row for a single provider against `config`.
    ///
    /// `config` supplies the active provider, the saved/default model, any
    /// provider-scoped base URL, and the credential state. No network access
    /// is performed.
    #[must_use]
    pub fn for_provider(config: &Config, provider: ApiProvider) -> Self {
        let active_provider = config.api_provider();
        let is_active = provider == active_provider;
        let has_key = has_api_key_for(config, provider);
        let local = is_local_provider(provider);

        let (resolved_model, model_provenance) = resolve_model(config, provider, is_active);

        // Capability/metadata is only meaningful when we know which model id
        // will be sent. When no model is known we leave the metadata `None`
        // (explicitly "unknown") rather than fabricating a default.
        let capability: Option<ProviderCapability> = resolved_model
            .as_deref()
            .map(|model| provider_capability(provider, model));

        let readiness = classify_readiness(provider, has_key, local);

        let base_url = saved_base_url(config, provider);

        let (context_window, max_output, thinking_supported, cache_telemetry_supported, mode) =
            match capability {
                Some(cap) => (
                    Some(cap.context_window),
                    Some(cap.max_output),
                    cap.thinking_supported,
                    cap.cache_telemetry_supported,
                    cap.request_payload_mode,
                ),
                None => (
                    None,
                    None,
                    false,
                    false,
                    RequestPayloadMode::ChatCompletions,
                ),
            };

        Self {
            provider,
            display_name: provider.display_name(),
            is_active,
            has_key,
            readiness,
            resolved_model,
            model_provenance,
            base_url,
            context_window,
            max_output,
            thinking_supported,
            cache_telemetry_supported,
            request_payload_mode: mode,
            // Every provider CodeWhale ships today streams responses.
            streaming_supported: true,
            local,
        }
    }
}

/// Build readiness rows for every known provider, in [`ApiProvider::all`] order.
///
/// This is the entry point a `/provider` dashboard would call once when the
/// modal opens. It performs no network I/O.
#[must_use]
pub fn provider_readiness_rows(config: &Config) -> Vec<ProviderReadinessRow> {
    ApiProvider::all()
        .iter()
        .map(|provider| ProviderReadinessRow::for_provider(config, *provider))
        .collect()
}

/// Self-hosted / local-first providers that run without authentication.
fn is_local_provider(provider: ApiProvider) -> bool {
    matches!(
        provider,
        ApiProvider::Sglang | ApiProvider::Vllm | ApiProvider::Ollama
    )
}

/// Providers whose credentials normally come from a CLI OAuth login on disk.
fn uses_cli_oauth(provider: ApiProvider) -> bool {
    match provider {
        ApiProvider::OpenaiCodex => true,
        // Kimi via Moonshot can use a CLI OAuth login; only treat it as OAuth
        // when those credentials are actually present so an explicit
        // MOONSHOT_API_KEY still reads as a plain configured key.
        ApiProvider::Moonshot => kimi_cli_credentials_present(),
        _ => false,
    }
}

/// Map credential + provider facts into a readiness state.
///
/// This only reflects credential presence; it never probes the network.
fn classify_readiness(provider: ApiProvider, has_key: bool, local: bool) -> ProviderReadiness {
    if local {
        // Local providers are usable with or without a key.
        return ProviderReadiness::OptionalKeyLocal;
    }
    if !has_key {
        return ProviderReadiness::NeedsKey;
    }
    if uses_cli_oauth(provider) {
        return ProviderReadiness::OauthReady;
    }
    ProviderReadiness::Configured
}

/// Resolve the model id for a provider plus where it came from.
///
/// For the active provider we defer to [`Config::default_model`], the
/// well-tested resolver that already accounts for normalization, custom
/// base-url pass-through, and provider-scoped overrides. For inactive
/// providers we prefer an explicit saved provider-scoped model, then fall back
/// to the first entry of the provider's built-in catalog.
fn resolve_model(
    config: &Config,
    provider: ApiProvider,
    is_active: bool,
) -> (Option<String>, ModelProvenance) {
    if is_active {
        let model = config.default_model();
        let trimmed = model.trim();
        if trimmed.is_empty() {
            return (None, ModelProvenance::Unknown);
        }
        // A provider-scoped saved model takes precedence as the provenance
        // signal; otherwise the active model is the resolved default.
        let provenance = if saved_model(config, provider).is_some() {
            ModelProvenance::Saved
        } else {
            ModelProvenance::Default
        };
        return (Some(trimmed.to_string()), provenance);
    }

    if let Some(saved) = saved_model(config, provider) {
        return (Some(saved), ModelProvenance::Saved);
    }

    // Fall back to the provider's built-in catalog default (first entry).
    match model_completion_names_for_provider(provider).first() {
        Some(model) => (Some((*model).to_string()), ModelProvenance::Default),
        None => (None, ModelProvenance::Unknown),
    }
}

/// A non-empty, provider-scoped saved model override, if present.
fn saved_model(config: &Config, provider: ApiProvider) -> Option<String> {
    config
        .provider_config_for(provider)
        .and_then(|entry| entry.model.clone())
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
}

/// A non-empty, provider-scoped saved base URL, if present.
fn saved_base_url(config: &Config, provider: ApiProvider) -> Option<String> {
    config
        .provider_config_for(provider)
        .and_then(|entry| entry.base_url.clone())
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal default config; tests then set specific fields.
    fn base_config() -> Config {
        Config::default()
    }

    #[test]
    fn local_provider_is_optional_key_and_usable() {
        let config = base_config();
        let row = ProviderReadinessRow::for_provider(&config, ApiProvider::Ollama);
        assert_eq!(row.provider, ApiProvider::Ollama);
        assert!(row.local);
        assert_eq!(row.readiness, ProviderReadiness::OptionalKeyLocal);
        assert!(row.readiness.is_usable());
        assert!(row.has_key, "self-hosted providers report a usable key");
        assert!(row.streaming_supported);
    }

    #[test]
    fn hosted_provider_without_key_needs_key_and_is_not_usable() {
        // Ensure no ambient credential leaks in from the environment.
        let _guard = EnvGuard::remove(&["FIREWORKS_API_KEY", "DEEPSEEK_API_KEY", "OPENAI_API_KEY"]);
        let config = base_config();
        let row = ProviderReadinessRow::for_provider(&config, ApiProvider::Fireworks);
        // With no env/config key, a hosted provider needs a key.
        assert!(!row.has_key);
        assert_eq!(row.readiness, ProviderReadiness::NeedsKey);
        assert!(!row.readiness.is_usable());
    }

    #[test]
    fn rows_cover_every_provider_in_canonical_order() {
        let config = base_config();
        let rows = provider_readiness_rows(&config);
        let providers: Vec<ApiProvider> = rows.iter().map(|row| row.provider).collect();
        assert_eq!(providers, ApiProvider::all().to_vec());
        assert_eq!(rows.len(), ApiProvider::all().len());
    }

    #[test]
    fn active_provider_is_flagged_and_resolves_a_model() {
        // Default config's active provider is DeepSeek.
        let config = base_config();
        let active = config.api_provider();
        let rows = provider_readiness_rows(&config);
        let active_rows: Vec<&ProviderReadinessRow> =
            rows.iter().filter(|row| row.is_active).collect();
        assert_eq!(active_rows.len(), 1, "exactly one active provider");
        let row = active_rows[0];
        assert_eq!(row.provider, active);
        // The active provider always resolves a concrete model id.
        assert!(
            row.resolved_model.is_some(),
            "active provider must resolve a model"
        );
        // And it carries non-empty capability metadata.
        assert!(row.context_window.unwrap_or(0) > 0);
        assert!(row.max_output.unwrap_or(0) > 0);
    }

    #[test]
    fn deepseek_v4_active_model_reports_million_window_and_thinking() {
        let mut config = base_config();
        config.provider = Some("deepseek".to_string());
        config.default_text_model = Some("deepseek-v4-pro".to_string());
        let row = ProviderReadinessRow::for_provider(&config, ApiProvider::Deepseek);
        assert!(row.is_active);
        assert_eq!(
            row.resolved_model.as_deref(),
            Some("deepseek-v4-pro"),
            "active DeepSeek model resolves through default_model"
        );
        // V4-class context window is 1M tokens and thinking is supported.
        assert_eq!(
            row.context_window,
            Some(crate::models::DEEPSEEK_V4_CONTEXT_WINDOW_TOKENS)
        );
        assert_eq!(row.max_output, Some(384_000));
        assert!(row.thinking_supported);
        // DeepSeek-native endpoints expose cache telemetry.
        assert!(row.cache_telemetry_supported);
        assert_eq!(
            row.request_payload_mode,
            RequestPayloadMode::ChatCompletions
        );
    }

    #[test]
    fn inactive_provider_falls_back_to_catalog_default_model() {
        // DeepSeek is active by default, so Moonshot is inactive here and
        // should resolve to its built-in catalog default with `Default`
        // provenance.
        let config = base_config();
        let row = ProviderReadinessRow::for_provider(&config, ApiProvider::Moonshot);
        assert!(!row.is_active);
        let expected = model_completion_names_for_provider(ApiProvider::Moonshot)
            .first()
            .map(|m| (*m).to_string());
        assert_eq!(row.resolved_model, expected);
        assert_eq!(row.model_provenance, ModelProvenance::Default);
    }

    #[test]
    fn unknown_pricing_and_context_are_explicit_when_no_model_known() {
        // Ollama's catalog list is empty and it is inactive by default, so no
        // model id is known: metadata must be explicitly None, not fabricated.
        let config = base_config();
        let row = ProviderReadinessRow::for_provider(&config, ApiProvider::Ollama);
        assert!(!row.is_active);
        assert_eq!(row.resolved_model, None);
        assert_eq!(row.model_provenance, ModelProvenance::Unknown);
        assert_eq!(row.context_window, None);
        assert_eq!(row.max_output, None);
        assert!(!row.thinking_supported);
    }

    #[test]
    fn saved_base_url_surfaces_for_self_hosted_provider() {
        let mut config = base_config();
        config.provider_config_for_mut(ApiProvider::Vllm).base_url =
            Some("http://gpu-box.internal:8000/v1".to_string());
        let row = ProviderReadinessRow::for_provider(&config, ApiProvider::Vllm);
        assert_eq!(
            row.base_url.as_deref(),
            Some("http://gpu-box.internal:8000/v1")
        );
        assert!(row.local);
        assert_eq!(row.readiness, ProviderReadiness::OptionalKeyLocal);
    }

    #[test]
    fn saved_provider_model_marks_saved_provenance() {
        let mut config = base_config();
        config
            .provider_config_for_mut(ApiProvider::Openrouter)
            .model = Some("some-vendor/custom-model".to_string());
        let row = ProviderReadinessRow::for_provider(&config, ApiProvider::Openrouter);
        assert!(!row.is_active);
        assert_eq!(
            row.resolved_model.as_deref(),
            Some("some-vendor/custom-model")
        );
        assert_eq!(row.model_provenance, ModelProvenance::Saved);
    }

    #[test]
    fn readiness_labels_are_stable() {
        // Every variant — including the ones reserved for the future cached
        // health layer — has a stable label, so the dashboard can render any
        // state the substrate may eventually report.
        assert_eq!(ProviderReadiness::Configured.label(), "configured");
        assert_eq!(ProviderReadiness::NeedsKey.label(), "needs key");
        assert_eq!(ProviderReadiness::OauthReady.label(), "OAuth ready");
        assert_eq!(
            ProviderReadiness::OptionalKeyLocal.label(),
            "optional key (local)"
        );
        assert_eq!(ProviderReadiness::Unreachable.label(), "unreachable");
        assert_eq!(ProviderReadiness::AuthFailed.label(), "auth failed");
        assert_eq!(ProviderReadiness::Unknown.label(), "unknown");

        // Credential-derived usability: only configured/oauth/local are usable;
        // the reserved health states are not (yet) usable.
        assert!(ProviderReadiness::Configured.is_usable());
        assert!(ProviderReadiness::OauthReady.is_usable());
        assert!(ProviderReadiness::OptionalKeyLocal.is_usable());
        assert!(!ProviderReadiness::NeedsKey.is_usable());
        assert!(!ProviderReadiness::Unreachable.is_usable());
        assert!(!ProviderReadiness::AuthFailed.is_usable());
        assert!(!ProviderReadiness::Unknown.is_usable());

        assert_eq!(ModelProvenance::Default.label(), "default");
        assert_eq!(ModelProvenance::Saved.label(), "saved");
        assert_eq!(ModelProvenance::Custom.label(), "custom");
        assert_eq!(ModelProvenance::Catalog.label(), "catalog");
        assert_eq!(ModelProvenance::Unknown.label(), "unknown");
    }

    /// RAII guard that removes env vars for the duration of a test and
    /// restores any prior values on drop, so credential-sensitive assertions
    /// are deterministic regardless of the developer's environment.
    struct EnvGuard {
        saved: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn remove(keys: &[&str]) -> Self {
            let saved = keys
                .iter()
                .map(|key| {
                    let prior = std::env::var(key).ok();
                    // SAFETY: tests in this module run single-threaded per the
                    // crate's test harness expectations for env mutation.
                    unsafe {
                        std::env::remove_var(key);
                    }
                    ((*key).to_string(), prior)
                })
                .collect();
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, prior) in &self.saved {
                match prior {
                    Some(value) => unsafe { std::env::set_var(key, value) },
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }
}
