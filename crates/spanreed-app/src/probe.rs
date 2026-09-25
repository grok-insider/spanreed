//! Probe orchestration: built-in providers run concurrently, then host
//! accounts and addons; local cost is added where the provider had none, and
//! the probe hooks see every result.

use std::thread;

use crate::model::{MetricKind, ProviderOutput};
use crate::ports::{ProbePorts, Provider, ProviderCatalog};

/// Probe every provider that is detected on this machine, in parallel.
pub fn probe_detected(catalog: &dyn ProviderCatalog, ports: ProbePorts<'_>) -> Vec<ProviderOutput> {
    probe_filtered(catalog, ports, |p| p.detect())
}

/// Probe all providers regardless of detection (used by `probe <id> --force`).
pub fn probe_all(catalog: &dyn ProviderCatalog, ports: ProbePorts<'_>) -> Vec<ProviderOutput> {
    probe_filtered(catalog, ports, |_| true)
}

/// Probe a single provider by id (forced).
pub fn probe_one(
    catalog: &dyn ProviderCatalog,
    ports: ProbePorts<'_>,
    id: &str,
) -> Option<ProviderOutput> {
    let mut out = catalog
        .providers()
        .into_iter()
        .find(|p| p.id() == id)
        .map(|p| p.probe(ports))
        .or_else(|| catalog.account_output(ports, id))
        .or_else(|| catalog.addon_output(id));
    if let Some(o) = &mut out {
        enrich_local_usage(ports, o);
        ports.after_probe(std::slice::from_ref(o));
    }
    out
}

fn probe_filtered<F>(
    catalog: &dyn ProviderCatalog,
    ports: ProbePorts<'_>,
    filter: F,
) -> Vec<ProviderOutput>
where
    F: Fn(&dyn Provider) -> bool,
{
    let selected: Vec<_> = catalog
        .providers()
        .into_iter()
        .filter(|p| filter(p.as_ref()))
        .collect();

    // Each provider runs on its own thread; provider probes are blocking I/O.
    let mut outs: Vec<_> = thread::scope(|scope| {
        let handles: Vec<_> = selected
            .iter()
            .map(|p| scope.spawn(move || p.probe(ports)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    outs.extend(catalog.account_outputs(ports));
    outs.extend(catalog.addon_outputs());
    for o in &mut outs {
        enrich_local_usage(ports, o);
    }
    ports.after_probe(&outs);
    outs
}

fn enrich_local_usage(ports: ProbePorts<'_>, output: &mut ProviderOutput) {
    if output
        .lines
        .iter()
        .any(|line| line.kind() == MetricKind::Cost)
    {
        return;
    }
    let lines = ports.cost.local_cost_lines(&output.provider_id, None);
    output.lines.extend(lines);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MetricLine;
    use crate::ports::{AfterProbe, CaptureCostQuery, CostSource, ListedSource};
    use std::sync::Mutex;

    struct Fixed(&'static str, bool);

    impl Provider for Fixed {
        fn id(&self) -> &'static str {
            self.0
        }
        fn name(&self) -> &'static str {
            self.0
        }
        fn detect(&self) -> bool {
            self.1
        }
        fn probe(&self, _ports: ProbePorts<'_>) -> ProviderOutput {
            ProviderOutput::new(
                self.0,
                self.0,
                vec![MetricLine::percent("Weekly", 10.0, None)],
            )
        }
    }

    struct Catalog;

    impl ProviderCatalog for Catalog {
        fn providers(&self) -> Vec<Box<dyn Provider>> {
            vec![
                Box::new(Fixed("codex", true)),
                Box::new(Fixed("amp", false)),
            ]
        }
        fn account_outputs(&self, _: ProbePorts<'_>) -> Vec<ProviderOutput> {
            vec![ProviderOutput::new(
                "grok/heavy-1",
                "Grok (heavy-1)",
                vec![],
            )]
        }
        fn account_output(&self, _: ProbePorts<'_>, id: &str) -> Option<ProviderOutput> {
            (id == "grok/heavy-1").then(|| ProviderOutput::new(id, id, vec![]))
        }
        fn addon_outputs(&self) -> Vec<ProviderOutput> {
            Vec::new()
        }
        fn addon_output(&self, _: &str) -> Option<ProviderOutput> {
            None
        }
        fn listed(&self) -> Vec<ListedSource> {
            Vec::new()
        }
    }

    struct Cost;

    impl CostSource for Cost {
        fn local_cost_lines(&self, provider_id: &str, _: Option<i64>) -> Vec<MetricLine> {
            vec![MetricLine::text(
                MetricKind::Cost,
                "Last 30 Days",
                provider_id,
            )]
        }
        fn local_totals_since(&self, _: &str, _: i64) -> Option<(u64, f64)> {
            None
        }
        fn capture_cost_lines(&self, _: CaptureCostQuery<'_>) -> Vec<MetricLine> {
            Vec::new()
        }
    }

    #[derive(Default)]
    struct Seen(Mutex<Vec<String>>);

    impl AfterProbe for Seen {
        fn after_probe(&self, outputs: &[ProviderOutput]) {
            let mut seen = self.0.lock().unwrap();
            seen.extend(outputs.iter().map(|o| o.provider_id.clone()));
        }
    }

    #[test]
    fn detected_providers_accounts_cost_and_hooks() {
        let seen = Seen::default();
        let hooks: [&dyn AfterProbe; 1] = [&seen];
        let ports = ProbePorts {
            cost: &Cost,
            after: &hooks,
        };
        let outputs = probe_detected(&Catalog, ports);
        let ids: Vec<_> = outputs.iter().map(|o| o.provider_id.as_str()).collect();
        assert_eq!(ids, ["codex", "grok/heavy-1"]);
        assert!(
            outputs
                .iter()
                .all(|o| o.lines.iter().any(|l| l.kind() == MetricKind::Cost))
        );
        assert_eq!(*seen.0.lock().unwrap(), ["codex", "grok/heavy-1"]);
        assert_eq!(probe_all(&Catalog, ProbePorts::bare()).len(), 3);
    }

    #[test]
    fn one_provider_falls_back_to_accounts() {
        assert_eq!(
            probe_one(&Catalog, ProbePorts::bare(), "amp")
                .unwrap()
                .provider_id,
            "amp"
        );
        assert!(probe_one(&Catalog, ProbePorts::bare(), "grok/heavy-1").is_some());
        assert!(probe_one(&Catalog, ProbePorts::bare(), "missing").is_none());
    }
}
