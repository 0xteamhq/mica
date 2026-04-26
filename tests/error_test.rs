use mica::error::WdError;

#[test]
fn session_not_created_shape() {
    let err = WdError::session_not_created("Browser not found: safari");
    let json = serde_json::to_value(&err).unwrap();
    let v = json.get("value").expect("value");
    assert_eq!(
        v.get("error").and_then(|x| x.as_str()),
        Some("session not created")
    );
    assert!(
        v.get("message")
            .and_then(|x| x.as_str())
            .unwrap()
            .contains("safari")
    );
    assert!(v.get("stacktrace").is_some());
}

#[test]
fn invalid_argument_shape() {
    let err = WdError::invalid_argument("bad caps");
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(
        json.get("value")
            .and_then(|v| v.get("error"))
            .and_then(|e| e.as_str()),
        Some("invalid argument")
    );
}

#[test]
fn unknown_error_shape() {
    let err = WdError::unknown_error("boom");
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(
        json.get("value")
            .and_then(|v| v.get("error"))
            .and_then(|e| e.as_str()),
        Some("unknown error")
    );
}
