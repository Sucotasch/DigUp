//! DigUp's helper processes, which do all the work: `digup search-service` answers the panel (a JSON request per line
//! in, the reply on a line out with the request's id; lines without an id are events), and `digup index-service` keeps
//! the index current (commands in, events out) and exits when it's done. Sources/digup has both protocols. Each one's
//! output is read on a thread of its own, which waits for the process when its output ends, so none is left a zombie.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

type EventSink = Arc<dyn Fn(Value) + Send + Sync>;

/// How a helper is started.
#[derive(Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Where its standard error goes (llama.cpp and the helper write their warnings there).
    pub log: Option<PathBuf>,
    /// Work in the background (indexing, the download): below the person's own programs for the processor.
    pub background: bool,
}

impl Launch {
    pub(crate) fn spawn(&self) -> Result<(Child, ChildStdin, ChildStdout), String> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(match self.log.as_ref().and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok()) {
                Some(file) => Stdio::from(file),
                None => Stdio::null(),
            });
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW (no console flashes up), and BELOW_NORMAL_PRIORITY_CLASS for background work.
            command.creation_flags(0x0800_0000 | if self.background { 0x0000_4000 } else { 0 });
        }
        #[cfg(unix)]
        if self.background {
            use std::os::unix::process::CommandExt;
            // SAFETY: nice() only changes the new process's own priority, between fork and exec.
            unsafe {
                command.pre_exec(|| {
                    libc::nice(10);
                    Ok(())
                });
            }
        }
        let mut child = command.spawn().map_err(|error| format!("can't start {}: {error}", self.program.display()))?;
        #[cfg(windows)]
        job::add(&child);
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        Ok((child, stdin, stdout))
    }
}

/// A process started apart from the helpers here (the speed check, folder estimates): on Windows it joins their job, so it
/// ends with DigUp Plus.
pub fn adopt(child: &Child) {
    #[cfg(windows)]
    job::add(child);
    #[cfg(not(windows))]
    let _ = child;
}

/// Asks the process to stop (`farewell` on stdin, then stdin closed), and ends it if it hasn't within two seconds.
fn stop(mut child: Child, mut stdin: ChildStdin, farewell: &str) {
    let _ = writeln!(stdin, "{farewell}").and_then(|_| stdin.flush());
    drop(stdin);
    for _ in 0..40 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Each helper started gets a number, so the thread reading it knows whether its slot still holds it or a newer one.
static STARTED: AtomicU64 = AtomicU64::new(1);

/// A helper whose output ended is on its way out: waits for it (up to five seconds) and gives its exit code.
fn reap(child: &mut Child) -> Option<i32> {
    for _ in 0..100 {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
    None
}

/// `digup search-service`: started on the first request, and again after it died.
pub struct SearchService {
    /// The launch for the next process: the model folder and the device can change while the shell runs.
    launch: Mutex<Launch>,
    running: Arc<Mutex<Option<Searching>>>,
    next_id: AtomicU64,
    on_event: EventSink,
}

struct Searching {
    child: Child,
    stdin: ChildStdin,
    number: u64,
    /// Replies still to come, by request id. Each process has its own, so one that dies fails only its own requests.
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
}

impl SearchService {
    pub fn new(launch: Launch, on_event: impl Fn(Value) + Send + Sync + 'static) -> SearchService {
        SearchService {
            launch: Mutex::new(launch),
            running: Arc::default(),
            next_id: AtomicU64::new(1),
            on_event: Arc::new(on_event),
        }
    }

    /// The next search process starts with this launch (a new model folder, the processor instead of the GPU): the one
    /// running now is stopped, so the change reaches the helper instead of waiting for the next start of DigUp.
    pub fn set_launch(&self, launch: Launch) {
        // The running process is taken while the launch is still held: a request can't slip in with the old
        // environment between the two.
        let mut current = self.launch.lock().unwrap();
        *current = launch;
        let running = self.running.lock().unwrap().take();
        drop(current);
        if let Some(searching) = running {
            stop(searching.child, searching.stdin, &json!({"op": "quit"}).to_string());
        }
    }

    #[cfg(test)]
    fn env(&self) -> Vec<(String, String)> {
        self.launch.lock().unwrap().env.clone()
    }

    /// Sends a request and waits for its reply.
    pub async fn ask(&self, mut request: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        request["id"] = json!(id);
        let reply = self.send(id, &request)?;
        let reply = reply.await.map_err(|_| "DigUp's helper stopped".to_string())?;
        match reply.get("ok") {
            Some(Value::Bool(false)) => Err(reply["error"].as_str().unwrap_or("the helper failed").to_string()),
            _ => Ok(reply),
        }
    }

    fn send(&self, id: u64, request: &Value) -> Result<oneshot::Receiver<Value>, String> {
        // The launch stays locked until the process is started: a request that arrives as the folder changes either
        // starts with the old folder before `set_launch`, or (holding both locks) with the new one after it — never
        // with the old environment and the new folder's process slot.
        let launch = self.launch.lock().unwrap();
        let mut running = self.running.lock().unwrap();
        if let Some(searching) = running.as_mut() {
            if !matches!(searching.child.try_wait(), Ok(None)) {
                *running = None;
            }
        }
        if running.is_none() {
            *running = Some(self.start(&launch)?);
        }
        let searching = running.as_mut().unwrap();
        let (sender, receiver) = oneshot::channel();
        searching.pending.lock().unwrap().insert(id, sender);
        if let Err(error) = writeln!(searching.stdin, "{request}").and_then(|_| searching.stdin.flush()) {
            searching.pending.lock().unwrap().remove(&id);
            *running = None;
            return Err(format!("DigUp's helper isn't answering: {error}"));
        }
        Ok(receiver)
    }

    fn start(&self, launch: &Launch) -> Result<Searching, String> {
        let (child, stdin, stdout) = launch.spawn()?;
        let number = STARTED.fetch_add(1, Ordering::Relaxed);
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> = Arc::default();
        let (replies, on_event, slot) = (pending.clone(), self.on_event.clone(), self.running.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
                match message.get("id").and_then(Value::as_u64) {
                    Some(id) => {
                        if let Some(sender) = replies.lock().unwrap().remove(&id) {
                            let _ = sender.send(message);
                        }
                    }
                    None => on_event(message),
                }
            }
            // Gone: its waiting requests fail (their senders drop here), and the next request starts another.
            replies.lock().unwrap().clear();
            let mut slot = slot.lock().unwrap();
            if slot.as_ref().is_some_and(|searching| searching.number == number) {
                if let Some(mut searching) = slot.take() {
                    reap(&mut searching.child);
                }
            }
            drop(slot);
            on_event(json!({"event": "exit"}));
        });
        Ok(Searching { child, stdin, number, pending })
    }

    pub fn stop(&self) {
        if let Some(searching) = self.running.lock().unwrap().take() {
            stop(searching.child, searching.stdin, &json!({"op": "quit"}).to_string());
        }
    }
}

