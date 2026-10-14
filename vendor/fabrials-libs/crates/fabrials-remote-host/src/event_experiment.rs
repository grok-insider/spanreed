use super::tests::TestChannel;
use super::*;
use fabrials_agent_host::protocol::Event;

async fn subscribe(channel: &mut TestChannel, after: u64) {
    drop(subscribe_snapshot(channel, after).await);
}

async fn subscribe_snapshot(channel: &mut TestChannel, after: u64) -> Vec<Value> {
    channel.send(json!({"id":"experiment-subscribe","method":"subscribe","body":{"afterSequence":after,"protocolVersion":4}})).await;
    let mut events = Vec::new();
    loop {
        let response = channel.receive().await;
        if response["id"] == "experiment-subscribe" {
            return events;
        }
        events.push(response);
    }
}

async fn next_sequence(channel: &mut TestChannel) -> u64 {
    loop {
        let response = channel.receive().await;
        if response
            .pointer("/event/event/kind")
            .and_then(Value::as_str)
            == Some("workspacesChanged")
        {
            return response["event"]["eventSequence"].as_u64().unwrap();
        }
    }
}

async fn close(channel: TestChannel) {
    drop(channel.input);
    channel.task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn notified_delivery_does_not_wait_for_fallback_tick() {
    let mut channel = TestChannel::with_delivery(EventDelivery::Notified).await;
    subscribe(&mut channel, 0).await;
    tokio::time::advance(Duration::from_millis(200)).await;
    tokio::task::yield_now().await;
    let before = tokio::time::Instant::now();
    let event = channel
        .state
        .emit_event(Event::WorkspacesChanged, None)
        .await;
    assert_eq!(next_sequence(&mut channel).await, event.event_sequence);
    assert_eq!(tokio::time::Instant::now(), before);
    close(channel).await;
}

#[tokio::test(start_paused = true)]
async fn subscription_snapshots_already_recorded_events_without_new_notification() {
    let mut channel = TestChannel::with_delivery(EventDelivery::Notified).await;
    let first = channel
        .state
        .emit_event(Event::WorkspacesChanged, None)
        .await;
    let second = channel
        .state
        .emit_event(Event::WorkspacesChanged, None)
        .await;
    tokio::time::advance(Duration::from_millis(200)).await;
    tokio::task::yield_now().await;
    let before = tokio::time::Instant::now();
    let frames = subscribe_snapshot(&mut channel, first.event_sequence).await;
    assert_eq!(
        frames.last().unwrap()["event"]["eventSequence"],
        second.event_sequence
    );
    assert_eq!(tokio::time::Instant::now(), before);
    close(channel).await;
}

#[tokio::test(start_paused = true)]
async fn registered_notification_covers_the_gap_after_an_empty_journal_read() {
    let channel = TestChannel::with_delivery(EventDelivery::Notified).await;
    let nonce = channel
        .state
        .pairing
        .lock()
        .await
        .mint_nonce(fabrials_agent_host::now_ms())
        .unwrap();
    let (access, _) = RemoteAccess::pair(channel.state.clone(), nonce.expose())
        .await
        .unwrap();
    let notification = access.event_notification();
    tokio::pin!(notification);
    notification.as_mut().enable();
    assert!(
        matches!(access.events(0).await.unwrap(), ReplayOutcome::Replay(events) if events.is_empty())
    );
    let event = channel
        .state
        .emit_event(Event::WorkspacesChanged, None)
        .await;
    let before = tokio::time::Instant::now();
    notification.await;
    assert_eq!(tokio::time::Instant::now(), before);
    assert!(
        matches!(access.events(0).await.unwrap(), ReplayOutcome::Replay(events) if events.len() == 1 && events[0].event_sequence == event.event_sequence)
    );
    close(channel).await;
}

#[tokio::test]
async fn notified_burst_keeps_sequence_with_a_full_slow_output_queue() {
    let mut channel = TestChannel::with_delivery(EventDelivery::Notified).await;
    subscribe(&mut channel, 0).await;
    let mut expected = Vec::new();
    for _ in 0..128 {
        expected.push(
            channel
                .state
                .emit_event(Event::WorkspacesChanged, None)
                .await
                .event_sequence,
        );
    }
    tokio::time::sleep(Duration::from_millis(30)).await;
    for sequence in expected {
        assert_eq!(next_sequence(&mut channel).await, sequence);
    }
    close(channel).await;
}

#[tokio::test(start_paused = true)]
async fn notified_revocation_closes_an_idle_unsubscribed_channel() {
    let mut channel = TestChannel::with_delivery(EventDelivery::Notified).await;
    tokio::time::advance(Duration::from_millis(200)).await;
    tokio::task::yield_now().await;
    let session_id = channel.paired["body"]["sessionId"].as_str().unwrap();
    assert!(channel.state.pairing.lock().await.revoke(session_id));
    let before = tokio::time::Instant::now();
    assert_eq!(
        channel.receive().await,
        json!({"type":"closed","reason":"revoked"})
    );
    assert!(before.elapsed() <= Duration::from_millis(200));
    drop(channel.input);
    assert!(channel.task.await.unwrap().is_err());
}

#[tokio::test(start_paused = true)]
async fn notified_future_cursor_resets_before_sending_new_events() {
    let mut channel = TestChannel::with_delivery(EventDelivery::Notified).await;
    let frames = subscribe_snapshot(&mut channel, u64::MAX).await;
    assert!(
        frames
            .iter()
            .any(|frame| frame["event"]["event"]["kind"] == "replayReset")
    );
    assert_eq!(
        frames.last().unwrap()["event"]["event"]["kind"],
        "workspacesChanged"
    );
    let next = channel
        .state
        .emit_event(Event::WorkspacesChanged, None)
        .await;
    assert_eq!(next_sequence(&mut channel).await, next.event_sequence);
    close(channel).await;
}

#[tokio::test]
async fn revocation_during_full_output_backlog_stops_before_the_rest_of_the_batch() {
    for delivery in [EventDelivery::Polling, EventDelivery::Notified] {
        let mut channel = TestChannel::with_delivery(delivery).await;
        subscribe(&mut channel, 0).await;
        for _ in 0..128 {
            channel
                .state
                .emit_event(Event::WorkspacesChanged, None)
                .await;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        let session_id = channel.paired["body"]["sessionId"].as_str().unwrap();
        assert!(channel.state.pairing.lock().await.revoke(session_id));
        let mut buffered = 0;
        loop {
            let response = channel.receive().await;
            if response["type"] == "closed" {
                assert_eq!(response["reason"], "revoked");
                break;
            }
            assert_eq!(response["event"]["event"]["kind"], "workspacesChanged");
            buffered += 1;
            assert!(
                buffered <= 17,
                "revoked access sent more than the bounded already-enqueued/in-flight frames"
            );
        }
        assert!(buffered > 0);
        drop(channel.input);
        assert_eq!(channel.task.await.unwrap(), Err("session_invalid".into()));
    }
}

#[tokio::test(start_paused = true)]
async fn notified_expired_cursor_resets_before_sending_the_next_event() {
    let mut channel = TestChannel::with_delivery(EventDelivery::Notified).await;
    for _ in 0..1200 {
        channel
            .state
            .emit_event(Event::WorkspacesChanged, None)
            .await;
    }
    let frames = subscribe_snapshot(&mut channel, 1).await;
    assert!(
        frames
            .iter()
            .any(|frame| frame["event"]["event"]["kind"] == "replayReset")
    );
    assert_eq!(
        frames.last().unwrap()["event"]["event"]["kind"],
        "workspacesChanged"
    );
    let next = channel
        .state
        .emit_event(Event::WorkspacesChanged, None)
        .await;
    assert_eq!(next_sequence(&mut channel).await, next.event_sequence);
    close(channel).await;
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn resource_sample() -> Value {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let rss = status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(") ")
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    json!({"rssKiB":rss,"userTicks":fields[11].parse::<u64>().unwrap(),"systemTicks":fields[12].parse::<u64>().unwrap(),"hostLoad":std::fs::read_to_string("/proc/loadavg").unwrap(),"hostCpu":std::fs::read_to_string("/proc/stat").unwrap().lines().next().unwrap(),"thermalMilliCelsius":std::fs::read_dir("/sys/class/thermal").ok().into_iter().flatten().filter_map(Result::ok).filter(|entry|entry.file_name().to_string_lossy().starts_with("thermal_zone")).filter_map(|entry| std::fs::read_to_string(entry.path().join("temp")).ok()).filter_map(|value|value.trim().parse::<i64>().ok()).collect::<Vec<_>>()})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "native Noise event latency experiment; requires a new evidence directory"]
async fn paired_native_event_delivery_experiment() {
    use std::io::Write;
    let directory = PathBuf::from(
        std::env::var_os("FABRIALS_REMOTE_EVENT_EXPERIMENT").expect("evidence directory"),
    );
    std::fs::create_dir(&directory).unwrap();
    let seed = 0x6843_21b7_d506_91a3_u64;
    let mut rng = seed;
    let mut orders = [false, false, false, false, true, true, true, true];
    for i in (1..orders.len()).rev() {
        let j = (next_random(&mut rng) as usize) % (i + 1);
        orders.swap(i, j);
    }
    let manifest = json!({
        "schemaVersion":1,"scope":"native journal -> managed drive_channel -> real Noise encryption -> in-process browser Noise decryption",
        "excluded":["WASM","relay WebSocket","browser rendering","OAuth","network latency"],
        "seed":seed.to_string(),"blocks":8,"warmups":50,"samples":200,
        "conditions":[{"channels":1,"slowConsumerMs":0},{"channels":10,"slowConsumerMs":40}],
        "candidateFirst":orders,"clock":"one process std::time::Instant","jitterMs":[1,30],
        "primary":"p95 of maximum elapsed among ALL consumers including the delayed consumer for each broadcast, 200 measured broadcasts per paired block",
        "gates":{"pairedMedianReductionPercent":20,"pairedMedianReductionMs":20,"bootstrap95LowerBoundPercent":0},
        "secondary":"maximum among nine non-delayed consumers and delayed consumer separately","default":"polling"
    });
    let mut metadata = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("manifest.json"))
        .unwrap();
    serde_json::to_writer_pretty(&mut metadata, &manifest).unwrap();
    metadata.sync_all().unwrap();
    let mut samples = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("samples.jsonl"))
        .unwrap();
    let mut completed = Vec::new();
    for channel_count in [1_usize, 10] {
        for (block, candidate_first) in orders.iter().copied().enumerate() {
            let variants = if candidate_first {
                [EventDelivery::Notified, EventDelivery::Polling]
            } else {
                [EventDelivery::Polling, EventDelivery::Notified]
            };
            for variant in variants {
                let first = TestChannel::with_delivery(variant).await;
                let host = first.state.clone();
                let mut channels = vec![first];
                for _ in 1..channel_count {
                    channels.push(TestChannel::with_state(host.clone(), variant).await);
                }
                for channel in &mut channels {
                    subscribe(channel, 0).await;
                }
                let start_resources = resource_sample();
                let variant_start = std::time::Instant::now();
                let mut cadence = seed ^ (block as u64) ^ ((channel_count as u64) << 32);
                for sample in 0..250 {
                    let jitter = next_random(&mut cadence) % 30 + 1;
                    tokio::time::sleep(Duration::from_millis(jitter)).await;
                    let start = std::time::Instant::now();
                    let expected = host
                        .emit_event(Event::WorkspacesChanged, None)
                        .await
                        .event_sequence;
                    let observed = futures_util::future::join_all(channels.iter_mut().enumerate().map(|(index, channel)| async move {
                        let slow = channel_count > 1 && index == channel_count - 1;
                        if slow {
                            tokio::time::sleep(Duration::from_millis(40)).await;
                        }
                        let sequence = next_sequence(channel).await;
                        assert_eq!(sequence, expected, "event lost, duplicated, or out of order");
                        json!({"channel":index,"slow":slow,"latencyMs":start.elapsed().as_secs_f64()*1000.0,"sequence":sequence})
                    })).await;
                    let record = json!({"block":block,"variant":format!("{variant:?}").to_lowercase(),"channels":channel_count,"sample":sample,"warmup":sample<50,"jitterMs":jitter,"observed":observed,"resources":resource_sample()});
                    serde_json::to_writer(&mut samples, &record).unwrap();
                    samples.write_all(b"\n").unwrap();
                }
                for channel in channels {
                    close(channel).await;
                }
                samples.sync_all().unwrap();
                let segment = json!({"block":block,"variant":format!("{variant:?}").to_lowercase(),"channels":channel_count,"elapsedSeconds":variant_start.elapsed().as_secs_f64(),"resourcesBefore":start_resources,"resourcesAfter":resource_sample(),"closed":true});
                eprintln!("completed {segment}");
                completed.push(segment);
            }
        }
    }
    let mut receipt = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("receipt.json"))
        .unwrap();
    serde_json::to_writer_pretty(
        &mut receipt,
        &json!({"status":"completed","segments":completed,"sameSequenceEveryDelivery":true}),
    )
    .unwrap();
    receipt.sync_all().unwrap();
}
