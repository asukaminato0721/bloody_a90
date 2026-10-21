use bloody_a90::{
    gestures::{Action, Document, Kind},
    host_open::{Plan, Queue},
    host_program::{self, Output, Vm},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
fn saved_gesture_launches_tool_with_literal_arguments() {
    struct Files(PathBuf);
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let files = Files(std::env::temp_dir().join(format!("a90-tool-{}", std::process::id())));
    fs::create_dir_all(&files.0).unwrap();
    let program = files.0.join("tool with spaces $(false).sh");
    let record = files.0.join("arguments");
    let unexpected = files.0.join("must-not-exist");
    fs::write(
        &program,
        "#!/bin/sh\nresult=$1\nshift\nprintf '%s\\000' \"$@\" > \"$result\"\n",
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let literal = format!("$(touch {})", unexpected.display());
    let arguments = format!(
        "{}\n--region\r\n中文 & < > 😀\n\n spaces \n{literal}\n'quoted'\n",
        record.display()
    );
    let mut document = Document::new(Kind::Mouse);
    document.actions[0] =
        Action::open_target_with_arguments(program.to_str().unwrap(), &arguments).unwrap();
    let restored = Document::parse(&document.to_bytes()).unwrap();
    assert_eq!(restored, document);
    let prepared = host_program::prepare(&restored.actions[0]).unwrap();
    assert!(!prepared.needs_text() && !prepared.needs_window());
    let now = Instant::now();
    let mut vm = Vm::new(prepared, now, false);
    let outputs = vm.tick(now, [0; 3]).unwrap();
    let [Output::Open(plan)] = outputs.as_slice() else {
        panic!("unexpected output: {outputs:?}");
    };
    assert!(vm.finished());
    assert!(vm.tick(now, [0; 3]).unwrap().is_empty());
    let mut queue = Queue::default();
    queue.launch(plan.clone()).unwrap();
    let expected = [
        "--region",
        "中文 & < > 😀",
        "",
        " spaces ",
        &literal,
        "'quoted'",
    ]
    .into_iter()
    .flat_map(|argument| argument.bytes().chain([0]))
    .collect::<Vec<_>>();
    let deadline = Instant::now() + Duration::from_secs(2);
    while fs::read(&record).ok().as_deref() != Some(expected.as_slice()) {
        assert!(queue.poll().is_empty(), "tool launch failed");
        assert!(
            Instant::now() < deadline,
            "tool did not receive literal arguments"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !unexpected.exists(),
        "argument text was executed as a command"
    );
}

#[test]
fn tool_arguments_require_an_executable_and_have_limits() {
    assert_eq!(
        Plan::prepare_with_arguments("/bin/true", "").unwrap(),
        Plan::prepare("/bin/true").unwrap()
    );
    assert_eq!(
        Plan::prepare_with_arguments("/bin/true", "\n").unwrap(),
        Plan::ProgramWithArguments("/bin/true".into(), vec![String::new()])
    );
    for target in ["https://example.invalid", "/tmp", "C:\\Tool.exe"] {
        assert!(Plan::prepare_with_arguments(target, "--region").is_err());
    }
    for arguments in [
        "bad\0arg".into(),
        "bad\u{1}arg".into(),
        "x".repeat(65537),
        "x\n".repeat(129),
    ] {
        assert!(Plan::prepare_with_arguments("/bin/true", &arguments).is_err());
    }
}
