//! One browser process per installation, for an engine that runs every
//! profile in one process (`EngineInfo::profiles_in_process`).
//!
//! The first process to start takes a lock in the brand's data directory
//! and listens on a loopback port; it writes the port and a random token
//! beside the lock, in a file only the user can read. A later start
//! finds the lock taken, sends its command line to that port with the
//! token, and exits: the running browser opens the profile and the
//! addresses as its own windows and tabs. The lock is the system's
//! (`File::try_lock`), so it goes with the process however it ends, and
//! a file left by a browser that crashed claims nothing.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const LOCK_NAME: &str = "instance.lock";
const INFO_NAME: &str = "instance.toml";
/// How long either end waits for the other on the loopback connection.
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a later start waits for the first one to say where it
/// listens (it takes the lock a moment before it writes the file).
const INFO_PATIENCE: Duration = Duration::from_secs(3);
/// The longest message a start may send: a command line of addresses.
const MAX_MESSAGE: u64 = 1 << 20;

/// What a later start of the browser asks of the one already running.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relaunch {
    /// The profile named on the command line, if one was.
    pub profile: Option<String>,
    /// The addresses (or text to search for) as they were typed.
    pub urls: Vec<String>,
    /// `--restore` was given.
    pub restore: bool,
}

/// Where the running browser listens, as it wrote it down.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Info {
    port: u16,
    token: String,
}

#[derive(Serialize, Deserialize)]
struct Message {
    token: String,
    #[serde(flatten)]
    relaunch: Relaunch,
}

/// This process is the browser: it holds the lock and a port to listen
/// on. Nothing is accepted until [`Primary::serve`].
pub struct Primary {
    lock: File,
    listener: TcpListener,
    token: String,
    info_path: PathBuf,
}

/// Another process is the browser; this one can only hand over.
pub struct Running {
    info: Info,
}

pub enum Claim {
    Primary(Primary),
    Running(Running),
}

/// Take the installation's lock in `dir` (the brand's data directory),
/// or find out who has it.
pub fn claim(dir: &Path) -> io::Result<Claim> {
    std::fs::create_dir_all(dir)?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK_NAME))?;
    let info_path = dir.join(INFO_NAME);
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Ok(Claim::Running(Running {
                info: read_info(&info_path)?,
            }));
        }
        Err(std::fs::TryLockError::Error(e)) => return Err(e),
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let info = Info {
        port: listener.local_addr()?.port(),
        token: random_token()?,
    };
    write_private(
        &info_path,
        &toml::to_string(&info).map_err(io::Error::other)?,
    )?;
    Ok(Claim::Primary(Primary {
        lock,
        listener,
        token: info.token,
        info_path,
    }))
}

impl Primary {
    /// Accept later starts from now on, on a thread of its own, handing
    /// each one's request to `deliver`. The lock is held for the rest of
    /// the process's life.
    pub fn serve(self, deliver: impl Fn(Relaunch) + Send + 'static) {
        let spawned = std::thread::Builder::new()
            .name("Instance".to_string())
            .spawn(move || {
                let _lock = &self.lock;
                for stream in self.listener.incoming() {
                    match stream.and_then(|s| receive(s, &self.token)) {
                        Ok(Some(relaunch)) => deliver(relaunch),
                        Ok(None) => log::warn!("a start with the wrong token was refused"),
                        Err(e) => log::warn!("cannot read another start's request: {e}"),
                    }
                }
            });
        if let Err(e) = spawned {
            log::warn!("cannot listen for later starts: {e}");
        }
    }

    /// Where this browser listens, for a test.
    #[cfg(test)]
    fn info_path(&self) -> &Path {
        &self.info_path
    }
}

impl Drop for Primary {
    fn drop(&mut self) {
        // Only reached when `serve` was never called; a served lock
        // lives in its thread until the process ends.
        let _ = std::fs::remove_file(&self.info_path);
    }
}

