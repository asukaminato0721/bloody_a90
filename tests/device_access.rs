use bloody_a90::device::Device;
use std::process::Command;

#[test]
#[ignore = "requires an A90; queries identity and tests exclusive HID access without writing settings"]
fn a_second_process_waits_and_reports_busy_before_sending_a_request() {
    let device = Device::open().unwrap();
    let blocked = Command::new(env!("CARGO_BIN_EXE_bloody_a90"))
        .arg("--device")
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("正由其他操作使用"));
    // The other process did not disturb the first handle's response.
    assert_eq!(device.identity().unwrap().firmware, 7);
    drop(device);
    let reopened = Command::new(env!("CARGO_BIN_EXE_bloody_a90"))
        .arg("--device")
        .output()
        .unwrap();
    assert!(
        reopened.status.success(),
        "{}",
        String::from_utf8_lossy(&reopened.stderr)
    );
    assert!(String::from_utf8_lossy(&reopened.stdout).contains("固件 07"));
}