/// `digup index-service`: syncs the index with the folders, indexes what's new, and exits once it's done (after a
/// minute with nothing to do). `run` starts it, asks the running one to sync again, or, when the folders changed (another
/// launch), starts it over with them. When it ends by itself, `on_event` gets {"event": "ended", "status": its exit
/// code, "owed": whether a sync asked of it never started}; one stopped here doesn't report.
pub struct IndexService {
    running: Arc<Mutex<Option<Indexing>>>,
    on_event: EventSink,
}

struct Indexing {
    child: Child,
    stdin: ChildStdin,
    args: Vec<String>,
    number: u64,
    /// Asked to sync, and it hasn't started that sync yet: owed if it exits first (it was leaving as it was asked).
    owed: bool,
}

impl IndexService {
    pub fn new(on_event: impl Fn(Value) + Send + Sync + 'static) -> IndexService {
        IndexService { running: Arc::default(), on_event: Arc::new(on_event) }
    }

    pub fn run(&self, launch: &Launch) -> Result<(), String> {
        let mut running = self.running.lock().unwrap();
        if let Some(indexing) = running.as_mut() {
            if matches!(indexing.child.try_wait(), Ok(None)) && indexing.args == launch.args {
                indexing.owed = true;
                return writeln!(indexing.stdin, "sync").and_then(|_| indexing.stdin.flush()).map_err(|e| e.to_string());
            }
        }
        // Gone, or other folders: (stop it and) start anew.
        if let Some(Indexing { child, stdin, .. }) = running.take() {
            stop(child, stdin, "stop");
        }
        let (child, stdin, stdout) = launch.spawn()?;
        let number = STARTED.fetch_add(1, Ordering::Relaxed);
        let (on_event, slot) = (self.on_event.clone(), self.running.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
                if event["event"] == "sync" {
                    if let Some(indexing) = slot.lock().unwrap().as_mut().filter(|indexing| indexing.number == number) {
                        indexing.owed = false;
                    }
                }
                on_event(event);
            }
            let mut slot = slot.lock().unwrap();
            if slot.as_ref().is_some_and(|indexing| indexing.number == number) {
                if let Some(mut indexing) = slot.take() {
                    drop(slot);
                    let status = reap(&mut indexing.child);
                    on_event(json!({"event": "ended", "status": status, "owed": indexing.owed}));
                }
            }
        });
        *running = Some(Indexing { child, stdin, args: launch.args.clone(), number, owed: false });
        Ok(())
    }

    /// Only what changed under these paths: the helper reconciles their folders instead of walking every chosen one.
    pub fn sync_paths(&self, launch: &Launch, paths: &[PathBuf]) -> Result<(), String> {
        self.sync_paths_as(launch, paths, "sync")
    }

    /// The same for the code index.
    pub fn sync_code_paths(&self, launch: &Launch, paths: &[PathBuf]) -> Result<(), String> {
        self.sync_paths_as(launch, paths, "sync code")
    }

    fn sync_paths_as(&self, launch: &Launch, paths: &[PathBuf], command: &str) -> Result<(), String> {
        if paths.is_empty() {
            // The system dropped events: anything may have changed, so walk everything.
            return self.run(launch);
        }
        let paths: Vec<String> = paths.iter().map(|path| path.to_string_lossy().replace('\\', "/")).collect();
        let request = format!("{command} {}", json!(paths));
        self.request(launch, &request)
    }

    /// A command for a running helper with these folders; one that isn't running starts (and syncs everything).
    fn request(&self, launch: &Launch, command: &str) -> Result<(), String> {
        {
            let mut running = self.running.lock().unwrap();
            if let Some(indexing) = running.as_mut() {
                if matches!(indexing.child.try_wait(), Ok(None)) && indexing.args == launch.args {
                    indexing.owed = true;
                    return writeln!(indexing.stdin, "{command}").and_then(|_| indexing.stdin.flush())
                        .map_err(|error| error.to_string());
                }
            }
        }
        self.run(launch)
    }

    pub fn stop(&self) {
        if let Some(indexing) = self.running.lock().unwrap().take() {
            stop(indexing.child, indexing.stdin, "stop");
        }
    }
}

