//! Port implementations and [`standard`], the adapter set a composition
//! root passes to `AppContext::new`.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use spanreed_app::app::accounts::{AccountSummary, LoginView, Progress};
use spanreed_app::app::addons::AddonListing;
use spanreed_app::app::agent::AgentStatus;
use spanreed_app::app::capture::Options;
use spanreed_app::app::clients::{HostedClientReview, HostedRequest, Preview, SessionMoveView};
use spanreed_app::app::fabrials::{LinkView, RemoteOperation};
use spanreed_app::app::migration::{
    MigrationCandidate, MigrationSelection, MigrationSessionView, SavedSession,
};
use spanreed_app::app::notifications::Settings;
use spanreed_app::app::proxy::Status;
use spanreed_app::app::routing::{RoutingLimits, RoutingPolicy};
use spanreed_app::app::setup::{
    Line, Outcome, PromptHints, SetupOptions, SetupPlan, SetupStatus, UninstallOptions,
};
use spanreed_app::app::sharing::{PendingLogin, SharingConsent};
use spanreed_app::app::sync::{PrivateRecentPage, SyncSettings, SyncStatus};
use spanreed_app::app::updates::CheckResult;
use spanreed_app::app::usage::{
    CardInput, Connection, HistorySample, HopRecord, PricingMap, UsageFilter, UsageReport,
    UsageSettings,
};
use spanreed_app::app::window::PendingAlert;
use spanreed_app::context::AppContext;
use spanreed_app::ports::{self, AfterProbe, AppPaths, ListedSource, ProbePorts, Services};

use crate::model::ProviderOutput;

/// Built-in providers, host-account drivers and addon providers.
pub struct Catalog;

impl ports::ProviderCatalog for Catalog {
    fn providers(&self) -> Vec<Box<dyn ports::Provider>> {
        crate::providers::all()
    }

    fn account_outputs(&self, ports: ProbePorts<'_>) -> Vec<ProviderOutput> {
        let mut outputs = crate::drivers::grok::probe_accounts(ports);
        outputs.extend(crate::drivers::codex::probe_accounts());
        outputs
    }

    fn account_output(&self, ports: ProbePorts<'_>, id: &str) -> Option<ProviderOutput> {
        crate::drivers::grok::probe_accounts(ports)
            .into_iter()
            .find(|output| output.provider_id == id)
            .or_else(|| {
                crate::drivers::codex::probe_accounts()
                    .into_iter()
                    .find(|output| output.provider_id == id)
            })
    }

    fn addon_outputs(&self) -> Vec<ProviderOutput> {
        crate::addons::extra_detected_outputs()
    }

    fn addon_output(&self, id: &str) -> Option<ProviderOutput> {
        crate::addons::host::probe_extra_one(id)
    }

    fn listed(&self) -> Vec<ListedSource> {
        let accounts = crate::accounts::list_provider("grok")
            .into_iter()
            .map(|account| ListedSource {
                name: format!("Grok ({})", account.alias),
                state: if account.active { "active" } else { "account" },
                id: account.id,
            });
        let addons = crate::addons::extra_provider_ids()
            .into_iter()
            .map(|(id, name, detected)| ListedSource {
                id,
                name: format!("{name} (addon)"),
                state: if detected { "detected" } else { "—" },
            });
        accounts.chain(addons).collect()
    }
}

/// Probe bookkeeping: first-seen weekly pool % and early reset detection.
pub struct ProbeBookkeeping;

impl AfterProbe for ProbeBookkeeping {
    fn after_probe(&self, outputs: &[ProviderOutput]) {
        for output in outputs {
            crate::pool_baseline::note_from_output(output);
        }
        crate::epoch::note_jumps_from_outputs(outputs);
    }
}

impl ports::PricingSource for crate::pricing::Catalog {
    fn table(&self) -> Arc<PricingMap> {
        crate::pricing::Catalog::table(self)
    }
    fn reload(&self) {
        crate::pricing::Catalog::reload(self)
    }
    fn refresh(&self) {
        crate::pricing::Catalog::refresh(self)
    }
    fn fetch_upstream(&self) -> Result<String, String> {
        crate::pricing::fetch_filtered()
    }
}

/// Quota history, the capture ledger and local consumption.
pub struct Usage;

