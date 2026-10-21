use bloody_a90::{
    device::Device,
    vendor_input::{Session, Source, State},
};
use std::time::{Duration, Instant};

struct Restore<'a> {
    device: &'a Device,
    profile: u8,
    armed: bool,
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        if self.armed
            && let Err(error) = self.device.select_profile(self.profile)
        {
            eprintln!("恢复配置 {} 失败：{error}", self.profile + 1);
        }
    }
}

fn wait_profile(mut read: impl FnMut() -> Result<Vec<State>, String>, profile: u8) -> State {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(state) = read()
            .unwrap()
            .into_iter()
            .find(|state| state.profile == profile)
        {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "没有收到配置 {} 的厂商通知",
            profile + 1
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "requires the A90 and A90_TEST_NATIVE_INPUT=1; restores runtime profile and notification mode"]
fn independent_readers_receive_profile_notifications_during_feature_access() {
    assert_eq!(std::env::var("A90_TEST_NATIVE_INPUT").as_deref(), Ok("1"));
    let device = Device::open().unwrap();
    let (current, maximum) = device.runtime_profile().unwrap();
    assert!(
        device.held_buttons().unwrap().is_empty(),
        "请先松开鼠标全部按键"
    );
    assert!(
        maximum > 0 && current <= maximum,
        "测试需要至少两个有效配置"
    );
    let mut first = Source::for_device(&device).unwrap();
    let mut second = Source::for_device(&device).unwrap();
    first.read().unwrap();
    second.read().unwrap();
    let previous_mode = device.ram(0x4b).unwrap();
    let mut session = Session::start(&device).unwrap();
    assert_eq!(device.ram(0x4b).unwrap(), previous_mode | 1);
    let state = wait_profile(|| session.read(), current);
    assert_eq!(state.held_buttons(), device.held_buttons().unwrap());
    assert_eq!(
        state.sensitivity,
        device.runtime_sensitivity().unwrap().selected
    );
    let mut restore = Restore {
        device: &device,
        profile: current,
        armed: true,
    };
    let target = if current == 0 { 1 } else { 0 };
    assert_eq!(device.select_profile(target).unwrap().0, target);
    wait_profile(|| first.read(), target);
    wait_profile(|| second.read(), target);
    assert_eq!(device.identity().unwrap().firmware, 7);
    assert_eq!(device.select_profile(current).unwrap().0, current);
    wait_profile(|| first.read(), current);
    wait_profile(|| second.read(), current);
    session.restore().unwrap();
    assert_eq!(device.ram(0x4b).unwrap(), previous_mode);
    restore.armed = false;
    assert_eq!(device.runtime_profile().unwrap(), (current, maximum));
    // The fallback cleanup must restore the mode without an explicit call.
    {
        let _session = Session::start(&device).unwrap();
        assert_eq!(device.ram(0x4b).unwrap(), previous_mode | 1);
    }
    assert_eq!(device.ram(0x4b).unwrap(), previous_mode);
}