impl Running {
    /// Hand `relaunch` to the running browser and wait for it to say it
    /// has it.
    pub fn forward(&self, relaunch: &Relaunch) -> io::Result<()> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.info.port));
        let mut stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        let message = Message {
            token: self.info.token.clone(),
            relaunch: relaunch.clone(),
        };
        let mut line = serde_json::to_string(&message).map_err(io::Error::other)?;
        line.push('\n');
        stream.write_all(line.as_bytes())?;
        let mut answer = String::new();
        BufReader::new(stream).read_line(&mut answer)?;
        if answer.trim() == "ok" {
            Ok(())
        } else {
            Err(io::Error::other("the running browser refused the request"))
        }
    }
}

/// One request off `stream`: `None` when its token is not ours.
fn receive(stream: TcpStream, token: &str) -> io::Result<Option<Relaunch>> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut line = String::new();
    BufReader::new(io::Read::take(&stream, MAX_MESSAGE)).read_line(&mut line)?;
    let message: Message = serde_json::from_str(&line).map_err(io::Error::other)?;
    let ours = same(message.token.as_bytes(), token.as_bytes());
    (&stream).write_all(if ours { b"ok\n" } else { b"no\n" })?;
    Ok(ours.then_some(message.relaunch))
}

/// Whether two byte strings are equal, in time that does not depend on
/// where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn read_info(path: &Path) -> io::Result<Info> {
    let deadline = Instant::now() + INFO_PATIENCE;
    loop {
        let read = std::fs::read_to_string(path)
            .and_then(|text| toml::from_str::<Info>(&text).map_err(io::Error::other));
        match read {
            Ok(info) => return Ok(info),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn random_token() -> io::Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(format!("no random numbers: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Write `text` to `path`, readable by the user alone where the system
/// has such a thing, replacing what was there in one step.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    let partial = path.with_extension("partial");
    let _ = std::fs::remove_file(&partial);
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&partial)?.write_all(text.as_bytes())?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn request() -> Relaunch {
        Relaunch {
            profile: Some("work".to_string()),
            urls: vec!["https://example.com/".to_string(), "two words".to_string()],
            restore: true,
        }
    }

    #[test]
    fn the_first_claim_is_the_browser_and_a_later_one_hands_over_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let Claim::Primary(primary) = claim(dir.path()).unwrap() else {
            panic!("the first claim is the browser");
        };
        let (sent, received) = mpsc::channel();
        primary.serve(move |relaunch| sent.send(relaunch).unwrap());
        let Claim::Running(running) = claim(dir.path()).unwrap() else {
            panic!("the lock is taken");
        };
        running.forward(&request()).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            request()
        );
        // And again: the browser keeps listening.
        running.forward(&Relaunch::default()).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            Relaunch::default()
        );
    }

    #[test]
    fn a_request_with_another_token_is_refused_and_never_delivered() {
        let dir = tempfile::tempdir().unwrap();
        let Claim::Primary(primary) = claim(dir.path()).unwrap() else {
            panic!("the first claim is the browser");
        };
        let info = read_info(primary.info_path()).unwrap();
        let (sent, received) = mpsc::channel();
        primary.serve(move |relaunch| sent.send(relaunch).unwrap());
        let stranger = Running {
            info: Info {
                port: info.port,
                token: "0".repeat(info.token.len()),
            },
        };
        assert!(stranger.forward(&request()).is_err());
        assert!(received.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn the_lock_goes_with_its_holder_and_the_file_it_left_claims_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let Claim::Primary(first) = claim(dir.path()).unwrap() else {
            panic!("the first claim is the browser");
        };
        let stale = std::fs::read_to_string(first.info_path()).unwrap();
        drop(first);
        // What a crashed browser leaves: its file, and no lock.
        std::fs::write(dir.path().join(INFO_NAME), stale).unwrap();
        assert!(matches!(claim(dir.path()).unwrap(), Claim::Primary(_)));
    }

    #[cfg(unix)]
    #[test]
    fn the_token_is_written_for_the_user_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let Claim::Primary(primary) = claim(dir.path()).unwrap() else {
            panic!("the first claim is the browser");
        };
        let mode = std::fs::metadata(primary.info_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn tokens_compare_whole() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"abcd"));
        assert!(!same(b"", b"a"));
    }
}
