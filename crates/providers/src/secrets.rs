use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("env var '{0}' referenced in secret field is not set")]
    Missing(String),
}

/// Resolve an API key value.
///
/// - `None` or `""` → `Ok(None)`
/// - `"env:NAME"` → reads `NAME` from process env; `Err(SecretError::Missing)` if not set
/// - any other string → `Ok(Some(value.to_owned()))`
pub fn resolve_secret(value: Option<&str>) -> Result<Option<String>, SecretError> {
    match value {
        None | Some("") => Ok(None),
        Some(v) if v.starts_with("env:") => {
            let name = &v[4..];
            std::env::var(name)
                .map(Some)
                .map_err(|_| SecretError::Missing(name.to_owned()))
        }
        Some(v) => Ok(Some(v.to_owned())),
    }
}
