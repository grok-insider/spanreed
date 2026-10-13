use super::*;

struct FixtureCredentials(Zeroizing<String>);

#[async_trait::async_trait]
impl Credentials for FixtureCredentials {
    async fn access_token(&self) -> Result<Zeroizing<String>, String> {
        Ok(self.0.clone())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the managed loopback acceptance driver"]
async fn managed_network_fixture() {
    let directory =
        PathBuf::from(std::env::var_os("FABRIALS_REMOTE_HOST_QA").expect("fixture directory"));
    let input: Value =
        serde_json::from_slice(&std::fs::read(directory.join("host-input.json")).unwrap()).unwrap();
    let url = input["relayUrl"].as_str().unwrap();
    let parsed = reqwest::Url::parse(url).unwrap();
    assert_eq!(parsed.scheme(), "ws");
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    let config = if directory.join("remote.json").exists() {
        state::load(&directory).unwrap()
    } else {
        let identity = fabrials_remote_noise::identity().unwrap();
        let config = state::Config {
            enabled: true,
            host_id: "qa-host".into(),
            public_key: state::hex(&identity.public),
            private_key: identity.private.to_vec(),
            label: "Managed acceptance".into(),
        };
        state::save(&directory, &config).unwrap();
        config
    };
    let host = Arc::new(
        HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        )
        .with_agent_mode(fabrials_agent_host::acp::AgentMode::Shared)
        .with_remote_experiments(fabrials_agent_host::remote_management::RemoteExperiments {
            projects: input["seedSyntheticSession"] == true,
            control_transfer: input["seedSyntheticSession"] == true,
            device_panel: input["seedSyntheticSession"] == true,
        })
        .with_persistence(
            &directory,
            Arc::new(fabrials_agent_host::picker::UnavailableDirectoryPicker),
        )
        .unwrap(),
    );
    let owner_control = tokio::spawn(fabrials_agent_host::control::serve(
        fabrials_agent_host::control::bind(&directory).unwrap(),
        host.clone(),
    ));
    if input["seedSyntheticSession"] == true {
        let projects = directory.join("synthetic-projects");
        let workspace = projects.join("Synthetic QA - no Grok process");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(projects.join("Second synthetic project")).unwrap();
        let mut index = fabrials_agent_host::workspace::load(&directory).unwrap();
        let enrolled = index
            .enrol(&workspace, fabrials_agent_host::now_ms())
            .unwrap();
        fabrials_agent_host::workspace::persist(&directory, &index).unwrap();
        host.load_workspaces(&index).await;
        let grok_home = fabrials_agent_host::session_catalog::grok_home();
        assert_eq!(
            grok_home,
            directory.join("synthetic-grok"),
            "the synthetic fixture requires its own GROK_HOME"
        );
        let transcript = grok_home
            .join("sessions")
            .join(fabrials_agent_host::session_catalog::encode_cwd_dirname(
                workspace.to_str().unwrap(),
            ))
            .join("synthetic-push-session");
        fabrials_agent_host::instance::ensure_private_directory(&transcript).unwrap();
        state::write_private_json(
            &transcript,
            "summary.json",
            &json!({
                "info":{"id":"synthetic-push-session","cwd":workspace},
                "session_summary":"Synthetic QA - no Grok process",
                "updated_at":"2026-10-13T00:00:00Z",
                "num_messages":2,
                "session_kind":"conversation"
            }),
        )
        .unwrap();
        let updates = [
            ("user_message_chunk", "Conversación sintética para comprobar borradores, dictado, lectura en voz alta y vista previa. No se ha ejecutado Grok."),
            ("agent_message_chunk", "Texto sintético de prueba. Esta frase permite comprobar la lectura en voz alta y la vista previa al compartir. No es una respuesta de Grok ni procede de inferencia."),
        ].into_iter().map(|(kind, text)| json!({"params":{"sessionId":"synthetic-push-session","update":{"sessionUpdate":kind,"content":{"type":"text","text":text}}}}).to_string()).collect::<Vec<_>>().join("\n")+"\n";
        let updates_path = transcript.join("updates.jsonl");
        std::fs::write(&updates_path, updates).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&updates_path, std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        assert_eq!(
            fabrials_agent_host::session_catalog::list_for_cwd(&workspace).len(),
            1
        );
        assert_eq!(
            fabrials_agent_host::session_catalog::rehydrate_session(
                &workspace,
                "synthetic-push-session"
            )
            .messages
            .len(),
            2
        );
        host.session
            .lock()
            .await
            .open_session(fabrials_agent_host::dispatch::LiveSession {
                id: "synthetic-push-session".into(),
                workspace_id: enrolled.id,
                workspace_name: "Synthetic QA - no Grok process".into(),
                running: false,
                queued: Vec::new(),
                opened_at_ms: fabrials_agent_host::now_ms(),
            })
            .unwrap();
        let response = fabrials_agent_host::control::call(
            &directory,
            &fabrials_agent_host::control::ControlRequest::AuthorizeRemoteRoot { path: projects },
        )
        .await
        .unwrap();
        assert!(matches!(
            response,
            fabrials_agent_host::control::ControlResponse::RemoteRoots { .. }
        ));
    }
    let nonce = host
        .pairing
        .lock()
        .await
        .mint_nonce(fabrials_agent_host::now_ms())
        .unwrap();
    state::write_private_json(
        &directory,
        "host-info.json",
        &json!({
            "hostId":config.host_id,
            "hostPublicKey":config.public_key,
            "nonce":nonce.expose(),
            "publicationAvailable":false,
        }),
    )
    .unwrap();
    let publisher = Arc::new(
        shares::Shares::new(host.clone(), directory.clone(), http_client().unwrap())
            .unwrap()
            .preview_only(),
    );
    let notifications: Option<Arc<dyn notifications::NotificationService>> =
        if let Some(api) = input["pushApi"].as_str() {
            let token = input["installationBearer"]
                .as_str()
                .expect("fixture installation credential");
            Some(Arc::new(
                notifications::Notifications::with_loopback_api(
                    host.clone(),
                    directory.clone(),
                    config.host_id.clone(),
                    reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .timeout(Duration::from_secs(3))
                        .build()
                        .unwrap(),
                    Arc::new(FixtureCredentials(Zeroizing::new(token.into()))),
                    api,
                )
                .await
                .unwrap(),
            ))
        } else {
            None
        };
    let notification_worker = notifications.as_ref().map(|service| {
        let service = service.clone();
        tokio::spawn(async move { service.run().await })
    });
    let monitor = status::Monitor::new(&directory).unwrap();
    let (socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let serving = host.clone();
    let root = directory.clone();
    let ticket = input["ticket"].as_str().unwrap().to_owned();
    let fixture_host_id = config.host_id.clone();
    let fixture_public_key = config.public_key.clone();
    let mut worker = tokio::spawn(async move {
        serve_socket(
            serving,
            &root,
            &config,
            publisher,
            &monitor,
            socket,
            &ticket,
            notifications,
        )
        .await
    });
    let timeout = input["timeoutSeconds"].as_u64().unwrap_or(120);
    assert!((30..=7200).contains(&timeout));
    let deadline = tokio::time::sleep(Duration::from_secs(timeout));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            result = &mut worker => panic!("fixture disconnected early: {result:?}"),
            _ = &mut deadline => {
                host.request_shutdown();
                let _ = worker.await;
                panic!("managed acceptance driver timed out");
            },
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if directory.join("stop").exists() {
                    host.request_shutdown();
                    worker.await.unwrap().unwrap();
                    if let Some(worker) = notification_worker { worker.await.unwrap(); }
                    owner_control.abort();
                    return;
                }
                let command_path = directory.join("notification-command.json");
                if command_path.exists() {
                    let value: Value = serde_json::from_slice(&std::fs::read(&command_path).unwrap()).unwrap();
                    let id = value["id"].as_str().expect("synthetic notification id");
                    assert!(opaque(id));
                    let receipt = match value["kind"].as_str() {
                        Some("pair") => {
                            let nonce = host.pairing.lock().await.mint_nonce(fabrials_agent_host::now_ms()).unwrap();
                            json!({"id":id,"hostId":fixture_host_id,"hostPublicKey":fixture_public_key,"nonce":nonce.expose()})
                        }
                        Some("permission") => {
                            host.emit_event(fabrials_agent_host::protocol::Event::PermissionRequest {
                                session_id:"synthetic-push-session".into(), request_id:id.into(), options:vec!["allow-once".into()], title:None, description:None,
                            }, None).await;
                            json!({"id":id,"emitted":true})
                        }
                        Some("turn") => {
                            for state in ["running", "idle"] {
                                host.emit_event(fabrials_agent_host::protocol::Event::SessionStatus { session_id:"synthetic-push-session".into(), state:state.into() }, None).await;
                            }
                            json!({"id":id,"emitted":true})
                        }
                        Some("revoke") => {
                            let device_id = value["deviceId"].as_str().expect("paired synthetic device");
                            let response = fabrials_agent_host::control::call(&directory, &fabrials_agent_host::control::ControlRequest::RevokePairing { session_id:device_id.into() }).await.unwrap();
                            assert!(matches!(response, fabrials_agent_host::control::ControlResponse::Revoked));
                            json!({"id":id,"emitted":true})
                        }
                        _ => panic!("invalid synthetic notification command"),
                    };
                    std::fs::remove_file(command_path).unwrap();
                    state::write_private_json(&directory, "notification-command-receipt.json", &receipt).unwrap();
                }
            }
        }
    }
}