impl ports::UsageStore for Usage {
    fn record_history(&self, outputs: &[ProviderOutput]) {
        crate::history::record(outputs);
    }
    fn should_record_on_probe(&self) -> bool {
        crate::history::should_record_on_probe()
    }
    fn history_samples(
        &self,
        provider: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HistorySample>, String> {
        crate::history::local_samples(provider, limit)
    }
    fn format_history(&self, samples: &[HistorySample]) -> String {
        crate::history::format_table(samples)
    }
    fn recent_hops(&self) -> Result<Vec<HopRecord>, String> {
        crate::grok_ledger::recent_hops()
    }
    fn report(
        &self,
        filter: UsageFilter,
        force: bool,
        pricing: &PricingMap,
    ) -> Result<UsageReport, String> {
        crate::usage::report(filter, force, pricing)
    }
    fn sources(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "clients": crate::usage::catalog_view(),
            "settings": crate::usage::discovery::settings()?,
            "connections": crate::usage::connections::status()?,
        }))
    }
    fn save_connection(&self, connection: Connection) -> Result<(), String> {
        crate::usage::connections::save(connection)
    }
    fn disconnect(&self, client: &str) -> Result<(), String> {
        crate::usage::connections::disconnect(client)
    }
    fn discovery_settings(&self) -> Result<UsageSettings, String> {
        crate::usage::discovery::settings()
    }
    fn save_discovery(&self, settings: &UsageSettings) -> Result<(), String> {
        crate::usage::discovery::save(settings)
    }
    fn card_html(&self, card: CardInput<'_>, pricing: &PricingMap) -> String {
        crate::tray_card::present(
            card.outputs,
            card.capture_up,
            card.status,
            crate::util::now_ms(),
            pricing,
        )
    }
    fn can_use_reset(&self, output: &ProviderOutput) -> bool {
        crate::tray_card::can_use_reset(output)
    }
    fn waybar(&self, outputs: &[ProviderOutput]) -> serde_json::Value {
        crate::output::waybar(outputs)
    }
}

/// The account registry, keyring/private-file secrets and account drivers.
pub struct Accounts;

impl ports::AccountStore for Accounts {
    fn accounts(&self) -> Result<Vec<AccountSummary>, String> {
        Ok(crate::accounts::routing_registry()?
            .accounts
            .into_iter()
            .map(|account| AccountSummary {
                id: account.id,
                provider: account.provider,
                alias: account.alias,
                generation: account.generation,
                active: account.active,
                plan_label: account.plan_label,
            })
            .collect())
    }
    fn add_api_key(&self, provider: &str, alias: &str, key: &str) -> Result<(), String> {
        crate::account_keys::add(provider, alias, key).map(|_| ())
    }
    fn replace_api_key(&self, id: &str, generation: Option<&str>, key: &str) -> Result<(), String> {
        crate::account_keys::replace(id, generation, key)
    }
    fn remove(&self, id: &str, generation: Option<&str>) -> Result<(), String> {
        crate::accounts::remove_if_current(id, generation)
    }
    fn activate(&self, id: &str) -> Result<(), String> {
        crate::accounts::set_active(id).map(|_| ())
    }
    fn command(&self, args: &[String]) -> Result<String, String> {
        crate::drivers::dispatch_account(args)
    }
    fn link_copilot(&self, args: &[String]) -> Result<(), String> {
        crate::providers::copilot::cmd_auth(args)
    }
    fn unlink_copilot(&self) -> Result<(), String> {
        crate::providers::copilot::cmd_logout()
    }
    fn models(&self, account_id: &str) -> Result<Vec<String>, String> {
        crate::local_relay::models_for_account(account_id)
    }
    fn redeem_codex_reset(&self, request_id: &str) -> Result<(), &'static str> {
        crate::providers::codex::redeem_reset(request_id)
    }
    fn valid_reset_request(&self, request_id: &str) -> bool {
        crate::providers::codex::valid_redeem_request_id(request_id)
    }
}

