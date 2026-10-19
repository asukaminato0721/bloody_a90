//! Linux counterpart of Simulator 20 / ShellExecuteW. No shell command parsing.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    Program(PathBuf),
    ProgramWithArguments(PathBuf, Vec<String>),
    Desktop(String),
}

impl Plan {
    /// Linux extension. Each line is one literal argument. A final line ending
    /// terminates the preceding argument, as in a text file.
    pub fn prepare_with_arguments(target: &str, arguments: &str) -> Result<Self, String> {
        if arguments.is_empty() {
            return Self::prepare(target);
        }
        if arguments.len() > 64 * 1024
            || arguments
                .chars()
                .any(|c| c < ' ' && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err("程序参数超过 64 KiB，或包含不支持的控制字符".into());
        }
        let arguments: Vec<String> = arguments.lines().map(String::from).collect();
        if arguments.len() > 128 {
            return Err("程序参数超过 128 个".into());
        }
        match Self::prepare(target)? {
            Self::Program(path) => Ok(Self::ProgramWithArguments(path, arguments)),
            _ => Err("带参数的目标必须是 Linux 可执行文件。请填写程序路径。".into()),
        }
    }

    pub fn prepare(target: &str) -> Result<Self, String> {
        if target.trim().is_empty() || target.contains('\0') {
            return Err("打开目标不能为空，也不能包含空字符".into());
        }
        if let Some((scheme, rest)) = target.split_once(':') {
            if scheme.len() == 1 && (rest.starts_with('\\') || rest.starts_with('/')) {
                return Err("请把 Windows 打开目标改为 Linux 路径".into());
            }
            if scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            {
                return Ok(Self::Desktop(target.into()));
            }
        }
        let path = Path::new(target);
        if let Ok(metadata) = fs::metadata(path) {
            let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
            if metadata.is_file()
                && metadata.permissions().mode() & 0o111 != 0
                && path.extension().is_none_or(|ext| ext != "desktop")
            {
                return Ok(Self::Program(absolute));
            }
            return Ok(Self::Desktop(absolute.to_string_lossy().into_owned()));
        }
        Err(format!("找不到打开目标：{target}"))
    }

    fn command(&self) -> Command {
        let mut command = match self {
            Self::Program(path) => Command::new(path),
            Self::ProgramWithArguments(path, arguments) => {
                let mut command = Command::new(path);
                command.args(arguments);
                command
            }
            Self::Desktop(target) => {
                let mut command = Command::new("xdg-open");
                command.arg(target);
                command
            }
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
}

pub struct Queue {
    sender: SyncSender<(u64, Arc<Plan>)>,
    results: Receiver<Result<(), String>>,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

impl Default for Queue {
    fn default() -> Self {
        let (sender, requests) = mpsc::sync_channel::<(u64, Arc<Plan>)>(32);
        let (replies, results) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let current_generation = generation.clone();
        std::thread::spawn(move || {
            let mut children: Vec<Child> = Vec::new();
            loop {
                if stopping.load(Ordering::Acquire) {
                    // Already launched applications can outlive the driver.
                    // Pending requests are cancelled. This thread only reaps.
                    while !children.is_empty() {
                        children.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
                        if !children.is_empty() {
                            std::thread::sleep(Duration::from_millis(200));
                        }
                    }
                    return;
                }
                match requests.recv_timeout(Duration::from_millis(10)) {
                    Ok((generation, plan)) => {
                        if stopping.load(Ordering::Acquire) {
                            continue;
                        }
                        if generation != current_generation.load(Ordering::Acquire) {
                            continue;
                        }
                        let result = if children.len() >= 32 {
                            Err("打开目标的进程队列已满，请稍后重试".into())
                        } else {
                            plan.command()
                                .spawn()
                                .map(|child| children.push(child))
                                .map_err(|e| format!("无法打开目标：{e}"))
                        };
                        if let Err(error) = result {
                            let _ = replies.try_send(Err(error));
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        stopping.store(true, Ordering::Release)
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                children.retain_mut(|child| match child.try_wait() {
                    Ok(None) => true,
                    Ok(Some(status)) => {
                        let result = if status.success() {
                            Ok(())
                        } else {
                            Err(format!("打开目标失败，进程状态：{status}"))
                        };
                        let _ = replies.try_send(result);
                        false
                    }
                    Err(error) => {
                        let _ =
                            replies.try_send(Err(format!("无法读取打开目标的进程状态：{error}")));
                        false
                    }
                });
            }
        });
        Self {
            sender,
            results,
            stop,
            generation,
        }
    }
}

impl Queue {
    pub fn launch(&mut self, plan: Arc<Plan>) -> Result<(), String> {
        self.sender
            .try_send((self.generation.load(Ordering::Acquire), plan))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "打开目标请求队列已满，请稍后重试".into(),
                mpsc::TrySendError::Disconnected(_) => "打开目标线程已退出".into(),
            })
    }
    pub fn poll(&mut self) -> Vec<String> {
        self.results.try_iter().filter_map(Result::err).collect()
    }
    pub fn cancel_pending(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_paths_uris_and_child_cleanup() {
        let root = std::env::temp_dir().join(format!("a90-open-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("program with spaces $(false).sh");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let plan = Plan::prepare(executable.to_str().unwrap()).unwrap();
        assert_eq!(plan, Plan::Program(executable));
        let mut queue = Queue::default();
        queue.launch(Arc::new(plan)).unwrap();
        queue
            .results
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        let document = root.join("-document with spaces.txt");
        fs::write(&document, "test").unwrap();
        let plan = Plan::prepare(document.to_str().unwrap()).unwrap();
        assert_eq!(
            plan.command().get_args().collect::<Vec<_>>(),
            [document.as_os_str()]
        );
        let uri = "https://example.invalid/a%20b?literal=$(false)";
        assert_eq!(
            Plan::prepare(uri)
                .unwrap()
                .command()
                .get_args()
                .collect::<Vec<_>>(),
            [std::ffi::OsStr::new(uri)]
        );
        assert!(Plan::prepare("C:\\Program Files\\test.exe").is_err());
        assert!(Plan::prepare("missing-a90-test-file").is_err());
        assert!(Plan::prepare("bad\0path").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
