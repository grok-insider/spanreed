use super::*;

fn cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(") ")
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
}

fn cpu_runtime_ns() -> (u64, Vec<String>) {
    let mut tasks: Vec<_> = std::fs::read_dir("/proc/self/task")
        .unwrap()
        .map(Result::unwrap)
        .collect();
    tasks.sort_by_key(|entry| entry.file_name());
    let names = tasks
        .iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let runtime = tasks
        .iter()
        .map(|entry| {
            std::fs::read_to_string(entry.path().join("schedstat"))
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        })
        .sum();
    (runtime, names)
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}

fn fixture(home: &Path, session_id: &str) -> Grant {
    let workspace = PathBuf::from("/workspace");
    let source = home
        .join("sessions")
        .join(fabrials_agent_host::session_catalog::encode_cwd_dirname(
            &workspace.to_string_lossy(),
        ))
        .join(session_id);
    std::fs::create_dir_all(&source).unwrap();
    let mut file = std::fs::File::create(source.join("updates.jsonl")).unwrap();
    for index in 0..1000 {
        let kind = if index % 2 == 0 {
            "user_message_chunk"
        } else {
            "agent_message_chunk"
        };
        writeln!(file,"{}",json!({"update":{"sessionUpdate":kind,"content":{"type":"text","text":format!("{index}: {}", "synthetic prose with escaped \"quotes\" and Unicode áß💎. ".repeat(4))}}})).unwrap();
    }
    file.sync_all().unwrap();
    let document = export(home, &workspace, session_id, "Synthetic session").unwrap();
    Grant {
        session_id: session_id.into(),
        generation: 1,
        upload_token: "synthetic-never-used".into(),
        workspace,
        title: "Synthetic session".into(),
        revision: 1,
        fingerprint: format!("{:x}", Sha256::digest(ensure_document(&document).unwrap())),
        stopped: false,
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "publication CPU experiment; requires a new evidence directory"]
async fn paired_publication_content_digest_experiment() {
    let directory = PathBuf::from(
        std::env::var_os("FABRIALS_REMOTE_PUBLICATION_EXPERIMENT").expect("evidence directory"),
    );
    std::fs::create_dir(&directory).unwrap();
    let home = tempfile::tempdir().unwrap();
    let grants: Vec<_> = (0..10)
        .map(|index| {
            (
                format!("share-{index}"),
                fixture(home.path(), &format!("session-{index}")),
            )
        })
        .collect();
    let seed = 0x35ea_b082_7946_d1c3_u64;
    let mut rng = seed;
    let mut orders = [false, false, false, false, true, true, true, true];
    for index in (1..orders.len()).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        orders.swap(index, rng as usize % (index + 1));
    }
    let manifest = json!({"schemaVersion":1,"scope":"native publication sync preparation with unchanged already-published synthetic documents","excluded":["HTTP uploads","S3","revocation service","three-second scheduler"],"seed":seed.to_string(),"blocks":8,"warmups":50,"samples":200,"publications":10,"entriesPerPublication":1000,"candidateFirst":orders,"cpuClock":"sum of kernel per-thread execution runtime nanoseconds from /proc/self/task/*/schedstat, stable thread membership checked","wallClock":"std::time::Instant","gates":{"cpuReductionPercent":30,"changedDocumentDelayRegressionMs":250,"nonTargetMemoryRegressionPercent":10},"bytesReadClaim":"no reduction; both variants read the entire source"});
    let mut metadata = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("manifest.json"))
        .unwrap();
    serde_json::to_writer_pretty(&mut metadata, &manifest).unwrap();
    metadata.sync_all().unwrap();
    let mut records = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("samples.jsonl"))
        .unwrap();
    let mut segments = Vec::new();
    for (block, candidate_first) in orders.into_iter().enumerate() {
        let variants = if candidate_first {
            [ChangeDetection::ContentDigest, ChangeDetection::FullExport]
        } else {
            [ChangeDetection::FullExport, ChangeDetection::ContentDigest]
        };
        for variant in variants {
            let state = Arc::new(HostState::new(
                fabrials_agent_host::origin::LocalOrigin::new(
                    "0123456789abcdef0123456789abcdef",
                    21001,
                )
                .unwrap(),
            ));
            let client = reqwest::Client::builder()
                .https_only(true)
                .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
                .timeout(std::time::Duration::from_millis(10))
                .build()
                .unwrap();
            let mut publisher = Shares::new(state, home.path().into(), client).unwrap();
            publisher.change_detection = variant;
            *publisher.grants.lock().await = grants.iter().cloned().collect();
            let mut measured_ticks = 0;
            let mut measured_ns = 0;
            let mut durations = Vec::new();
            let mut rss = Vec::new();
            for sample in 0..250 {
                let before = cpu_ticks();
                let (cpu_before, tasks_before) = cpu_runtime_ns();
                let start = std::time::Instant::now();
                for (share, grant) in &grants {
                    publisher
                        .sync_one_at(home.path(), share, grant)
                        .await
                        .unwrap();
                }
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let (cpu_after, tasks_after) = cpu_runtime_ns();
                assert_eq!(
                    tasks_before, tasks_after,
                    "thread membership changed during CPU sample"
                );
                let cpu_ns = cpu_after
                    .checked_sub(cpu_before)
                    .expect("CPU runtime is monotonic");
                let ticks = cpu_ticks() - before;
                let memory = rss_kib();
                if sample >= 50 {
                    measured_ticks += ticks;
                    measured_ns += cpu_ns;
                    durations.push(elapsed);
                    rss.push(memory);
                }
                let record = json!({"block":block,"variant":format!("{variant:?}").to_lowercase(),"sample":sample,"warmup":sample<50,"elapsedMs":elapsed,"cpuTicks":ticks,"cpuNanoseconds":cpu_ns,"rssKiB":memory,"hostLoad":std::fs::read_to_string("/proc/loadavg").unwrap()});
                serde_json::to_writer(&mut records, &record).unwrap();
                records.write_all(b"\n").unwrap();
            }
            assert!(
                publisher
                    .grants
                    .lock()
                    .await
                    .values()
                    .all(|grant| grant.revision == 1 && !grant.stopped)
            );
            let (share, grant) = &grants[0];
            let before = std::time::Instant::now();
            let unseen = publisher
                .changed_document(home.path(), "uncached-change", grant)
                .await
                .unwrap()
                .unwrap()
                .0;
            let changed_delay = before.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(
                unseen,
                export(
                    home.path(),
                    &grant.workspace,
                    &grant.session_id,
                    &grant.title
                )
                .unwrap()
            );
            assert!(
                publisher
                    .source_fingerprints
                    .lock()
                    .await
                    .get(share)
                    .is_some()
                    == (variant == ChangeDetection::ContentDigest)
            );
            records.sync_all().unwrap();
            let segment = json!({"block":block,"variant":format!("{variant:?}").to_lowercase(),"measuredCpuTicks":measured_ticks,"measuredCpuNanoseconds":measured_ns,"elapsedMs":durations,"rssKiB":rss,"uncachedDocumentMs":changed_delay,"unchangedRevisions":true,"documentIdentity":true});
            eprintln!(
                "completed publication block {block} {variant:?}: {measured_ticks} CPU ticks"
            );
            segments.push(segment);
        }
    }
    home.close().unwrap();
    let mut receipt = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("receipt.json"))
        .unwrap();
    serde_json::to_writer_pretty(
        &mut receipt,
        &json!({"status":"completed","segments":segments,"temporarySourcesRemoved":true}),
    )
    .unwrap();
    receipt.sync_all().unwrap();
}