impl ports::DeviceLogins for crate::account_login::Logins {
    fn begin(&self, provider: &str, alias: String) -> Result<LoginView, String> {
        crate::account_login::Logins::begin(self, provider, alias)
    }
    fn begin_inactive(&self, provider: &str, alias: String) -> Result<LoginView, String> {
        crate::account_login::Logins::begin_inactive(self, provider, alias)
    }
    fn reauthorize(&self, id: &str) -> Result<LoginView, String> {
        crate::account_login::Logins::reauthorize(self, id)
    }
    fn poll(&self, id: &str) -> Result<Progress, String> {
        crate::account_login::Logins::poll(self, id)
    }
    fn cancel(&self, id: &str) -> Result<(), String> {
        crate::account_login::Logins::cancel(self, id)
    }
    fn verification_url(&self, id: &str) -> Result<String, String> {
        crate::account_login::Logins::verification_url(self, id)
    }
}

/// Routing policies in config.json and the account pools behind them.
pub struct Routing;

impl ports::RoutingStore for Routing {
    fn providers(&self) -> &'static [&'static str] {
        crate::local_control::PROVIDERS
    }
    fn policy(&self, provider: &str) -> Result<RoutingPolicy, String> {
        crate::local_control::policy_view(provider)
    }
    fn limits(&self) -> Result<RoutingLimits, String> {
        crate::local_control::limits_view()
    }
    fn set_policy(&self, provider: &str, on: bool, threshold: Option<f64>) -> Result<(), String> {
        crate::local_control::set_policy(provider, on, threshold)
    }
}

/// Notification settings, the delivery store and the OS notifier.
pub struct Notices;

impl ports::Notifications for Notices {
    fn settings(&self) -> Result<Settings, String> {
        crate::notifications::settings()
    }
    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        crate::notifications::set_enabled(enabled)
    }
    fn deliver(
        &self,
        outputs: &[ProviderOutput],
        send: &mut dyn FnMut(&str, &str) -> Result<(), String>,
    ) -> Result<u32, String> {
        crate::notifications::deliver_outputs(outputs, send)
    }
    fn deliver_user_visible(&self, title: &str, body: &str) -> Result<(), String> {
        crate::notifications::deliver_user_visible(title, body)
    }
}

/// Sharing consent, the share session and uploads to fabrials.com.
pub struct Share;

impl ports::Sharing for Share {
    fn consent(&self) -> SharingConsent {
        crate::privacy::load()
    }
    fn save_consent(&self, consent: &SharingConsent) -> Result<(), String> {
        crate::privacy::save(consent)
    }
    fn is_linked(&self) -> bool {
        crate::share_session::is_logged_in()
    }
    fn has_refresh_session(&self) -> bool {
        crate::share_session::load().is_some_and(|session| !session.refresh_token.is_empty())
    }
    fn last_shared_day(&self) -> Option<String> {
        crate::share_state::last_shared_day()
    }
    fn due_today(&self) -> bool {
        crate::share_state::is_due_today()
    }
    fn begin_link(&self) -> Result<PendingLogin, String> {
        crate::share_session::start_device_login()
    }
    fn wait_link(&self, pending: &PendingLogin) -> Result<(), String> {
        crate::share_session::wait_device_login(pending).map(|_| ())
    }
    fn unlink(&self) -> Result<(), String> {
        crate::share_session::clear()
    }
    fn share_now(&self, ctx: &AppContext, force: bool) -> Result<String, String> {
        crate::share::share_once(ctx, force)
    }
}

/// Private history synchronization with Fabrials.
pub struct SyncService;

impl ports::PrivateSync for SyncService {
    fn settings(&self) -> Result<SyncSettings, String> {
        crate::sync::settings()
    }
    fn save_settings(&self, settings: SyncSettings) -> Result<(), String> {
        crate::sync::save_settings(settings)
    }
    fn status(&self) -> SyncStatus {
        crate::sync::status()
    }
    fn recent(&self, before: Option<i64>) -> Result<PrivateRecentPage, String> {
        crate::sync::recent(before)
    }
    fn run(&self, pricing: &PricingMap) -> Result<String, String> {
        crate::sync::run(pricing)
    }
    fn link_codex(&self) -> Result<String, String> {
        crate::sync::link_codex()
    }
}

/// The Fabrials device link and hosted workspace client.
pub struct Fabrials;

