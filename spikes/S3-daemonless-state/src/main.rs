//! adage-s3 — CLI for spike S3. See README section of FINDINGS.md / run.sh.

use s3_daemonless_state::checks::{self, gen_case};
use s3_daemonless_state::daemon::{self, DaemonExec};
use s3_daemonless_state::exec::{FileExec, ProcExec};
use s3_daemonless_state::spool::{self, kind_name, Record};
use s3_daemonless_state::state::{C_CORRUPT, C_LATE};
use s3_daemonless_state::store::{Backend, Durability, Store};
use s3_daemonless_state::util::{from_hex, loadavg, pct, Rng};
use s3_daemonless_state::workload::{drive, script, DriveOpts, Exec, Sample, OP_PEEK};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn arg<'a>(args: &'a [String], name: &str, default: &'a str) -> &'a str {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(|s| s.as_str()).unwrap_or(default)
}

fn spike_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if std::env::var("S3_SERIAL_OBS").is_ok() {
        s3_daemonless_state::workload::SERIAL_OBS.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    spool::START.get_or_init(Instant::now);
    if std::env::var("S3_TRACE_SLOW").is_ok() {
        spool::TRACE_SLOW.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    match args.get(1).map(|s| s.as_str()) {
        Some("hook") => hook(&args[2..]),
        Some("daemon") => daemon::serve(Path::new(&args[2])).unwrap(),
        Some("spool-writer") => spool_writer(&args[2..]),
        Some("crash-state-child") => checks::crash_child_loop(Path::new(&args[2]), Backend::parse(&args[3]), &args[4]),
        Some("crash-decide-child") => checks::crash_decide_child(Path::new(&args[2]), Backend::parse(&args[3]), &args[4], args[5].parse().unwrap()),
        Some("bench") => bench(&args[2..]),
        Some("correctness") => correctness(&args[2..]),
        _ => {
            eprintln!("usage: adage-s3 bench|correctness|hook|daemon|spool-writer|crash-state-child …");
            std::process::exit(2);
        }
    }
}

/// One hook invocation as its own process. Prints: total access lock fold folded (ns).
fn hook(a: &[String]) {
    let store = Store::new(Path::new(&a[0]), Backend::parse(&a[1]), Durability::parse(&a[2]));
    let session = &a[3];
    let rec = from_hex(&a[5]).and_then(|b| Record::decode(&b));
    let t = match a[4].as_str() {
        "decide" => store.decide(session, rec.unwrap()).unwrap().1,
        "observe" => store.observe(session, &rec.unwrap()).unwrap(),
        "peek" => store.peek(session).unwrap().1,
        m => panic!("mode {m}"),
    };
    println!("{} {} {} {} {}", t.total_ns, t.state_access_ns(), t.lock_ns, t.fold_ns, t.folded);
}

/// Child for the concurrency/crash tests: append n records (n = 0: forever).
fn spool_writer(a: &[String]) {
    let path = Path::new(&a[0]);
    let wid: u32 = a[1].parse().unwrap();
    let n: u32 = a[2].parse().unwrap();
    let max: usize = a[3].parse().unwrap();
    let mut rng = Rng::new(a[4].parse::<u64>().unwrap() ^ wid as u64);
    let mut i = 0u32;
    while n == 0 || i < n {
        let fr = checks::test_frame(wid, i, rng.below(max as u64 + 1) as usize, &mut rng);
        match spool::append_frame(path, &fr).unwrap() {
            spool::Append::Ok => {}
            spool::Append::Short(k) => panic!("short write {k}"),
        }
        i += 1;
    }
}

// ============================================================================ bench

#[derive(Default)]
struct Agg {
    total: Vec<u64>,
    access: Vec<u64>,
    lock: Vec<u64>,
    fold: Vec<u64>,
    wall: Vec<u64>,
    folded: Vec<u64>,
}
impl Agg {
    fn push(&mut self, s: &Sample) {
        self.total.push(s.total_ns);
        self.access.push(s.access_ns);
        self.lock.push(s.lock_ns);
        self.fold.push(s.fold_ns);
        if s.wall_ns > 0 { self.wall.push(s.wall_ns) }
        self.folded.push(s.folded as u64);
    }
    fn json(&mut self) -> String {
        for v in [&mut self.total, &mut self.access, &mut self.lock, &mut self.fold, &mut self.wall, &mut self.folded] {
            v.sort_unstable();
        }
        let us = |v: &[u64], p: f64| format!("{:.1}", pct(v, p) as f64 / 1000.0);
        let mut s = format!(
            "{{\"n\":{},\"total_us\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}},\"access_us\":{{\"p50\":{},\"p95\":{},\"p99\":{}}},\"lock_us\":{{\"p50\":{},\"p95\":{},\"p99\":{}}},\"fold_us\":{{\"p50\":{},\"p95\":{}}},\"folded\":{{\"p50\":{},\"p95\":{},\"max\":{}}}",
            self.total.len(),
            us(&self.total, 50.0), us(&self.total, 95.0), us(&self.total, 99.0), us(&self.total, 100.0),
            us(&self.access, 50.0), us(&self.access, 95.0), us(&self.access, 99.0),
            us(&self.lock, 50.0), us(&self.lock, 95.0), us(&self.lock, 99.0),
            us(&self.fold, 50.0), us(&self.fold, 95.0),
            pct(&self.folded, 50.0), pct(&self.folded, 95.0), pct(&self.folded, 100.0),
        );
        if !self.wall.is_empty() {
            s += &format!(",\"wall_us\":{{\"p50\":{},\"p95\":{},\"p99\":{}}}", us(&self.wall, 50.0), us(&self.wall, 95.0), us(&self.wall, 99.0));
        }
        s + "}"
    }
}

fn bench(a: &[String]) {
    let exec_kind = arg(a, "--exec", "file").to_string();
    let backend = Backend::parse(arg(a, "--backend", "slots"));
    let durability_s = arg(a, "--durability", "none").to_string();
    let durability = Durability::parse(&durability_s);
    let sessions: usize = arg(a, "--sessions", "1").parse().unwrap();
    let turns: usize = arg(a, "--turns", "32").parse().unwrap();
    let reps: usize = arg(a, "--reps", "3").parse().unwrap();
    let seed: u64 = arg(a, "--seed", "1").parse().unwrap();
    let overlap = arg(a, "--overlap", "1") == "1";
    let lag: u64 = arg(a, "--lag-us", "300").parse().unwrap();
    let pace_us: u64 = arg(a, "--pace-us", "0").parse().unwrap();
    let label = arg(a, "--label", "bench").to_string();
    let out_dir = PathBuf::from(arg(a, "--out-dir", spike_dir().join("results").to_str().unwrap()));
    fs::create_dir_all(&out_dir).unwrap();
    let exe = std::env::current_exe().unwrap();
    // Relative paths keep the daemon's socket path under the 104-byte sun_path limit.
    std::env::set_current_dir(spike_dir()).unwrap();

    let load_before = loadavg();
    let mut groups: BTreeMap<String, Agg> = BTreeMap::new();
    let mut recon_ok = 0;
    let mut recon_total = 0;
    let mut late = 0u64;
    let mut corrupt = 0u64;
    let mut disk = String::new();
    let wall0 = Instant::now();
    let mut total_events = 0usize;
    let mut drive_s = 0f64;

    // Deleting the previous run's tree and immediately recreating files in it produced
    // 50–480 ms APFS stalls in open()/write() across all threads (a harness artifact —
    // see FINDINGS). So: clear once, let the filesystem settle, and give every rep its
    // own fresh directory.
    let base = std::env::var("S3_ROOT_BASE").unwrap_or_else(|_| "target/bench".into());
    let label_dir = Path::new(&base).join(&label);
    let _ = fs::remove_dir_all(&label_dir);
    let settle: u64 = std::env::var("S3_SETTLE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    std::thread::sleep(Duration::from_millis(settle));
    for rep in 0..reps {
        let root = checks::fresh_dir(&label_dir.join(format!("r{rep}")));
        let scripts: Vec<_> = (0..sessions).map(|i| script(seed + rep as u64 * 1000, i, turns)).collect();
        let mut daemon_child = None;
        let exec: Box<dyn Exec> = match exec_kind.as_str() {
            "file" => Box::new(FileExec(Store::new(&root, backend, durability))),
            "proc" => {
                let _ = Store::new(&root, backend, durability);
                Box::new(ProcExec {
                    exe: exe.clone(),
                    root: root.clone(),
                    backend: backend.name(),
                    durability: Box::leak(durability_s.clone().into_boxed_str()),
                })
            }
            "daemon" => {
                let child = Command::new(&exe).arg("daemon").arg(&root).stderr(Stdio::inherit()).spawn().unwrap();
                let sock = daemon::sock_path(&root);
                let t0 = Instant::now();
                while std::os::unix::net::UnixStream::connect(&sock).is_err() {
                    assert!(t0.elapsed() < Duration::from_secs(5), "daemon did not start");
                    std::thread::sleep(Duration::from_millis(5));
                }
                daemon_child = Some(child);
                Box::new(DaemonExec { sock })
            }
            k => panic!("exec {k}"),
        };
        let opts = DriveOpts { overlap, max_lag_us: lag, peek_per_turn: true, pace_us };
        let t_drive = Instant::now();
        let samples: Vec<Vec<Sample>> = std::thread::scope(|sc| {
            let hs: Vec<_> = scripts
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let exec = &*exec;
                    let opts = &opts;
                    sc.spawn(move || drive(exec, s, seed ^ (i as u64 * 7 + rep as u64), opts))
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        drive_s += t_drive.elapsed().as_secs_f64();
        total_events += scripts.iter().map(|s| s.events).sum::<usize>();
        if let Some(mut c) = daemon_child {
            DaemonExec { sock: daemon::sock_path(&root) }.shutdown();
            let _ = c.wait();
        }
        let slow_us: u64 = std::env::var("S3_SLOW_US").ok().and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
        for s in samples.iter().flatten() {
            if s.total_ns / 1000 > slow_us {
                eprintln!("slow {} decide={} total={}us lock={}us access={}us fold={}us folded={}", kind_name(s.op), s.decide, s.total_ns / 1000, s.lock_ns / 1000, s.access_ns / 1000, s.fold_ns / 1000, s.folded);
            }
            let name = if s.op == OP_PEEK { "peek".to_string() } else if s.decide { format!("decide:{}", kind_name(s.op)) } else { format!("observe:{}", kind_name(s.op)) };
            groups.entry(name).or_default().push(s);
            if s.op != OP_PEEK {
                groups.entry(if s.decide { "decide:ALL".into() } else { "observe:ALL".into() }).or_default().push(s);
            }
        }
        // correctness on the real concurrent spools: deterministic reconstruction
        let mut spool_bytes = 0u64;
        let mut json_bytes = 0usize;
        let mut raw_bytes = 0usize;
        let mut events = 0usize;
        let mut state_bytes = 0u64;
        let mut rebuild_ns = Vec::new();
        for sc in &scripts {
            let p = spool::spool_path(&root, &sc.session);
            let (recs, scan) = spool::read_all(&p).unwrap();
            let (order, dup, missing) = spool::reconstruct(&recs);
            recon_total += 1;
            if order == sc.canonical && dup == 0 && missing == 0 && scan.corrupt.is_empty() {
                recon_ok += 1;
            } else {
                eprintln!("reconstruct mismatch in {} (dup {dup}, missing {missing}, corrupt {})", sc.session, scan.corrupt.len());
            }
            spool_bytes += fs::metadata(&p).unwrap().len();
            json_bytes += recs.iter().map(|r| r.json_len()).sum::<usize>();
            raw_bytes += recs.iter().map(|r| r.raw.len()).sum::<usize>();
            events += recs.len();
            if exec_kind != "daemon" {
                let st_store = Store::new(&root, backend, durability);
                let (st, _) = st_store.peek(&sc.session).unwrap();
                if let Some(st) = st { late += st.c[C_LATE]; corrupt += st.c[C_CORRUPT]; state_bytes += st.encode().len() as u64; }
                let t0 = Instant::now();
                let _ = st_store.rebuild(&sc.session).unwrap();
                rebuild_ns.push(t0.elapsed().as_nanos() as u64);
            }
        }
        if rep == 0 {
            rebuild_ns.sort_unstable();
            let per_k = |b: f64| b / events as f64 * 1000.0 / 1024.0;
            disk = format!(
                "{{\"sessions\":{sessions},\"events\":{events},\"events_per_session\":{:.0},\"spool_bytes\":{spool_bytes},\"spool_kib_per_1000_events\":{:.1},\"json_kib_per_1000_events\":{:.1},\"raw_text_kib_per_1000_events\":{:.1},\"state_image_bytes_per_session\":{},\"rebuild_from_spool_us\":{{\"p50\":{:.1},\"max\":{:.1}}}}}",
                events as f64 / sessions as f64,
                per_k(spool_bytes as f64),
                per_k(json_bytes as f64),
                per_k(raw_bytes as f64),
                state_bytes / sessions.max(1) as u64,
                pct(&rebuild_ns, 50.0) as f64 / 1000.0,
                pct(&rebuild_ns, 100.0) as f64 / 1000.0,
            );
        }
    }
    let load_after = loadavg();
    let mut body = String::new();
    for (k, v) in groups.iter_mut() {
        if !body.is_empty() { body += ",\n    " }
        body += &format!("\"{k}\": {}", v.json());
    }
    let json = format!(
        "{{\n  \"label\": \"{label}\",\n  \"exec\": \"{exec_kind}\",\n  \"backend\": \"{}\",\n  \"durability\": \"{durability_s}\",\n  \"sessions\": {sessions},\n  \"turns\": {turns},\n  \"reps\": {reps},\n  \"parallel_tools_per_batch\": 8,\n  \"overlap\": {overlap},\n  \"max_lag_us\": {lag},\n  \"pace_us\": {pace_us},\n  \"root_base\": \"{}\",\n  \"loadavg_before\": \"{load_before}\",\n  \"loadavg_after\": \"{load_after}\",\n  \"elapsed_s\": {:.2},\n  \"events_per_s\": {:.0},\n  \"reconstruct_ok\": \"{recon_ok}/{recon_total}\",\n  \"late_observations_folded\": {late},\n  \"corrupt_regions\": {corrupt},\n  \"disk\": {disk},\n  \"ops\": {{\n    {body}\n  }}\n}}\n",
        if exec_kind == "daemon" { "memory" } else { backend.name() },
        std::env::var("S3_ROOT_BASE").unwrap_or_else(|_| "target/bench (in repo, iCloud-synced Documents)".into()),
        wall0.elapsed().as_secs_f64(),
        total_events as f64 / drive_s
    );
    fs::write(out_dir.join(format!("bench-{label}.json")), &json).unwrap();
    let d = groups.get_mut("decide:ALL").unwrap();
    d.total.sort_unstable();
    d.access.sort_unstable();
    let o = groups.get_mut("observe:ALL").unwrap();
    o.total.sort_unstable();
    println!(
        "{label}: decide total p50/p95/p99 = {:.0}/{:.0}/{:.0} us, access p95 = {:.0} us, observe p95 = {:.0} us, reconstruct {recon_ok}/{recon_total}, load {load_before} -> {load_after}",
        pct(&groups["decide:ALL"].total, 50.0) as f64 / 1e3,
        pct(&groups["decide:ALL"].total, 95.0) as f64 / 1e3,
        pct(&groups["decide:ALL"].total, 99.0) as f64 / 1e3,
        pct(&groups["decide:ALL"].access, 95.0) as f64 / 1e3,
        pct(&groups["observe:ALL"].total, 95.0) as f64 / 1e3,
    );
}

// ============================================================================ correctness

fn correctness(a: &[String]) {
    let full = arg(a, "--scale", "full") == "full";
    let exe = std::env::current_exe().unwrap();
    let dir = checks::fresh_dir(&spike_dir().join("target/correctness"));
    let mut out = Vec::new();
    let load_before = loadavg();
    let t0 = Instant::now();

    // 1. concurrent writers — threads and processes
    let n_cases = if full { 200 } else { 20 };
    let mut ok_t = 0;
    let mut recs_t = 0;
    let mut by_size: HashMap<usize, (usize, usize)> = HashMap::new();
    for seed in 0..n_cases {
        let c = gen_case(seed);
        match checks::spool_threads(&dir, &c) {
            Ok(n) => { ok_t += 1; recs_t += n; let e = by_size.entry(c.max_size).or_default(); e.0 += 1; e.1 += 1; }
            Err(e) => { eprintln!("threads case {seed}: {e}"); by_size.entry(c.max_size).or_default().0 += 1; }
        }
    }
    let p_cases = if full { 60 } else { 8 };
    let mut ok_p = 0;
    let mut recs_p = 0;
    for seed in 0..p_cases {
        let c = gen_case(10_000 + seed);
        match checks::spool_procs(&exe, &dir, &c) {
            Ok(n) => { ok_p += 1; recs_p += n; }
            Err(e) => eprintln!("procs case {seed}: {e}"),
        }
    }
    let mut sizes: Vec<_> = by_size.into_iter().collect();
    sizes.sort();
    let sizes: Vec<String> = sizes.iter().map(|(s, (n, ok))| format!("\"{s}\":\"{ok}/{n}\"")).collect();
    out.push(format!(
        "\"concurrent_writers\": {{\"thread_cases_ok\":\"{ok_t}/{n_cases}\",\"thread_records\":{recs_t},\"by_max_record_bytes\":{{{}}},\"process_cases_ok\":\"{ok_p}/{p_cases}\",\"process_records\":{recs_p}}}",
        sizes.join(",")
    ));

    // 2. ordering + convergence
    let oc = if full { 2000 } else { 100 };
    let o0 = checks::ordering(0..oc, 12, 0, 0.0);
    let o1 = checks::ordering(0..oc, 12, 1, 0.0);
    let o1d = checks::ordering(0..oc, 12, 1, 0.02);
    let o5 = checks::ordering(0..oc, 12, 5, 0.0);
    let ord = |r: &checks::OrderingReport| format!(
        "{{\"cases\":{},\"reconstruct_ok\":{},\"converge_ok\":{},\"late_folds\":{},\"dups_injected\":{},\"dups_detected\":{}}}",
        r.cases, r.reconstruct_ok, r.converge_ok, r.late_folds, r.dups_injected, r.dups_detected
    );
    out.push(format!(
        "\"ordering\": {{\"lag0\":{},\"lag1\":{},\"lag1_dup2pct\":{},\"lag5\":{}}}",
        ord(&o0), ord(&o1), ord(&o1d), ord(&o5)
    ));

    // 3. crash safety
    let it = if full { 1000 } else { 60 };
    let mut crash = Vec::new();
    for b in [Backend::Rename, Backend::Slots, Backend::UnsafeInPlace] {
        let r = checks::crash_state(&exe, &dir, b, it, 2, 7);
        crash.push(format!(
            "\"{}\":{{\"iterations\":{},\"kills\":{},\"progressed\":{},\"torn\":{},\"regressed\":{},\"stale_tmp\":{},\"final_value\":{},\"lock_free_after\":{}}}",
            r.backend, r.iterations, r.kills, r.progressed, r.torn, r.regressed, r.stale_tmp, r.final_value, r.lock_free_after
        ));
    }
    let mut cd = Vec::new();
    for b in [Backend::Rename, Backend::Slots] {
        let r = checks::crash_decide(&exe, &dir, b, if full { 500 } else { 40 }, 21);
        cd.push(format!(
            "\"{}\":{{\"iterations\":{},\"kills\":{},\"decisions\":{},\"dseq_contiguous\":{},\"replayed_after_kill\":{},\"state_equals_rebuild\":{},\"corrupt_regions\":{},\"order_slips\":{}}}",
            r.backend, r.iterations, r.kills, r.decisions, r.dseq_contiguous, r.replays, r.state_equals_rebuild, r.corrupt_regions, r.order_slips
        ));
    }
    out.push(format!("\"crash_decide\": {{{}}}", cd.join(",")));
    let sc = checks::crash_spool(&exe, &dir, if full { 300 } else { 30 }, 4, 480, 11);
    let sc_big = checks::crash_spool(&exe, &dir.join("big"), if full { 150 } else { 15 }, 4, 65_000, 12);
    let spc = |r: &checks::SpoolCrashReport| format!(
        "{{\"iterations\":{},\"writers_killed\":{},\"records\":{},\"corrupt_regions\":{},\"gaps_or_dups\":{},\"tail_pending\":{}}}",
        r.iterations, r.writers_killed, r.records, r.corrupt_regions, r.gaps_or_dups, r.tail_pending
    );
    out.push(format!("\"crash_state\": {{{}}}", crash.join(",")));
    out.push(format!("\"crash_spool\": {{\"records_le_480B\":{},\"records_le_65KB\":{}}}", spc(&sc), spc(&sc_big)));

    // 4. torn-frame injection
    let inj = checks::inject(0..if full { 2000 } else { 100 });
    out.push(format!(
        "\"torn_injection\": {{\"cases\":{},\"ok\":{},\"damaged_frames\":{},\"recovered\":{},\"false_records\":{},\"coincidental_repairs\":{}}}",
        inj.cases, inj.ok, inj.damaged, inj.recovered, inj.false_records, inj.coincidental_repairs
    ));

    // 5. learn-style reader concurrent with writers
    let mut rr = Vec::new();
    for (i, (w, n, m)) in [(8u32, 500u32, 480usize), (32, 200, 480), (16, 100, 16_000), (8, 40, 65_000)].iter().enumerate() {
        let r = checks::reader_prefix(&exe, &dir, *w, *n, *m, 100 + i as u64);
        rr.push(format!(
            "{{\"writers\":{},\"max_record_bytes\":{m},\"records\":{},\"polls\":{},\"pending_tail_seen\":{},\"corrupt_seen\":{},\"prefix_ok\":{}}}",
            r.writers, r.records, r.polls, r.pending_tail_seen, r.corrupt_seen, r.prefix_ok
        ));
    }
    out.push(format!("\"reader_prefix\": [{}]", rr.join(",")));

    let json = format!(
        "{{\n  \"scale\": \"{}\",\n  \"loadavg_before\": \"{load_before}\",\n  \"loadavg_after\": \"{}\",\n  \"elapsed_s\": {:.1},\n  {}\n}}\n",
        if full { "full" } else { "small" },
        loadavg(),
        t0.elapsed().as_secs_f64(),
        out.join(",\n  ")
    );
    let path = spike_dir().join("results").join(format!("correctness-{}.json", if full { "full" } else { "small" }));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = OpenOptions::new().write(true).create(true).truncate(true).open(&path).unwrap();
    use std::io::Write;
    f.write_all(json.as_bytes()).unwrap();
    print!("{json}");
}
