//! Versioned developer protocol. Runtime calls stay on the viewer event thread.
use crate::developer::{Failure, Output};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub const VERSION: u16 = 1;
const REQUEST_BYTES: usize = 16 * 1024;
const RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const RESPONSE_WAIT: Duration = Duration::from_secs(120);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub version: u16,
    pub id: u64,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Serialize)]
pub struct Reply {
    pub version: u16,
    pub id: Option<u64>,
    pub command: Option<String>,
    pub status: &'static str,
    pub result: Option<Value>,
    pub warnings: Vec<String>,
    pub error: Option<Failure>,
}

impl Reply {
    pub fn result(id: u64, command: &str, output: Result<Output, Failure>) -> Self {
        let (status, result, error) = match output {
            Ok(Output::Data(data)) => ("completed", Some(data), None),
            Ok(Output::Accepted(data)) => ("accepted", Some(data), None),
            Ok(Output::Clear) => ("completed", Some(json!({"clear":true})), None),
            Err(failure) => ("failed", None, Some(failure)),
        };
        let mut warnings = Vec::new();
        if let Some(data) = &result {
            if data["truncated"] == true {
                warnings
                    .push("Object or issue results reached their limit; narrow the query".into());
            }
            if data.get("view_ready").is_some_and(|value| *value == false) {
                warnings.push("Destination loaded; poll pos for the new presented view".into());
            }
        }
        Self {
            version: VERSION,
            id: Some(id),
            command: Some(command.into()),
            status,
            result,
            warnings,
            error,
        }
    }

    fn malformed(failure: Failure) -> Self {
        Self {
            version: VERSION,
            id: None,
            command: None,
            status: "failed",
            result: None,
            warnings: Vec::new(),
            error: Some(failure),
        }
    }
}

pub struct Job {
    pub call: Call,
    pub reply: mpsc::SyncSender<Reply>,
}

pub struct Server {
    pub jobs: mpsc::Receiver<Job>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
    identity: (u64, u64),
    automatic_directory: Option<PathBuf>,
}

fn frame(stream: &UnixStream, limit: usize, timeout: Duration) -> io::Result<Vec<u8>> {
    let deadline = std::time::Instant::now() + timeout;
    let mut reader = BufReader::new(stream);
    let mut data = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "JSON message deadline elapsed",
            ));
        }
        stream.set_read_timeout(Some(remaining))?;
        let bytes = reader.fill_buf()?;
        let end = bytes
            .iter()
            .position(|&byte| byte == b'\n')
            .map(|at| at + 1);
        let size = end.unwrap_or(bytes.len());
        if size == 0 || size > limit.saturating_sub(data.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Expected a bounded newline-delimited JSON message",
            ));
        }
        data.extend_from_slice(&bytes[..size]);
        if end.is_some() {
            return Ok(data);
        }
        reader.consume(size);
    }
}

fn parse(bytes: &[u8]) -> Result<Call, Failure> {
    let call: Call = serde_json::from_slice(bytes)
        .map_err(|e| Failure::new("invalid_request", e.to_string()))?;
    if call.version != VERSION {
        return Err(Failure::new(
            "unsupported_protocol",
            "Use protocol version 1",
        ));
    }
    if call.args.len() > 24 || call.command.is_empty() || call.command.len() > 64 {
        return Err(Failure::new(
            "invalid_request",
            "Expected a command name and at most 24 arguments",
        ));
    }
    Ok(call)
}