impl ports::FabrialsLink for Fabrials {
    fn status(&self) -> Result<LinkView, String> {
        crate::fabrials_login::status()
    }
    fn begin(&self) -> Result<LinkView, String> {
        crate::fabrials_login::begin()
    }
    fn poll(&self, id: &str) -> Result<LinkView, String> {
        crate::fabrials_login::poll(id)
    }
    fn cancel(&self, id: &str) -> Result<(), String> {
        crate::fabrials_login::cancel(id)
    }
    fn disconnect(&self) -> Result<(), String> {
        crate::fabrials_login::disconnect()
    }
    fn verification_url(&self, id: &str) -> Result<String, String> {
        crate::fabrials_login::verification_url(id)
    }
    fn remote(
        &self,
        operation: RemoteOperation,
        body: serde_json::Value,
        days: Option<u32>,
        owner: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        crate::remote_workspace::request(operation, body, days, owner)
    }
    fn authorization_url(&self, raw: &str) -> Result<String, String> {
        crate::remote_workspace::verification_url(raw)
    }
}

/// Migration sessions; OAuth items authorize through the shared logins.
pub struct MigrationSessions {
    pub logins: Arc<crate::account_login::Logins>,
}

impl ports::Migrations for MigrationSessions {
    fn saved(&self) -> Result<Vec<SavedSession>, String> {
        crate::migration::session::saved()
    }
    fn candidates(&self) -> Result<Vec<MigrationCandidate>, String> {
        crate::migration::candidates()
    }
    fn forget(&self, id: &str) -> Result<(), String> {
        crate::migration::session::forget(id)
    }
    fn begin_authorization(&self, id: &str, source_id: &str) -> Result<LoginView, String> {
        crate::migration::session::begin_authorization(&self.logins, id, source_id)
    }
    fn authorizations(&self, id: &str) -> Result<Vec<String>, String> {
        crate::migration::session::authorizations(id)
    }
    fn inventory(&self, id: &str) -> Result<Vec<MigrationCandidate>, String> {
        crate::migration::session::inventory(id)
    }
    fn propose(
        &self,
        id: &str,
        selection: &[MigrationSelection],
    ) -> Result<MigrationSessionView, String> {
        crate::migration::session::propose(id, selection)
    }
    fn execute(&self, id: &str, revision: &str) -> Result<Vec<String>, String> {
        crate::migration::session::execute(id, revision)
    }
    fn pair(
        &self,
        origin: &str,
        id: &str,
        invitation: &str,
    ) -> Result<MigrationSessionView, String> {
        crate::migration::session::pair(origin, id, invitation)
    }
    fn status(&self, id: &str) -> Result<MigrationSessionView, String> {
        crate::migration::session::status(id)
    }
    fn cancel(&self, id: &str) -> Result<(), String> {
        crate::migration::session::cancel(id)
    }
}

/// Reviewed client configuration; local previews require the proxy this
/// process runs.
pub struct Clients {
    pub proxy: Arc<crate::desktop_runtime::ProxyControl>,
    pub reviews: crate::client_configuration::Reviews,
    pub hosted: crate::hosted_client_configuration::HostedReviews,
}

impl ports::ClientConfigurator for Clients {
    fn preview_grok(&self, alias: &str, model: Option<&str>) -> Result<Preview, String> {
        self.reviews
            .preview_grok_with_model(&self.proxy, alias, model)
    }
    fn preview_opencode(
        &self,
        provider: &str,
        alias: &str,
        model: &str,
    ) -> Result<Preview, String> {
        self.reviews
            .preview_opencode(&self.proxy, provider, alias, model)
    }
    fn preview_opencode_update(
        &self,
        provider: &str,
        alias: &str,
        model: &str,
    ) -> Result<Preview, String> {
        self.reviews
            .preview_opencode_change(&self.proxy, provider, alias, model, true)
    }
    fn preview_opencode_remove(&self, provider: &str, alias: &str) -> Result<Preview, String> {
        self.reviews
            .preview_opencode_remove(&self.proxy, provider, alias)
    }
    fn apply(&self, id: &str) -> Result<Option<String>, String> {
        self.reviews.apply_configuration(&self.proxy, id)
    }
    fn preview_hosted(&self, request: HostedRequest<'_>) -> Result<HostedClientReview, String> {
        self.hosted.preview(
            request.owner,
            request.client,
            request.alias,
            request.key,
            request.model,
        )
    }
    fn apply_hosted(&self, owner: &str, id: &str) -> Result<String, String> {
        self.hosted.apply(owner, id)
    }
    fn session_current(&self, owner: &str) -> Result<Option<SessionMoveView>, String> {
        crate::codex_session_move::current(owner)
    }
    fn session_preview(&self, owner: &str, alias: &str) -> Result<SessionMoveView, String> {
        crate::codex_session_move::preview(owner, alias)
    }
    fn session_apply(&self, owner: &str, id: &str) -> Result<SessionMoveView, String> {
        crate::codex_session_move::apply(owner, id)
    }
    fn session_recover(
        &self,
        owner: &str,
        id: &str,
        cancel: bool,
    ) -> Result<SessionMoveView, String> {
        crate::codex_session_move::recover(owner, id, cancel)
    }
    fn session_dismiss(&self, owner: &str, id: &str) -> Result<(), String> {
        crate::codex_session_move::dismiss(owner, id)
    }
}