/// `digup download-model`: the model files, from the Hugging Face cache or downloaded with resume, each checked. One at
/// a time; `run` again while it runs does nothing. Its events ("progress", "done", "error"…) go to `on_event`.
pub struct ModelDownload {
    running: Arc<Mutex<Option<(Child, u64)>>>,
    on_event: EventSink,
}

impl ModelDownload {
    pub fn new(on_event: impl Fn(Value) + Send + Sync + 'static) -> ModelDownload {
        ModelDownload { running: Arc::default(), on_event: Arc::new(on_event) }
    }

    pub fn run(&self, launch: &Launch) -> Result<(), String> {
        let mut running = self.running.lock().unwrap();
        if let Some((child, _)) = running.as_mut() {
            if matches!(child.try_wait(), Ok(None)) {
                return Ok(());
            }
        }
        let (child, stdin, stdout) = launch.spawn()?;
        drop(stdin);
        let number = STARTED.fetch_add(1, Ordering::Relaxed);
        let (on_event, slot) = (self.on_event.clone(), self.running.clone());
        std::thread::spawn(move || {
            let mut ended = false;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(event) = serde_json::from_str::<Value>(&line) {
                    ended |= event["event"] == "done" || event["event"] == "error";
                    on_event(event);
                }
            }
            let mut slot = slot.lock().unwrap();
            if slot.as_ref().is_some_and(|(_, started)| *started == number) {
                if let Some((mut child, _)) = slot.take() {
                    reap(&mut child);
                }
            }
            drop(slot);
            if !ended {
                // No final word: the helper died (after a finished file, on some Linux systems). What to make of it is
                // the app's call: the files may all be there.
                on_event(json!({"event": "stopped"}));
            }
        });
        *running = Some((child, number));
        Ok(())
    }

    pub fn stop(&self) {
        if let Some((mut child, _)) = self.running.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// On Windows the helpers go into a job that closes with this process, so they end even if the shell crashes (on Linux
/// and macOS the helpers watch their parent themselves: `ParentWatch` in Sources/digup). Their own children follow
/// them into it.
#[cfg(test)]
mod tests {
    use super::*;

    fn launch(folder: &str) -> Launch {
        Launch { program: PathBuf::from("digup"), args: vec!["search-service".into()],
                 env: vec![("DIGUP_MODELS".into(), folder.into())], log: None, background: false }
    }

    #[test]
    fn a_new_launch_reaches_the_next_search_process() {
        let service = SearchService::new(launch("C:/old"), |_| {});
        assert_eq!(service.env(), vec![("DIGUP_MODELS".into(), "C:/old".into())]);
        service.set_launch(launch("G:/Ai/DigUp/Models/embeddinggemma-2"));
        assert_eq!(service.env(), vec![("DIGUP_MODELS".into(), "G:/Ai/DigUp/Models/embeddinggemma-2".into())]);
    }
}
#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    struct Job(HANDLE);
    // SAFETY: a job's handle may be used from any thread; it's never closed (the system closes it with this process).
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    fn job() -> Option<&'static Job> {
        static JOB: OnceLock<Option<Job>> = OnceLock::new();
        JOB.get_or_init(|| unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(handle, JobObjectExtendedLimitInformation,
                                              &limits as *const _ as *const _, std::mem::size_of_val(&limits) as u32);
            (set != 0).then_some(Job(handle))
        })
        .as_ref()
    }

    pub fn add(child: &std::process::Child) {
        if let Some(job) = job() {
            // SAFETY: both handles are valid: the job's for this process's life, the child's while `child` lives.
            unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) };
        }
    }

}
