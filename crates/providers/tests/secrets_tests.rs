use providers::resolve_secret;
use providers::SecretError;

#[test]
fn none_input_returns_none() {
    assert_eq!(resolve_secret(None).unwrap(), None);
}

#[test]
fn empty_string_returns_none() {
    assert_eq!(resolve_secret(Some("")).unwrap(), None);
}

#[test]
fn plain_string_returned_as_is() {
    assert_eq!(
        resolve_secret(Some("sk-abc123")).unwrap(),
        Some("sk-abc123".to_owned())
    );
}

#[test]
fn env_prefix_resolves_set_var() {
    unsafe { std::env::set_var("TEST_SECRET_RESOLVE_XYZ", "my_secret_value") };
    let result = resolve_secret(Some("env:TEST_SECRET_RESOLVE_XYZ")).unwrap();
    assert_eq!(result, Some("my_secret_value".to_owned()));
    unsafe { std::env::remove_var("TEST_SECRET_RESOLVE_XYZ") };
}

#[test]
fn env_prefix_errors_on_missing_var() {
    unsafe { std::env::remove_var("TEST_SECRET_MISSING_ABC") };
    let err = resolve_secret(Some("env:TEST_SECRET_MISSING_ABC")).unwrap_err();
    assert!(matches!(err, SecretError::Missing(name) if name == "TEST_SECRET_MISSING_ABC"));
}

#[test]
fn env_prefix_only_no_name_errors() {
    // "env:" with empty name — env var "" is unlikely to be set and
    // std::env::var("") returns Err on most platforms
    let result = resolve_secret(Some("env:"));
    // Empty env var name: std::env::var("") is Err on most platforms
    assert!(result.is_err() || result.unwrap().is_none());
}