impl ports::ProxyRuntime for crate::desktop_runtime::ProxyControl {
    fn status(&self) -> Result<Status, String> {
        crate::desktop_runtime::ProxyControl::status(self)
    }
    fn start(&self, bind: &str) -> Result<Status, String> {
        crate::desktop_runtime::ProxyControl::start(self, bind)
    }
    fn stop(&self) -> Result<Status, String> {
        crate::desktop_runtime::ProxyControl::stop(self)
    }
}

impl ports::AgentRuntime for crate::desktop_runtime::AgentControl {
    fn status(&self) -> Result<AgentStatus, String> {
        crate::desktop_runtime::AgentControl::status(self)
    }
    fn start(&self) -> Result<AgentStatus, String> {
        crate::desktop_runtime::AgentControl::start(self)
    }
    fn stop(&self) -> Result<AgentStatus, String> {
        crate::desktop_runtime::AgentControl::stop(self)
    }
}

/// The capture relay, its watchdog, log and user service.
pub struct Capture;

impl ports::CaptureService for Capture {
    fn ensure(&self, dry_run: bool) -> Result<String, String> {
        crate::setup::service_ensure(dry_run)
    }
    fn is_up(&self) -> bool {
        crate::setup::capture_ports_up()
    }
    fn log_path(&self) -> std::path::PathBuf {
        crate::capture_log::capture_log_path()
    }
    fn log(&self, message: &str) {
        crate::capture_log::append(message);
    }
    fn serve(&self, options: &Options) -> Result<(), String> {
        crate::capture::serve(options)
    }
    fn watchdog(&self, serve_args: &[String]) -> Result<(), String> {
        crate::capture_watchdog::run(serve_args)
    }
}

/// Installation, OS service units and client wiring.
pub struct Setup;

impl ports::Installer for Setup {
    fn apply(&self, plan: &SetupPlan, options: SetupOptions, out: &mut dyn FnMut(Line)) -> Outcome {
        crate::setup::apply(plan, options, out)
    }
    fn uninstall(&self, options: UninstallOptions, out: &mut dyn FnMut(Line)) -> Outcome {
        crate::setup::uninstall(options, out)
    }
    fn status(&self) -> SetupStatus {
        crate::setup::status()
    }
    fn prompt_hints(&self) -> PromptHints {
        crate::setup::prompt_hints()
    }
    fn tray_default(&self, capture_service_on: bool) -> bool {
        crate::setup::tray_default(capture_service_on)
    }
    fn share_schedule_status(&self) -> String {
        crate::setup::share_schedule::status()
    }
    fn enable_share_schedule(&self, dry_run: bool) -> Result<String, String> {
        crate::setup::share_schedule::enable(dry_run)
    }
    fn disable_share_schedule(&self, dry_run: bool) -> Result<String, String> {
        crate::setup::share_schedule::disable(dry_run)
    }
}

/// GitHub Releases self-update.
pub struct Updates;

impl ports::SelfUpdater for Updates {
    fn check_for_update(&self) -> Result<CheckResult, String> {
        crate::self_update::check_for_update()
    }
    fn apply_update(&self, result: &CheckResult, dry_run: bool) -> Result<String, String> {
        crate::self_update::apply_update(result, dry_run)
    }
    fn can_apply_self_update(&self) -> bool {
        crate::self_update::can_apply_self_update()
    }
    fn apply_blocked_reason(&self) -> Option<&'static str> {
        crate::self_update::apply_blocked_reason()
    }
    fn offline(&self) -> bool {
        crate::product::env_offline()
    }
}

/// The desktop window's route and alert files.
pub struct Window;

