use mica::caps::Caps;
use mica::config::Config;

#[test]
fn parses_w3c_always_match() {
    let body = serde_json::json!({
        "capabilities": { "alwaysMatch": { "browserName": "chrome", "browserVersion": "125" } }
    });
    let caps = Caps::parse(&body).expect("parse");
    assert_eq!(caps.browser_name.as_deref(), Some("chrome"));
    assert_eq!(caps.browser_version.as_deref(), Some("125"));
}

#[test]
fn parses_legacy_desired_capabilities() {
    let body = serde_json::json!({
        "desiredCapabilities": { "browserName": "firefox", "version": "126.0" }
    });
    let caps = Caps::parse(&body).expect("parse");
    assert_eq!(caps.browser_name.as_deref(), Some("firefox"));
    assert_eq!(caps.browser_version.as_deref(), Some("126.0"));
}

#[test]
fn extracts_mica_extensions() {
    let body = serde_json::json!({
        "capabilities": {
            "alwaysMatch": {
                "browserName": "chrome",
                "mica:options": {
                    "enableVNC": true,
                    "enableVideo": true,
                    "screenResolution": "1280x1024x24",
                    "name": "my-test"
                }
            }
        }
    });
    let caps = Caps::parse(&body).expect("parse");
    assert!(caps.enable_vnc);
    assert!(caps.enable_video);
    assert_eq!(caps.screen_resolution.as_deref(), Some("1280x1024x24"));
    assert_eq!(caps.name.as_deref(), Some("my-test"));
}

#[test]
fn missing_capabilities_errors() {
    let body = serde_json::json!({ "something": "else" });
    assert!(Caps::parse(&body).is_err());
}

#[test]
fn caps_resolve_to_browser() {
    let cfg = Config::load("tests/fixtures/browsers.json").unwrap();
    let body = serde_json::json!({
        "capabilities": { "alwaysMatch": { "browserName": "firefox" } }
    });
    let caps = Caps::parse(&body).unwrap();
    let (browser, version) = cfg
        .find(
            caps.browser_name.as_deref().unwrap_or(""),
            caps.browser_version.as_deref(),
        )
        .expect("found");
    assert_eq!(version, "126.0");
    assert_eq!(browser.docker_image(), Some("selenoid/firefox:126.0"));
}
