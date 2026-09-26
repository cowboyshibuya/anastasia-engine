#[test]
fn production_telemetry_stays_disabled() {
    let home = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("ANASTASIA_CLI_HOME", home.path()) };

    assert!(!anastasia_telemetry_core::is_enabled());
    anastasia_telemetry_core::begin_session("test-provider", "test-model");
    anastasia_telemetry_core::record_turn();
    assert!(!home.path().join("telemetry_id").exists());
}
