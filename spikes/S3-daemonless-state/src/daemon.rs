//! The comparison point: a minimal resident daemon. Unix socket (0600), one thread per
//! connection, session state in memory behind a per-session mutex, and a single writer
//! thread draining a bounded queue into the same per-session spool format.
//!
//! Deliberately minimal — it omits what a production daemon also needs (socket
//! activation, idle exit, peer-credential checks, client/daemon version skew, restart
//! replay, back-pressure policy per record class). See FINDINGS.md for that list.

use crate::spool::{self, Record};
use crate::state::{apply, policy, SessionState};
use crate::workload::{Exec, Sample, OP_PEEK};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const OP_DECIDE: u8 = 1;
const OP_OBSERVE: u8 = 2;
const OP_PEEK_REQ: u8 = 3;
const OP_SHUTDOWN: u8 = 9;

struct Daemon {
    sessions: Mutex<HashMap<String, Arc<Mutex<SessionState>>>>,
    tx: SyncSender<(String, Vec<u8>)>,
    dropped: AtomicU64,
}

pub fn sock_path(root: &Path) -> PathBuf {
    root.join("adaged.sock")
}

pub fn serve(root: &Path) -> std::io::Result<()> {
    fs::create_dir_all(root.join("spool"))?;
    let sp = sock_path(root);
    let _ = fs::remove_file(&sp);
    let listener = UnixListener::bind(&sp)?;
    fs::set_permissions(&sp, fs::Permissions::from_mode(0o600))?;
    let (tx, rx) = sync_channel::<(String, Vec<u8>)>(4096);
    let spool_root = root.to_path_buf();
    std::thread::spawn(move || {
        let mut files: HashMap<String, File> = HashMap::new();
        for (s, frame) in rx {
            let f = files.entry(s.clone()).or_insert_with(|| {
                OpenOptions::new().append(true).create(true).open(spool::spool_path(&spool_root, &s)).unwrap()
            });
            f.write_all(&frame).unwrap();
        }
    });
    let d = Arc::new(Daemon { sessions: Mutex::new(HashMap::new()), tx, dropped: AtomicU64::new(0) });
    for conn in listener.incoming() {
        let mut c = match conn { Ok(c) => c, Err(_) => continue };
        let d = d.clone();
        std::thread::spawn(move || {
            let mut len = [0u8; 4];
            if c.read_exact(&mut len).is_err() { return; }
            let mut buf = vec![0u8; u32::from_le_bytes(len) as usize];
            if c.read_exact(&mut buf).is_err() { return; }
            let t0 = Instant::now();
            let op = buf[0];
            if op == OP_SHUTDOWN {
                eprintln!("daemon: dropped={}", d.dropped.load(Ordering::Relaxed));
                let _ = c.write_all(&[0u8; 17]);
                std::process::exit(0);
            }
            let sl = buf[1] as usize;
            let session = String::from_utf8_lossy(&buf[2..2 + sl]).to_string();
            let st = d.sessions.lock().unwrap().entry(session.clone()).or_default().clone();
            let mut directive = 0u64;
            match op {
                OP_DECIDE => {
                    let mut ev = Record::decode(&buf[2 + sl..]).unwrap();
                    let mut st = st.lock().unwrap();
                    ev.dseq = st.decision_seq + 1;
                    ev.directive = policy(&st, &ev);
                    directive = ev.directive;
                    apply(&mut st, &ev);
                    // decisions are never dropped (F6): blocking send
                    d.tx.send((session, spool::frame(&ev.encode()))).unwrap();
                }
                OP_OBSERVE => {
                    let ev = Record::decode(&buf[2 + sl..]).unwrap();
                    apply(&mut st.lock().unwrap(), &ev);
                    if let Err(TrySendError::Full(_)) = d.tx.try_send((session, spool::frame(&ev.encode()))) {
                        d.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
                OP_PEEK_REQ => {
                    let _snapshot = st.lock().unwrap().clone();
                }
                _ => {}
            }
            let ns = t0.elapsed().as_nanos() as u64;
            let mut reply = [0u8; 17];
            reply[1..9].copy_from_slice(&ns.to_le_bytes());
            reply[9..17].copy_from_slice(&directive.to_le_bytes());
            let _ = c.write_all(&reply);
        });
    }
    Ok(())
}

pub struct DaemonExec {
    pub sock: PathBuf,
}
impl DaemonExec {
    fn call(&self, op: u8, s: &str, r: Option<&Record>) -> (u64, u64) {
        let t0 = Instant::now();
        let mut c = UnixStream::connect(&self.sock).expect("connect");
        let mut body = vec![op, s.len() as u8];
        body.extend_from_slice(s.as_bytes());
        if let Some(r) = r { body.extend(r.encode()); }
        let mut msg = (body.len() as u32).to_le_bytes().to_vec();
        msg.extend(body);
        c.write_all(&msg).unwrap();
        let mut reply = [0u8; 17];
        c.read_exact(&mut reply).unwrap();
        (t0.elapsed().as_nanos() as u64, u64::from_le_bytes(reply[1..9].try_into().unwrap()))
    }
    pub fn shutdown(&self) {
        if let Ok(mut c) = UnixStream::connect(&self.sock) {
            let _ = c.write_all(&[1, 0, 0, 0, OP_SHUTDOWN]);
            let mut reply = [0u8; 17];
            let _ = c.read_exact(&mut reply);
        }
    }
}
impl Exec for DaemonExec {
    fn decide(&self, s: &str, r: Record) -> Sample {
        let (wall, server) = self.call(OP_DECIDE, s, Some(&r));
        Sample { op: r.kind, decide: true, total_ns: wall, access_ns: server, ..Default::default() }
    }
    fn observe(&self, s: &str, r: &Record) -> Sample {
        let (wall, server) = self.call(OP_OBSERVE, s, Some(r));
        Sample { op: r.kind, decide: false, total_ns: wall, access_ns: server, ..Default::default() }
    }
    fn peek(&self, s: &str) -> Option<Sample> {
        let (wall, server) = self.call(OP_PEEK_REQ, s, None);
        Some(Sample { op: OP_PEEK, decide: false, total_ns: wall, access_ns: server, ..Default::default() })
    }
}