impl Server {
    pub fn bind_automatic(wake: impl Fn() + Send + 'static) -> io::Result<Self> {
        let uid = fs::metadata("/proc/self")?.uid();
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .filter(|path| {
                fs::symlink_metadata(path).is_ok_and(|meta| {
                    meta.is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0
                })
            });
        let base = runtime.as_deref().unwrap_or(Path::new("/tmp"));
        // A fresh private directory prevents collisions with other instances and
        // stale filesystem entries. The timestamp distinguishes reused PIDs.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = base.join(format!("arcanum-rpc-{}-{stamp:x}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        match Self::bind(&directory.join("engine.sock"), wake) {
            Ok(mut server) => {
                server.automatic_directory = Some(directory);
                Ok(server)
            }
            Err(error) => {
                let _ = fs::remove_dir(directory);
                Err(error)
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bind(path: &Path, wake: impl Fn() + Send + 'static) -> io::Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let parent_meta = fs::metadata(&parent)?;
        let uid = fs::metadata("/proc/self")?.uid();
        if !parent_meta.is_dir() || parent_meta.uid() != uid || parent_meta.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Socket directory must be owned by the process user and private (0700)",
            ));
        }
        let path = parent.join(path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Socket filename missing")
        })?);
        let listener = UnixListener::bind(&path).map_err(|error| {
            if error.kind() == io::ErrorKind::AddrInUse {
                io::Error::new(error.kind(), "Socket path exists; use a new path or remove the stale socket after confirming its owner has exited")
            } else { error }
        })?;
        let meta = fs::symlink_metadata(&path)?;
        let identity = (meta.dev(), meta.ino());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(1);
        let mut server = Self {
            jobs: rx,
            stop: stop.clone(),
            thread: None,
            path,
            identity,
            automatic_directory: None,
        };
        fs::set_permissions(&server.path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        server.thread = Some(thread::Builder::new().name("developer-ipc".into()).spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                        let call = frame(&stream, REQUEST_BYTES, Duration::from_secs(1))
                            .map_err(|err| Failure::new("invalid_request", err.to_string()))
                            .and_then(|bytes| parse(&bytes));
                        let reply = match call {
                            Ok(call) => {
                                let (reply, result) = mpsc::sync_channel(1);
                                let id = call.id;
                                let command = call.command.clone();
                                if tx.try_send(Job { call, reply }).is_err() {
                                    Reply::result(id, &command, Err(Failure::new("busy", "Runtime request queue is unavailable")))
                                } else {
                                    wake();
                                    let mut waited = Duration::ZERO;
                                    loop {
                                        match result.recv_timeout(Duration::from_millis(100)) {
                                            Ok(reply) => break reply,
                                            Err(error) => {
                                                waited += Duration::from_millis(100);
                                                if stop.load(Ordering::Relaxed) || waited >= RESPONSE_WAIT || error == mpsc::RecvTimeoutError::Disconnected {
                                                    break Reply::result(id, &command, Err(Failure::new("timeout", "Runtime response unavailable; a dispatched operation may still have executed")));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(error) => Reply::malformed(error),
                        };
                        if let Ok(data) = serde_json::to_vec(&reply) {
                            if data.len() < RESPONSE_BYTES {
                                let _ = stream.write_all(&data).and_then(|_| stream.write_all(b"\n"));
                            } else {
                                let reply = Reply::malformed(Failure::new("response_too_large", "Use a narrower query"));
                                let _ = serde_json::to_writer(&mut stream, &reply);
                                let _ = stream.write_all(b"\n");
                            }
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::park_timeout(Duration::from_millis(20)),
                    Err(_) => break,
                }
            }
        })?);
        Ok(server)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|meta| (meta.dev(), meta.ino()) == self.identity)
        {
            let _ = fs::remove_file(&self.path);
        }
        if let Some(directory) = &self.automatic_directory {
            let _ = fs::remove_dir(directory);
        }
    }
}

pub fn call(path: &Path, request: &Call) -> Result<Value, Box<dyn std::error::Error>> {
    let mut bytes = serde_json::to_vec(request)?;
    bytes.push(b'\n');
    if bytes.len() > REQUEST_BYTES {
        return Err("Request exceeds 16 KiB".into());
    }
    // Linux returns EAGAIN immediately if a local listener's backlog is full.
    // This avoids an unbounded blocking connect before applying I/O deadlines.
    let socket = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK,
        None,
    )?;
    let address = rustix::net::SocketAddrUnix::new(path)?;
    rustix::net::connect(&socket, &address)?;
    let mut stream = UnixStream::from(socket);
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(RESPONSE_WAIT))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&bytes)?;
    let result: Value = serde_json::from_slice(&frame(&stream, RESPONSE_BYTES, RESPONSE_WAIT)?)?;
    if result["version"] != VERSION || (result["id"] != request.id && !result["id"].is_null()) {
        return Err("Unexpected response version or request ID".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_socket_handles_disconnected_clients_and_cleans_up() {
        use std::os::unix::fs::DirBuilderExt;
        let directory = std::env::temp_dir().join(format!(
            "arcanum-rpc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let path = directory.join("test.sock");
        let server = Server::bind(&path, || {}).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(Server::bind(&path, || {}).is_err());
        let mut broken = UnixStream::connect(&path).unwrap();
        broken.write_all(b"{").unwrap();
        drop(broken);
        let client = {
            let path = path.clone();
            std::thread::spawn(move || {
                call(
                    &path,
                    &Call {
                        version: 1,
                        id: 7,
                        command: "pos".into(),
                        args: vec![],
                    },
                )
                .unwrap()
            })
        };
        let job = server.jobs.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(job.call.id, 7);
        job.reply
            .send(Reply::result(
                7,
                "pos",
                Ok(Output::Data(json!({"tile":[3,4]}))),
            ))
            .unwrap();
        assert_eq!(client.join().unwrap()["result"]["tile"], json!([3, 4]));
        drop(server);
        assert!(!path.exists());
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn validation_and_envelopes() {
        assert!(parse(br#"{"version":1,"id":3,"command":"pos","args":[]}"#).is_ok());
        assert_eq!(
            parse(br#"{"version":2,"id":3,"command":"pos"}"#)
                .unwrap_err()
                .code,
            "unsupported_protocol"
        );
        assert_eq!(
            parse(br#"{"version":1,"id":3,"command":"pos","bad":[]}"#)
                .unwrap_err()
                .code,
            "invalid_request"
        );
        let reply = Reply::result(3, "capture", Ok(Output::Accepted(json!({"token":1}))));
        let value = serde_json::to_value(reply).unwrap();
        assert_eq!(value["id"], 3);
        assert_eq!(value["status"], "accepted");
        assert_eq!(value["result"]["token"], 1);
    }
}