impl ports::DesktopBridge for Window {
    fn open(&self, page: &str) -> Result<(), String> {
        crate::desktop_open::request(page)
    }
    fn peek(&self) -> Option<String> {
        crate::desktop_open::peek()
    }
    fn take(&self) -> Option<String> {
        crate::desktop_open::take()
    }
    fn mark_running(&self) {
        crate::desktop_open::mark_running()
    }
    fn unmark_running(&self) {
        crate::desktop_open::unmark_running()
    }
    fn hand_off_alert(&self, title: &str, body: &str) -> bool {
        crate::desktop_open::hand_off_alert(title, body)
    }
    fn take_alert(&self) -> Option<PendingAlert> {
        crate::desktop_open::take_alert()
    }
    fn ack_alert(&self, id: &str) {
        crate::desktop_open::ack_alert(id)
    }
    fn route_location_script(&self, href: &str) -> Option<String> {
        crate::desktop_open::route_location_script(href)
    }
}

/// In-process, toml-manifest and PATH addons.
pub struct Addons;

impl ports::AddonHost for Addons {
    fn list(&self) -> Vec<AddonListing> {
        crate::addons::host::list_all()
    }
    fn dispatch_prefix(&self, prefix: &str, rest: &[String]) -> Option<ExitCode> {
        crate::addons::dispatch_prefix(prefix, rest)
    }
}

/// Built-in status-bar profiles and the panel snapshot.
pub struct Profiles;

impl ports::StatusBarProfiles for Profiles {
    fn files(&self, name: &str) -> Option<Vec<(&'static str, &'static str)>> {
        crate::profiles::files(name)
    }
    fn install(&self, name: &str, directory: &Path) -> Result<(), String> {
        crate::profiles::install(name, directory)
    }
    fn panel(&self) -> serde_json::Value {
        crate::panel::snapshot()
    }
}

/// The local HTTP API on 127.0.0.1:6736.
pub struct LocalApi;

impl ports::LocalApiServer for LocalApi {
    fn serve(&self, ctx: &AppContext, refresh_secs: u64) -> std::io::Result<()> {
        crate::api::serve(
            refresh_secs,
            crate::api::Services {
                source: Arc::new(ctx.clone()),
                notifier: ctx.notifier(),
            },
        )
    }
    fn cached(&self) -> Option<Vec<ProviderOutput>> {
        crate::api::fetch_cached()
    }
}

/// The directories this installation uses.
pub fn paths() -> AppPaths {
    AppPaths {
        config: crate::product::config_dir(),
        data: crate::product::data_dir(),
        cache: crate::product::cache_dir(),
    }
}

/// The production adapter set: loads the pricing catalog and owns this
/// process's proxy, agent host, device logins and pending reviews.
pub fn standard() -> Services {
    let pricing = Arc::new(crate::pricing::Catalog::load());
    let logins = Arc::new(crate::account_login::Logins::default());
    let proxy = Arc::new(crate::desktop_runtime::ProxyControl::default());
    Services {
        paths: paths(),
        providers: Arc::new(Catalog),
        probe_hooks: vec![
            Arc::new(ProbeBookkeeping),
            Arc::new(crate::sync::HistorySync::new(Arc::clone(&pricing))),
        ],
        cost: Arc::new(crate::cost::LocalCost::new(Arc::clone(&pricing))),
        notifier: Arc::new(crate::notifications::ResetExpiryNotifier),
        pricing,
        usage: Arc::new(Usage),
        accounts: Arc::new(Accounts),
        logins: logins.clone(),
        routing: Arc::new(Routing),
        notifications: Arc::new(Notices),
        sharing: Arc::new(Share),
        sync: Arc::new(SyncService),
        fabrials: Arc::new(Fabrials),
        migrations: Arc::new(MigrationSessions { logins }),
        clients: Arc::new(Clients {
            proxy: Arc::clone(&proxy),
            reviews: Default::default(),
            hosted: Default::default(),
        }),
        proxy,
        agent: Arc::new(crate::desktop_runtime::AgentControl::default()),
        capture: Arc::new(Capture),
        installer: Arc::new(Setup),
        updates: Arc::new(Updates),
        window: Arc::new(Window),
        addons: Arc::new(Addons),
        profiles: Arc::new(Profiles),
        local_api: Arc::new(LocalApi),
    }
}

/// An `AppContext` over [`standard`] adapters (tests and simple hosts).
pub fn context() -> AppContext {
    AppContext::new(standard())
}
