use mica::config::Config;

#[test]
fn loads_browsers_json() {
    let cfg = Config::load("tests/fixtures/browsers.json").expect("load");
    let (browser, version) = cfg.find("firefox", None).expect("find firefox default");
    assert_eq!(version, "126.0");
    assert_eq!(browser.docker_image(), Some("selenoid/firefox:126.0"));
    assert_eq!(browser.port, "4444");
    assert_eq!(browser.path.as_deref(), Some("/wd/hub"));
}

#[test]
fn version_fallback_partial_match() {
    let cfg = Config::load("tests/fixtures/browsers.json").expect("load");
    let (_, version) = cfg.find("chrome", Some("124")).expect("partial match");
    assert_eq!(version, "124.0");
}

#[test]
fn explicit_version_match() {
    let cfg = Config::load("tests/fixtures/browsers.json").expect("load");
    let (browser, version) = cfg.find("chrome", Some("125.0")).expect("exact match");
    assert_eq!(version, "125.0");
    assert_eq!(browser.docker_image(), Some("selenoid/chrome:125.0"));
}

#[test]
fn unknown_browser_returns_none() {
    let cfg = Config::load("tests/fixtures/browsers.json").expect("load");
    assert!(cfg.find("safari", None).is_none());
}

#[test]
fn empty_version_uses_default() {
    let cfg = Config::load("tests/fixtures/browsers.json").expect("load");
    let (_, version) = cfg.find("firefox", Some("")).expect("empty -> default");
    assert_eq!(version, "126.0");
}

#[test]
fn driver_mode_image_rejected() {
    // Phase 1 explicitly drops driver mode — entries whose `image` is an
    // array (a driver argv) must not resolve, so callers get the standard
    // W3C "browser not found" path instead of a panic later.
    let cfg_json = r#"
    {
      "drv": {
        "default": "1",
        "versions": { "1": { "image": ["/usr/bin/geckodriver"], "port": "4444" } }
      }
    }"#;
    let path = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(path.path(), cfg_json).unwrap();
    let cfg = Config::load(path.path()).expect("load");
    assert!(
        cfg.find("drv", None).is_none(),
        "driver-mode image must not resolve"
    );
}
