//! HTTPS-only native transport. Credentials stay out of URLs and diagnostics.
use super::wire::{Exported, Imported, Inventory, OkResponse, Request, Revision};
use fabrials_accounts::transfer::ApiKeyTransfer;
use fabrials_core::migration::{MigrationCandidate, MigrationReview, MigrationSessionView};
use serde::de::DeserializeOwned;
#[cfg(test)]
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};

const MAX_BYTES: usize = 1024 * 1024;
pub struct Client {
    origin: url::Url,
    http: reqwest::blocking::Client,
}
fn token(value: &str) -> bool {
    (32..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
pub fn new_proof() -> Result<String, String> {
    let mut bytes = [0; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| "Random source unavailable")?;
    Ok(hex::encode(bytes))
}
impl Client {
    pub fn new(origin: &str) -> Result<Self, String> {
        Self::build(origin, false)
    }
    fn build(origin: &str, allow_http: bool) -> Result<Self, String> {
        if origin.len() > 2048 || origin.trim() != origin || origin.chars().any(char::is_control) {
            return Err("Invalid hosted origin".into());
        }
        let origin = url::Url::parse(origin).map_err(|_| "Invalid hosted origin")?;
        if !(origin.scheme() == "https" || allow_http && origin.scheme() == "http")
            || origin.host().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            return Err("Use an HTTPS origin without credentials, paths, query or fragment".into());
        }
        let http = reqwest::blocking::Client::builder()
            .https_only(!allow_http)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| "Migration HTTP client unavailable")?;
        Ok(Self { origin, http })
    }
    pub fn origin(&self) -> &str {
        self.origin.as_str()
    }
    fn post<T: DeserializeOwned>(
        &self,
        action: &str,
        id: &str,
        proof: &str,
        body: impl serde::Serialize,
    ) -> Result<T, String> {
        if !token(id) || !token(proof) {
            return Err("Invalid migration session or proof".into());
        }
        let payload = serde_json::to_vec(&body).map_err(|_| "Invalid migration request")?;
        if payload.len() > MAX_BYTES {
            return Err("Migration request exceeds 1 MiB".into());
        }
        let url = self
            .origin
            .join(&format!("/ui/migration/v1/{action}"))
            .map_err(|_| "Invalid migration endpoint")?;
        let response = self
            .http
            .post(url)
            .bearer_auth(proof)
            .header("Content-Type", "application/json")
            .header("X-Requested-With", "spanreed-migration")
            .body(payload)
            .send()
            .map_err(|_| "Migration connection failed; check the session before retrying")?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "Migration service returned HTTP {}; check the session before retrying",
                status.as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BYTES as u64)
        {
            return Err("Migration response exceeds 1 MiB".into());
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or("")
            .trim();
        if !content_type.eq_ignore_ascii_case("application/json") {
            return Err("Migration service returned a non-JSON response".into());
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Migration response interrupted; check session before retrying")?;
        if bytes.len() > MAX_BYTES {
            return Err("Migration response exceeds 1 MiB".into());
        }
        serde_json::from_slice(&bytes).map_err(|_| "Invalid migration response".into())
    }
    pub fn pair(
        &self,
        id: &str,
        invitation: &str,
        proof: &str,
        local_environment: &str,
    ) -> Result<(), String> {
        if !token(proof) {
            return Err("Invalid migration proof".into());
        }
        let challenge = hex::encode(Sha256::digest(proof.as_bytes()));
        let result: OkResponse = self.post(
            "pair",
            id,
            invitation,
            Request {
                id: id.into(),
                challenge: Some(challenge),
                local_environment: Some(local_environment.into()),
                ..Default::default()
            },
        )?;
        result.check()
    }
    pub fn status(&self, id: &str, proof: &str) -> Result<MigrationSessionView, String> {
        let view: MigrationSessionView = self.post(
            "status",
            id,
            proof,
            Request {
                id: id.into(),
                ..Default::default()
            },
        )?;
        if view.id != id {
            return Err("Migration response belongs to another session".into());
        }
        Ok(view)
    }
    pub fn candidates(&self, id: &str, proof: &str) -> Result<Vec<MigrationCandidate>, String> {
        let inventory: Inventory = self.post(
            "candidates",
            id,
            proof,
            Request {
                id: id.into(),
                ..Default::default()
            },
        )?;
        Ok(inventory.accounts)
    }
    pub fn propose(
        &self,
        id: &str,
        proof: &str,
        review: &MigrationReview,
    ) -> Result<String, String> {
        let result: Revision = self.post(
            "propose",
            id,
            proof,
            Request {
                id: id.into(),
                review: Some(review.clone()),
                ..Default::default()
            },
        )?;
        let expected = hex::encode(Sha256::digest(
            serde_json::to_vec(review).map_err(|_| "Invalid migration review")?,
        ));
        if result.revision != expected {
            return Err("Migration review revision does not match".into());
        }
        Ok(result.revision)
    }
    pub fn import(
        &self,
        id: &str,
        proof: &str,
        revision: &str,
        entries: &[ApiKeyTransfer],
    ) -> Result<Vec<String>, String> {
        let result: Imported = self.post(
            "import",
            id,
            proof,
            Request {
                id: id.into(),
                revision: Some(revision.into()),
                entries: Some(entries.to_vec()),
                ..Default::default()
            },
        )?;
        let mut expected = entries
            .iter()
            .map(|entry| format!("{}/{}", entry.provider, entry.target_alias))
            .collect::<Vec<_>>();
        let mut actual = result.account_ids.clone();
        expected.sort();
        actual.sort();
        if actual != expected {
            return Err("Migration receipt does not match transferred accounts".into());
        }
        Ok(result.account_ids)
    }
    pub fn export(
        &self,
        id: &str,
        proof: &str,
        revision: &str,
        review: &MigrationReview,
    ) -> Result<Vec<ApiKeyTransfer>, String> {
        let result: Exported = self.post(
            "export",
            id,
            proof,
            Request {
                id: id.into(),
                revision: Some(revision.into()),
                ..Default::default()
            },
        )?;
        super::validate_transfer(review, &review.destination_environment, &result.entries)
            .map_err(str::to_string)?;
        Ok(result.entries)
    }
    pub fn acknowledge_export(&self, id: &str, proof: &str, revision: &str) -> Result<(), String> {
        let result: OkResponse = self.post(
            "acknowledge",
            id,
            proof,
            Request {
                id: id.into(),
                revision: Some(revision.into()),
                ..Default::default()
            },
        )?;
        result.check()
    }
    pub fn cancel(&self, id: &str, proof: &str) -> Result<(), String> {
        let result: OkResponse = self.post(
            "cancel",
            id,
            proof,
            Request {
                id: id.into(),
                ..Default::default()
            },
        )?;
        result.check()
    }
}
impl OkResponse {
    fn check(self) -> Result<(), String> {
        if self.ok {
            Ok(())
        } else {
            Err("Migration operation was not confirmed".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    fn server(reply: Vec<u8>) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            use std::io::Write;
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 16384);
            }
            let headers = String::from_utf8(request.clone()).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            let start = request.len();
            request.resize(start + length, 0);
            stream.read_exact(&mut request[start..]).unwrap();
            let _ = stream.write_all(&reply);
            request
        });
        (origin, worker)
    }
    fn json_reply(body: &str) -> Vec<u8> {
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).into_bytes()
    }
    #[test]
    fn production_requires_a_credential_free_https_origin() {
        for origin in [
            "http://example.test",
            "https://user:secret@example.test",
            "https://example.test/path",
            "https://example.test?secret=key",
            "https://example.test/#fragment",
            " https://example.test",
            "https://example.test\n",
        ] {
            assert!(Client::new(origin).is_err());
        }
        assert_eq!(
            Client::new("https://example.test:8443").unwrap().origin(),
            "https://example.test:8443/"
        );
    }
    #[test]
    fn pairing_sends_only_challenge_and_does_not_leak_verifier() {
        let (origin, server) = server(json_reply(r#"{"ok":true}"#));
        let client = Client::build(&origin, true).unwrap();
        let id = new_proof().unwrap();
        let invite = new_proof().unwrap();
        let proof = new_proof().unwrap();
        client
            .pair(&id, &invite, &proof, "spanreed-fixture")
            .unwrap();
        let raw = String::from_utf8(server.join().unwrap()).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("POST /ui/migration/v1/pair HTTP/1.1"));
        assert!(head
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {invite}")));
        assert!(!raw.contains(&proof));
        assert!(!body.contains(&invite));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            payload["challenge"],
            hex::encode(Sha256::digest(proof.as_bytes()))
        );
    }
    #[test]
    fn never_follows_redirects_with_session_proof() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let reply=format!("HTTP/1.1 302 Found\r\nLocation: http://{}/capture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",destination.local_addr().unwrap());
        let (origin, server) = server(reply.into_bytes());
        let client = Client::build(&origin, true).unwrap();
        let error = client
            .cancel(&new_proof().unwrap(), &new_proof().unwrap())
            .unwrap_err();
        assert!(error.contains("HTTP 302"));
        assert!(
            matches!(destination.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
        );
        server.join().unwrap();
    }
    #[test]
    fn bounds_streaming_responses_and_redacts_remote_errors() {
        let mut oversized =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n"
                .to_vec();
        oversized.extend(vec![b' '; MAX_BYTES + 1]);
        let (origin, worker) = server(oversized);
        let error = Client::build(&origin, true)
            .unwrap()
            .cancel(&new_proof().unwrap(), &new_proof().unwrap())
            .unwrap_err();
        assert_eq!(error, "Migration response exceeds 1 MiB");
        worker.join().unwrap();
        let (origin,worker)=server(b"HTTP/1.1 409 Conflict\r\nContent-Length: 16\r\nConnection: close\r\n\r\nsynthetic-secret".to_vec());
        let error = Client::build(&origin, true)
            .unwrap()
            .cancel(&new_proof().unwrap(), &new_proof().unwrap())
            .unwrap_err();
        assert!(error.contains("HTTP 409"));
        assert!(!error.contains("synthetic-secret"));
        worker.join().unwrap();
    }
    #[test]
    #[ignore = "Requires isolated HTTPS ai-relay/PostgreSQL fixture and explicit trusted CA"]
    fn trusted_https_hosted_lifecycle() {
        use fabrials_core::migration::{MigrationAction, MigrationItem};
        assert_eq!(std::env::var("FABRIALS_HTTPS_FIXTURE").unwrap(), "1");
        let origin = "https://localhost:19488";
        let cookie = std::env::var("FABRIALS_HTTPS_FIXTURE_COOKIE").unwrap();
        let http = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let hosted = |action: &str, body: Value| -> Value {
            let response = http
                .post(format!("{origin}/ui/api/migration/{action}"))
                .header("Cookie", &cookie)
                .header("Origin", origin)
                .header("Content-Type", "application/json")
                .header("X-Requested-With", "ai-relay-dashboard")
                .body(serde_json::to_vec(&body).unwrap())
                .send()
                .unwrap();
            assert_eq!(response.status().as_u16(), 200);
            serde_json::from_slice(&response.bytes().unwrap()).unwrap()
        };
        let invitation = hosted("session", json!({"direction":"localToHosted"}));
        let id = invitation["id"].as_str().unwrap();
        let proof = new_proof().unwrap();
        let client = Client::new(origin).unwrap();
        client
            .pair(
                id,
                invitation["secret"].as_str().unwrap(),
                &proof,
                "spanreed_https_rust_fixture",
            )
            .unwrap();
        let view = client.status(id, &proof).unwrap();
        let review = MigrationReview {
            source_environment: "spanreed_https_rust_fixture".into(),
            destination_environment: view.hosted_environment,
            items: vec![MigrationItem {
                source_id: "nous/source".into(),
                source_generation: "fixture-generation".into(),
                provider: "nous".into(),
                target_alias: "rust-https-copy".into(),
                action: MigrationAction::CopyApiKey,
            }],
        };
        let revision = client.propose(id, &proof, &review).unwrap();
        let entries = vec![ApiKeyTransfer {
            source_id: "nous/source".into(),
            source_generation: "fixture-generation".into(),
            provider: "nous".into(),
            target_alias: "rust-https-copy".into(),
            api_key: fabrials_accounts::transfer::ApiKey::new("synthetic-rust-https-key".into())
                .unwrap(),
        }];
        assert!(client.import(id, &proof, &revision, &entries).is_err());
        hosted("session/approve", json!({"id":id,"revision":revision}));
        assert_eq!(
            client.import(id, &proof, &revision, &entries).unwrap(),
            vec!["nous/rust-https-copy"]
        );
        assert_eq!(
            client.import(id, &proof, &revision, &entries).unwrap(),
            vec!["nous/rust-https-copy"]
        );
        assert_eq!(client.status(id, &proof).unwrap().phase, "completed");
        hosted("session/forget", json!({"id":id}));
        assert!(client.status(id, &proof).is_err());

        let outgoing = hosted("session", json!({"direction":"hostedToLocal"}));
        let outgoing_id = outgoing["id"].as_str().unwrap();
        let outgoing_proof = new_proof().unwrap();
        client
            .pair(
                outgoing_id,
                outgoing["secret"].as_str().unwrap(),
                &outgoing_proof,
                "spanreed_https_destination",
            )
            .unwrap();
        assert!(client.status(outgoing_id, &proof).is_err());
        let outgoing_view = client.status(outgoing_id, &outgoing_proof).unwrap();
        let candidate = client
            .candidates(outgoing_id, &outgoing_proof)
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == "nous/rust-https-copy")
            .unwrap();
        let outgoing_review = MigrationReview {
            source_environment: outgoing_view.hosted_environment,
            destination_environment: "spanreed_https_destination".into(),
            items: vec![MigrationItem {
                source_id: candidate.id,
                source_generation: candidate.generation,
                provider: candidate.provider,
                target_alias: "returned".into(),
                action: MigrationAction::CopyApiKey,
            }],
        };
        let outgoing_revision = client
            .propose(outgoing_id, &outgoing_proof, &outgoing_review)
            .unwrap();
        assert!(client
            .export(
                outgoing_id,
                &outgoing_proof,
                &outgoing_revision,
                &outgoing_review
            )
            .is_err());
        hosted(
            "session/approve",
            json!({"id":outgoing_id,"revision":outgoing_revision}),
        );
        let exported = client
            .export(
                outgoing_id,
                &outgoing_proof,
                &outgoing_revision,
                &outgoing_review,
            )
            .unwrap();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].api_key, entries[0].api_key);
        let destination =
            std::env::temp_dir().join(format!("fabrials-https-import-{}", new_proof().unwrap()));
        for _ in 0..2 {
            assert_eq!(
                crate::migration::import_files(
                    &destination,
                    "spanreed_https_destination",
                    outgoing_id,
                    &outgoing_review,
                    &exported
                )
                .unwrap(),
                vec!["nous/returned"]
            );
        }
        client
            .acknowledge_export(outgoing_id, &outgoing_proof, &outgoing_revision)
            .unwrap();
        client
            .acknowledge_export(outgoing_id, &outgoing_proof, &outgoing_revision)
            .unwrap();
        assert_eq!(
            client.status(outgoing_id, &outgoing_proof).unwrap().phase,
            "completed"
        );
        assert!(client
            .export(
                outgoing_id,
                &outgoing_proof,
                &outgoing_revision,
                &outgoing_review
            )
            .is_err());
        let registry: Value =
            serde_json::from_slice(&std::fs::read(destination.join("index.json")).unwrap())
                .unwrap();
        assert_eq!(registry["accounts"].as_array().unwrap().len(), 1);
        assert_eq!(registry["accounts"][0]["active"], false);
        std::fs::remove_dir_all(destination).unwrap();
        hosted("session/forget", json!({"id":outgoing_id}));
    }
}
