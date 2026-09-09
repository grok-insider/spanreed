use fabrials_core::usage::{ImportCheckpoint, UsageParser};
fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap();
        let ours = fabrials_providers::usage::codex::Codex
            .parse(&bytes, "fixture", &ImportCheckpoint::default())
            .unwrap();
        let reference =
            fabrials_providers::usage::files::read("codex", std::path::Path::new(&path)).unwrap();
        let sum = |records: &[fabrials_core::usage::UsageRecord]| {
            records
                .iter()
                .map(|r| r.tokens.as_ref().map_or(0, |t| t.total()))
                .sum::<u64>()
        };
        println!(
            "{}",
            serde_json::json!({"file":std::path::Path::new(&path).file_name().unwrap().to_string_lossy(),"bytes":bytes.len(),"ours_tokens":sum(&ours.records),"reference_tokens":sum(&reference),"ours_records":ours.records.len(),"reference_records":reference.len(),"rejected":ours.rejected_records})
        );
    }
}
