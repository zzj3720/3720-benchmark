use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
#[cfg(test)]
use std::fs::OpenOptions;
use std::fs::{self, File};
#[cfg(test)]
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Router, body::Body};
use fs2::FileExt;
use futures_util::StreamExt;
use notify::{RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, broadcast, watch};
use tokio_stream::wrappers::BroadcastStream;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::NotForContentType;

use crate::{replay_projection, storage};

const EMPTY_EXPERIENCE: &str = r#"{"updated_at":null,"source_count":0,"counts":{"plan":0,"verified":0,"rejected":0,"solved":0},"plan":[],"verified":[],"rejected":[],"solved":[]}"#;

#[derive(Clone)]
struct App {
    root: PathBuf,
    archive: PathBuf,
    journals: PathBuf,
    cache: PathBuf,
    run_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    summaries: Arc<Mutex<HashMap<String, CachedSummary>>>,
    details: Arc<Mutex<HashMap<String, CachedRun>>>,
    subscription: Arc<Mutex<CachedSubscription>>,
    revision: Arc<AtomicU64>,
    ready: Arc<AtomicBool>,
    shutdown: watch::Sender<bool>,
    changes: broadcast::Sender<u64>,
    _watcher: Option<Arc<Mutex<notify::RecommendedWatcher>>>,
}

impl App {
    /// A projection over the authority without HTTP serving or file watching.
    fn offline(root: PathBuf, cache: PathBuf) -> Self {
        Self {
            archive: root.join(".harbor/live-archive"),
            journals: root.join(".harbor/run-journals"),
            root,
            cache,
            run_locks: Arc::new(Mutex::new(HashMap::new())),
            summaries: Arc::new(Mutex::new(HashMap::new())),
            details: Arc::new(Mutex::new(HashMap::new())),
            subscription: Arc::new(Mutex::new(CachedSubscription::default())),
            revision: Arc::new(AtomicU64::new(1)),
            ready: Arc::new(AtomicBool::new(true)),
            shutdown: watch::channel(false).0,
            changes: broadcast::channel(1).0,
            _watcher: None,
        }
    }
}

/// Response bodies exactly as the gateway serves them, for publishers that
/// replicate the read API elsewhere.
pub struct Projector {
    app: App,
}

impl Projector {
    pub fn open(root: &Path, cache: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(display_error)?;
        fs::create_dir_all(cache).map_err(display_error)?;
        Ok(Self {
            app: App::offline(root, cache.to_path_buf()),
        })
    }

    pub fn journals(&self) -> &Path {
        &self.app.journals
    }

    pub fn archive(&self) -> &Path {
        &self.app.archive
    }

    /// Changes whenever any chain's authority, lease, or the archive changes.
    pub fn source_revisions(&self) -> BTreeMap<String, String> {
        source_revisions(&self.app.journals, &self.app.archive)
    }

    pub fn is_native(&self, run_id: &str) -> bool {
        source_signature(&self.app.journals.join(run_id)).is_some()
    }

    pub fn runs(&self) -> Result<Vec<Value>, String> {
        list_runs(&self.app)
    }

    pub fn detail(&self, run_id: &str) -> Payload {
        run_payload(&self.app, run_id, RunQuery::default())
    }

    pub fn catalog(&self, run_id: &str, before: u64) -> Payload {
        run_payload(
            &self.app,
            run_id,
            RunQuery {
                catalog_before: Some(before),
                ..RunQuery::default()
            },
        )
    }

    pub fn replay(&self, run_id: &str, attempt: u64, after: Option<u64>) -> Payload {
        run_payload(
            &self.app,
            run_id,
            RunQuery {
                replay_attempt: Some(attempt),
                after_sequence: after,
                ..RunQuery::default()
            },
        )
    }

    pub fn asset(&self, asset_id: &str) -> Payload {
        asset_payload(&self.app, asset_id)
    }
}

/// A read API body, or the HTTP status and message the gateway would return.
pub type Payload = Result<Vec<u8>, (u16, String)>;

struct CachedRun {
    signature: String,
    bytes: usize,
    writer_lease: bool,
    run: Arc<NativeRun>,
    used_at: u64,
}

#[derive(Default)]
struct CachedSubscription {
    revision: u64,
    data: Option<Arc<str>>,
    value: Option<Arc<Value>>,
}

#[derive(Clone)]
struct CachedSummary {
    signature: String,
    writer_lease: bool,
    value: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunLifecycle {
    Live,
    Orphaned,
    Finished,
}

#[derive(Debug)]
struct NativeRun {
    id: String,
    summary: Value,
    detail: Value,
    objects: PathBuf,
    chain: PathBuf,
    cache: PathBuf,
}

#[derive(Default, Deserialize)]
struct RunQuery {
    replay_attempt: Option<u64>,
    after_sequence: Option<u64>,
    catalog_before: Option<u64>,
}

pub async fn serve(root: PathBuf, host: &str, port: u16) -> Result<(), String> {
    let root = root.canonicalize().map_err(display_error)?;
    let journals = root.join(".harbor/run-journals");
    let archive = root.join(".harbor/live-archive");
    let cache = std::env::var_os("LIVE_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".harbor/live-cache"));
    fs::create_dir_all(&cache).map_err(display_error)?;
    let revision = Arc::new(AtomicU64::new(1));
    let (changes, _) = broadcast::channel(32);
    let notify_revision = Arc::clone(&revision);
    let notify_changes = changes.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| {
            event.paths.iter().any(|path| {
                path.file_name().is_some_and(|name| {
                    matches!(
                        name.to_str(),
                        Some(
                            "journal.jsonl"
                                | "journal-index.json"
                                | "index.json.gz"
                                | "index.json.zst"
                        )
                    )
                })
            })
        }) {
            let next = notify_revision.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = notify_changes.send(next);
        }
    })
    .map_err(display_error)?;
    if journals.is_dir() {
        watcher
            .watch(&journals, RecursiveMode::Recursive)
            .map_err(display_error)?;
    }
    if archive.is_dir() {
        watcher
            .watch(&archive, RecursiveMode::Recursive)
            .map_err(display_error)?;
    }
    let lease_archive = archive.clone();
    let lease_journals = journals.clone();
    let lease_revision = Arc::clone(&revision);
    let lease_changes = changes.clone();
    tokio::spawn(async move {
        let mut active = source_revisions(&lease_journals, &lease_archive);
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            let current = source_revisions(&lease_journals, &lease_archive);
            if current != active {
                active = current;
                let next = lease_revision.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = lease_changes.send(next);
            }
        }
    });
    let app = App {
        root,
        archive,
        journals,
        cache,
        run_locks: Arc::new(Mutex::new(HashMap::new())),
        summaries: Arc::new(Mutex::new(HashMap::new())),
        details: Arc::new(Mutex::new(HashMap::new())),
        subscription: Arc::new(Mutex::new(CachedSubscription::default())),
        revision,
        ready: Arc::new(AtomicBool::new(false)),
        shutdown: watch::channel(false).0,
        changes,
        _watcher: Some(Arc::new(Mutex::new(watcher))),
    };
    let warm_app = app.clone();
    tokio::spawn(async move {
        loop {
            let app = warm_app.clone();
            let permit = work_limit().acquire_owned().await.expect("work semaphore");
            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                list_runs(&app)
            })
            .await;
            match result {
                Ok(Ok(_)) => {
                    warm_app.ready.store(true, Ordering::Release);
                    break;
                }
                Ok(Err(error)) => eprintln!("live index warmup failed: {error}"),
                Err(error) => eprintln!("live index warmup failed: {error}"),
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
    let shutdown = app.shutdown.clone();
    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/runs", get(runs))
        .route("/v1/runs/{run_id}", get(run))
        .route("/v1/assets/{asset_id}", get(asset))
        .route("/v1/subscribe", get(subscribe))
        // Large replay/detail JSON crawls through the Cloudflare Tunnel at
        // tens of KB/s; gzip cuts it by an order of magnitude. SSE stays
        // identity so keepalives are never held by a compressor buffer.
        .layer(CompressionLayer::new().compress_when(NotForContentType::SSE))
        .with_state(app);
    let listener = TcpListener::bind((host, port))
        .await
        .map_err(display_error)?;
    println!("3720 Rust live gateway listening on http://{host}:{port}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            #[cfg(unix)]
            {
                let mut terminate =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=terminate.recv()=>{} }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            shutdown.send_replace(true);
        })
        .await
        .map_err(display_error)
}

async fn health(State(app): State<App>) -> impl IntoResponse {
    let ready = app.ready.load(Ordering::Acquire);
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({"ok":ready,"runtime":"rust","storage":"segmented-zstd-v2","ready":ready})),
    )
}

static WORK_LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();
static REPLAY_LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();
fn work_limit() -> Arc<Semaphore> {
    WORK_LIMIT
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
}
async fn blocking_response(work: impl FnOnce() -> Response + Send + 'static) -> Response {
    let permit = work_limit().acquire_owned().await.expect("work semaphore");
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .unwrap_or_else(|error| error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn runs(State(app): State<App>) -> Response {
    blocking_response(move || runs_response(&app)).await
}

fn runs_response(app: &App) -> Response {
    match list_runs(app) {
        Ok(runs) => json_response(
            StatusCode::OK,
            json!({
                "schema": "benchmark-live-runs-v1",
                "generated_at": now_ms(),
                "runs": runs,
            }),
        ),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn run(
    State(app): State<App>,
    AxumPath(run_id): AxumPath<String>,
    Query(query): Query<RunQuery>,
) -> Response {
    let _replay_permit = if query.replay_attempt.is_some() {
        Some(
            REPLAY_LIMIT
                .get_or_init(|| Arc::new(Semaphore::new(1)))
                .clone()
                .acquire_owned()
                .await
                .expect("replay semaphore"),
        )
    } else {
        None
    };
    blocking_response(move || run_response(&app, &run_id, query)).await
}

fn run_response(app: &App, run_id: &str, query: RunQuery) -> Response {
    payload_response(run_payload(app, run_id, query), "no-store")
}

fn run_payload(app: &App, run_id: &str, query: RunQuery) -> Payload {
    const BAD_REQUEST: u16 = 400;
    const NOT_FOUND: u16 = 404;
    const INTERNAL: u16 = 500;
    if !safe_id(run_id) {
        return Err((BAD_REQUEST, "invalid run id".into()));
    }
    if let Some(attempt_id) = query.replay_attempt {
        return match native_run(app, run_id) {
            Ok(Some(run)) => match native_replay(&run, attempt_id, query.after_sequence) {
                Ok(Some(value)) => json_payload(&value),
                Ok(None) => Err((NOT_FOUND, "unknown replay attempt".into())),
                Err(error) => Err((INTERNAL, error)),
            },
            Ok(None) => archive_payload(
                &app.archive
                    .join("replays")
                    .join(run_id)
                    .join(format!("{attempt_id}.json.gz")),
                "unknown replay attempt",
            ),
            Err(error) => Err((INTERNAL, error)),
        };
    }
    if let Some(before) = query.catalog_before {
        return match native_run(app, run_id)
            .and_then(|run| run.ok_or_else(|| "unknown run".to_owned()))
            .and_then(|run| crate::index::RunIndex::open(&run.chain, &run.cache))
            .and_then(|index| index.catalog(Some(before)))
        {
            Ok((attempts, more)) => json_payload(&catalog_value(&attempts, more)),
            Err(error) => Err((BAD_REQUEST, error)),
        };
    }
    match native_run(app, run_id) {
        Ok(Some(run)) => json_payload(&json!({"schema": "benchmark-live-run-v1", "run": run.detail})),
        Ok(None) => archive_payload(
            &app.archive.join("runs").join(format!("{run_id}.json.gz")),
            "unknown run",
        ),
        Err(error) => Err((INTERNAL, error)),
    }
}

async fn asset(State(app): State<App>, AxumPath(asset_id): AxumPath<String>) -> Response {
    blocking_response(move || {
        payload_response(
            asset_payload(&app, &asset_id),
            "public, max-age=31536000, immutable",
        )
    })
    .await
}

fn asset_payload(app: &App, asset_id: &str) -> Payload {
    if asset_id.len() != 64 || !asset_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err((400, "invalid asset id".into()));
    }
    let archived = storage::resolve(
        &app.archive
            .join("assets")
            .join(format!("{asset_id}.json.gz")),
    );
    if archived.is_file() {
        return read_gzip(&archived).map_err(|error| (500, error));
    }
    let Ok(chains) = fs::read_dir(&app.journals) else {
        return Err((404, "unknown asset".into()));
    };
    for chain in chains.flatten() {
        let object = storage::resolve(
            &chain
                .path()
                .join("objects")
                .join(format!("{asset_id}.json.gz")),
        );
        if object.is_file() {
            return read_gzip(&object).map_err(|error| (500, error));
        }
    }
    Err((404, "unknown asset".into()))
}

fn payload_response(payload: Payload, cache_control: &'static str) -> Response {
    match payload {
        Ok(body) => response(StatusCode::OK, body, cache_control),
        Err((status, error)) => error_response(
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error,
        ),
    }
}

fn json_payload(value: &Value) -> Payload {
    serde_json::to_vec(value).map_err(|error| (500, error.to_string()))
}

fn archive_payload(path: &Path, not_found: &str) -> Payload {
    let path = storage::resolve(path);
    if !path.is_file() {
        return Err((404, not_found.to_owned()));
    }
    read_gzip(&path).map_err(|error| (500, error))
}

#[derive(Default, Deserialize)]
struct SubscribeQuery {
    protocol: Option<u8>,
    run_id: Option<String>,
}

#[derive(Default)]
struct SubscriptionCursor {
    previous: HashMap<String, (String, usize, String)>,
    initialized: bool,
}

impl SubscriptionCursor {
    fn project(&mut self, snapshot: &Value, selected: Option<&str>) -> Option<Value> {
        let mut next = HashMap::new();
        let mut updates = Vec::new();
        for run in snapshot.get("runs")?.as_array()? {
            let id = run.get("id")?.as_str()?.to_owned();
            let mut stable = run.clone();
            stable.as_object_mut()?.remove("observed_at");
            let signature = storage::digest(&serde_json::to_vec(&stable).ok()?);
            let history = run
                .get("score_history")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let history_digest = storage::digest(&serde_json::to_vec(&history).ok()?);
            if self
                .previous
                .get(&id)
                .is_none_or(|(previous, _, _)| previous != &signature)
            {
                let mut update = run.clone();
                if let Some((_, length, digest)) = self.previous.get(&id)
                    && history.len() >= *length
                    && storage::digest(&serde_json::to_vec(&history[..*length]).ok()?) == *digest
                {
                    update.as_object_mut()?.remove("score_history");
                    update["score_history_delta"] = json!(&history[*length..]);
                }
                updates.push(update);
            }
            next.insert(id, (signature, history.len(), history_digest));
        }
        let removed = self
            .previous
            .keys()
            .filter(|id| !next.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        let reset = !self.initialized;
        self.previous = next;
        self.initialized = true;
        if !reset && updates.is_empty() && removed.is_empty() {
            return None;
        }
        let selected=selected.map(|id|json!({"id":id,"revision":snapshot["runs"].as_array().and_then(|runs|runs.iter().find(|run|run["id"].as_str()==Some(id))).and_then(|run|run.get("detail_revision"))}));
        Some(
            json!({"schema":"benchmark-live-subscription-v2","revision":snapshot["revision"],"generated_at":now_ms(),"reset":reset,"runs":updates,"removed":removed,"selected":selected}),
        )
    }
}

async fn subscribe(State(app): State<App>, Query(query): Query<SubscribeQuery>) -> Response {
    if query.run_id.as_ref().is_some_and(|id| !safe_id(id)) {
        return error_response(StatusCode::BAD_REQUEST, "invalid run id");
    }
    let mut shutdown = app.shutdown.subscribe();
    let initial = app.revision.load(Ordering::Relaxed);
    let receiver = app.changes.subscribe();
    let stream_app = app.clone();
    let cursor = Arc::new(Mutex::new(SubscriptionCursor::default()));
    let query = Arc::new(query);
    let changes = BroadcastStream::new(receiver)
        .map(move |value| value.unwrap_or_else(|_| app.revision.load(Ordering::Relaxed)));
    let stream = tokio_stream::once(initial).chain(changes).then(move |revision| {
        let app = stream_app.clone();let cursor=cursor.clone();let query=query.clone();
        async move {
            let permit=work_limit().acquire_owned().await.expect("work semaphore");
            let result = tokio::task::spawn_blocking(move || -> Result<Option<String>,String> {
                let _permit=permit;
                let data = subscription_data(&app,revision)?;
                if query.protocol != Some(2) { return Ok(Some(data.to_string())); }
                let snapshot=app.subscription.lock().map_err(display_error)?.value.clone().ok_or("missing subscription snapshot")?;
                cursor.lock().map_err(display_error)?.project(&snapshot,query.run_id.as_deref()).map(|value|serde_json::to_string(&value).map_err(display_error)).transpose()
            }).await;
            let data=match result {
                Ok(Ok(data))=>data,
                Ok(Err(error))=>Some(json!({"schema":"benchmark-live-error-v1","error":error,"revision":revision}).to_string()),
                Err(error)=>Some(json!({"schema":"benchmark-live-error-v1","error":error.to_string(),"revision":revision}).to_string()),
            };
            data.map(|data|Ok::<Event,Infallible>(Event::default().data(data)))
        }
    }).filter_map(|event|async move{event}).take_until(async move{let _ = shutdown.wait_for(|closed| *closed).await; });
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response();
    response
        .headers_mut()
        .insert("access-control-allow-origin", HeaderValue::from_static("*"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

fn subscription_data(app: &App, revision: u64) -> Result<Arc<str>, String> {
    let mut cached = app
        .subscription
        .lock()
        .map_err(|_| "subscription cache lock poisoned".to_owned())?;
    if cached.revision >= revision
        && let Some(data) = &cached.data
    {
        return Ok(Arc::clone(data));
    }
    let revision = app.revision.load(Ordering::Relaxed).max(revision);
    let value = Arc::new(subscription_value(app, revision)?);
    let data = Arc::<str>::from(serde_json::to_string(value.as_ref()).map_err(display_error)?);
    cached.value = Some(value);
    cached.revision = revision;
    cached.data = Some(Arc::clone(&data));
    Ok(data)
}

fn subscription_value(app: &App, revision: u64) -> Result<Value, String> {
    Ok(json!({
        "schema": "benchmark-live-subscription-v1",
        "generated_at": now_ms(),
        "revision": revision,
        // The dashboard score chart reads score_history straight from the
        // subscription stream; stripping it flattens every series.
        "runs": list_runs(app)?,
    }))
}

fn list_runs(app: &App) -> Result<Vec<Value>, String> {
    let mut by_id = BTreeMap::<String, Value>::new();
    let archived = storage::resolve(&app.archive.join("index.json.gz"));
    if archived.is_file() {
        let value = read_gzip_json(&archived)?;
        if let Some(runs) = value.get("runs").and_then(Value::as_array) {
            for run in runs {
                if let Some(id) = run.get("id").and_then(Value::as_str) {
                    by_id.insert(id.to_owned(), run.clone());
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(&app.journals) {
        for entry in entries.flatten() {
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Some(summary) = native_summary(app, &id)? {
                by_id.insert(id, summary);
            }
        }
    }
    let mut runs = by_id.into_values().collect::<Vec<_>>();
    runs.sort_by(|left, right| {
        let left_key = (
            text(left, "game"),
            !boolean(left, "live"),
            -number(left, "score"),
            text(left, "model"),
        );
        let right_key = (
            text(right, "game"),
            !boolean(right, "live"),
            -number(right, "score"),
            text(right, "model"),
        );
        left_key.cmp(&right_key)
    });
    Ok(runs)
}

fn source_signature(chain: &Path) -> Option<String> {
    let mut signature = String::new();
    for name in [storage::JOURNAL_INDEX, "journal.jsonl", "journal.jsonl.zst"] {
        if let Ok(metadata) = fs::metadata(chain.join(name)) {
            signature.push_str(&format!(
                "{name}:{}:{:?};",
                metadata.len(),
                metadata.modified().ok()
            ));
        }
    }
    if signature.is_empty() {
        return None;
    }
    if std::env::var("LIVE_LEASE_MODE").as_deref() == Ok("heartbeat") {
        signature.push_str(&format!(
            "lease:{}",
            fresh_lease(chain)
                .and_then(|lease| lease
                    .get("segment_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned))
                .unwrap_or_default()
        ));
    }
    Some(signature)
}

fn run_lock(app: &App, id: &str) -> Result<Arc<Mutex<()>>, String> {
    Ok(app
        .run_locks
        .lock()
        .map_err(display_error)?
        .entry(id.to_owned())
        .or_default()
        .clone())
}

fn native_run(app: &App, run_id: &str) -> Result<Option<Arc<NativeRun>>, String> {
    let chain = app.journals.join(run_id);
    let Some(signature) = source_signature(&chain) else {
        return Ok(None);
    };
    let run_lock = run_lock(app, run_id)?;
    let _guard = run_lock.lock().map_err(display_error)?;
    let writer_lease = writer_lease(&chain)?;
    {
        let mut cached = app.details.lock().map_err(display_error)?;
        if let Some(value) = cached.get_mut(run_id)
            && value.signature == signature
            && value.writer_lease == writer_lease
        {
            value.used_at = now_ms();
            return Ok(Some(value.run.clone()));
        }
    }
    let index = crate::index::RunIndex::open(&chain, &app.cache.join(run_id))?;
    let Some(mut run) = build_native_run(app, run_id, true, index.summary_rows()?)? else {
        return Ok(None);
    };
    let (history, count) = index.history()?;
    for value in [&mut run.summary, &mut run.detail] {
        value["score_history"] = json!(history);
        value["score_history_points"] = json!(count);
        value["score_history_sampled"] = json!(count > history.len() as u64);
        if let Some(last) = history
            .iter()
            .rev()
            .find(|point| point["score"].as_i64().unwrap_or(0) > 0)
        {
            value["last_score_elapsed_ms"] = last["elapsed_ms"].clone();
            value["last_score_at"] = last["timestamp_ms"].clone();
        }
    }
    let (attempts, more) = index.catalog(None)?;
    let catalog = catalog_value(&attempts, more);
    run.detail["replay_groups"] = catalog["groups"].clone();
    run.detail["replay_catalog_more"] = json!(more);
    run.detail["replay_catalog_before"] = catalog["before"].clone();
    let bytes = serde_json::to_vec(&run.detail)
        .map_err(display_error)?
        .len()
        .saturating_mul(6);
    let run = Arc::new(run);
    let budget = std::env::var("LIVE_DETAIL_CACHE_MIB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(32)
        * 1024
        * 1024;
    let mut cached = app.details.lock().map_err(display_error)?;
    cached.remove(run_id);
    if bytes <= budget {
        while cached.values().map(|value| value.bytes).sum::<usize>() + bytes > budget {
            let Some(oldest) = cached
                .iter()
                .min_by_key(|(_, value)| value.used_at)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            cached.remove(&oldest);
        }
        cached.insert(
            run_id.to_owned(),
            CachedRun {
                signature,
                bytes,
                writer_lease,
                run: run.clone(),
                used_at: now_ms(),
            },
        );
    }
    Ok(Some(run))
}

fn native_summary(app: &App, run_id: &str) -> Result<Option<Value>, String> {
    let chain = app.journals.join(run_id);
    let Some(signature) = source_signature(&chain) else {
        return Ok(None);
    };
    let writer_lease = writer_lease(&chain)?;
    if let Some(cached) = app.summaries.lock().map_err(display_error)?.get(run_id)
        && cached.signature == signature
        && cached.writer_lease == writer_lease
    {
        return Ok(cached.value.clone());
    }
    let lock = run_lock(app, run_id)?;
    let _guard = lock.lock().map_err(display_error)?;
    let index = crate::index::RunIndex::open(&chain, &app.cache.join(run_id))?;
    let mut value =
        build_native_run(app, run_id, false, index.summary_rows()?)?.map(|run| run.summary);
    if let Some(value) = value.as_mut() {
        let (history, count) = index.history()?;
        if let Some(last) = history
            .iter()
            .rev()
            .find(|point| point["score"].as_i64().unwrap_or(0) > 0)
        {
            value["last_score_elapsed_ms"] = last["elapsed_ms"].clone();
            value["last_score_at"] = last["timestamp_ms"].clone();
        }
        value["score_history_sampled"] = json!(count > history.len() as u64);
        value["score_history_points"] = json!(count);
        value["score_history"] = json!(history);
    }
    app.summaries.lock().map_err(display_error)?.insert(
        run_id.to_owned(),
        CachedSummary {
            signature,
            writer_lease,
            value: value.clone(),
        },
    );
    Ok(value)
}

fn catalog_value(attempts: &[crate::index::IndexedAttempt], more: bool) -> Value {
    let mut groups = Vec::<Value>::new();
    for attempt in attempts {
        let reference = &attempt.context.reference;
        let position = groups
            .iter()
            .position(|group| group["reference"].as_str() == Some(reference));
        let position=position.unwrap_or_else(||{groups.push(json!({"kind":attempt.context.kind,"reference":reference,"title":attempt.context.title,"score":attempt.score,"attempts":[]}));groups.len()-1});
        groups[position]["attempts"].as_array_mut().unwrap().push(json!({"id":attempt.id,"successful":attempt.successful,"score":attempt.score,"status":if !attempt.closed{"running"}else if attempt.successful{"completed"}else{"failed"}}));
    }
    json!({"schema":"benchmark-live-catalog-v1","groups":groups,"more":more,"before":attempts.first().map(|attempt|attempt.id)})
}

#[cfg(test)]
fn append_relevant_rows(
    journal: &Path,
    offset: u64,
    length: u64,
    rows: &mut Vec<Value>,
) -> Result<u64, String> {
    complete_lines(journal, offset, length, |line| {
        if include_live_line(line) {
            rows.push(serde_json::from_str::<Value>(line).map_err(display_error)?);
        }
        Ok(())
    })
}

/// A cursor always points after a durable newline. Incomplete tails are retried
/// on the next append; malformed complete records are errors, never silent loss.
#[cfg(test)]
fn complete_lines(
    path: &Path,
    offset: u64,
    length: u64,
    mut visit: impl FnMut(&str) -> Result<(), String>,
) -> Result<u64, String> {
    let mut file = File::open(path).map_err(display_error)?;
    file.seek(SeekFrom::Start(offset)).map_err(display_error)?;
    let mut reader = BufReader::new(file.take(length.saturating_sub(offset)));
    let mut committed = offset;
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        let count = reader
            .read_until(b'\n', &mut bytes)
            .map_err(display_error)?;
        if count == 0 || !bytes.ends_with(b"\n") {
            break;
        }
        let line = std::str::from_utf8(&bytes).map_err(display_error)?;
        if !line.trim().is_empty() {
            visit(line.trim_end())?;
        }
        committed += count as u64;
    }
    Ok(committed)
}

#[cfg(test)]
const LIVE_PROJECTION: &str = "live.jsonl";
#[cfg(test)]
const LIVE_CURSOR: &str = "live.cursor";

#[cfg(test)]
fn sync_live_projection(chain: &Path, journal_length: u64) -> Result<(), String> {
    sync_projection_from(&chain.join("journal.jsonl"), chain, journal_length)
}

#[cfg(test)]
fn sync_projection_from(
    journal: &Path,
    directory: &Path,
    journal_length: u64,
) -> Result<(), String> {
    let projection = directory.join(LIVE_PROJECTION);
    let cursor_path = directory.join(LIVE_CURSOR);
    let previous = storage::read_json(&cursor_path).ok().and_then(|value| {
        let source = value.get("source_offset")?.as_u64()?;
        let projected = value.get("projection_bytes")?.as_u64()?;
        (source <= journal_length && projected <= fs::metadata(&projection).ok()?.len())
            .then_some((source, projected))
    });
    if let Some((_, bytes)) = previous {
        // Roll back an interrupted append whose cursor was not committed.
        OpenOptions::new()
            .write(true)
            .open(&projection)
            .and_then(|file| file.set_len(bytes))
            .map_err(display_error)?;
    }
    if previous.is_some_and(|(source, _)| source == journal_length) {
        return Ok(());
    }
    let cursor = if let Some((cursor, _)) = previous {
        let mut output = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&projection)
            .map_err(display_error)?;
        let next = project_journal_range(journal, cursor, journal_length, true, &mut output)?;
        output.sync_all().map_err(display_error)?;
        next
    } else {
        let temporary = directory.join(format!(".{LIVE_PROJECTION}.{}.tmp", std::process::id()));
        let mut output = File::create(&temporary).map_err(display_error)?;
        let next = project_journal_range(journal, 0, journal_length, true, &mut output)?;
        output.sync_all().map_err(display_error)?;
        fs::rename(&temporary, &projection).map_err(display_error)?;
        next
    };
    let checkpoint = json!({"schema":"benchmark-projection-cursor-v2", "source_offset":cursor, "projection_bytes":fs::metadata(&projection).map_err(display_error)?.len()});
    storage::atomic_write(
        &cursor_path,
        &serde_json::to_vec(&checkpoint).map_err(display_error)?,
    )
}

#[cfg(test)]
fn project_journal_range(
    journal: &Path,
    offset: u64,
    length: u64,
    include_agent_updates: bool,
    output: &mut File,
) -> Result<u64, String> {
    complete_lines(journal, offset, length, |line| {
        if !line.contains(r#""source":"agent""#)
            || (include_agent_updates && is_visible_agent_line(line))
        {
            serde_json::from_str::<Value>(line).map_err(display_error)?;
            output.write_all(line.as_bytes()).map_err(display_error)?;
            output.write_all(b"\n").map_err(display_error)?;
        }
        Ok(())
    })
}

#[cfg(test)]
fn include_live_line(line: &str) -> bool {
    !line.contains(r#""source":"agent""#) || is_visible_agent_line(line)
}

#[cfg(test)]
fn is_visible_agent_line(line: &str) -> bool {
    line.contains(r#""type":"agent_message""#) || line.contains(r#""type":"experience_updated""#)
}

fn source_revisions(journals: &Path, archive: &Path) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    if let Ok(entries) = fs::read_dir(journals) {
        for entry in entries.flatten() {
            if let Some(signature) = source_signature(&entry.path()) {
                values.insert(
                    entry.file_name().to_string_lossy().into_owned(),
                    format!(
                        "{signature}:{}",
                        writer_lease(&entry.path()).unwrap_or(false)
                    ),
                );
            }
        }
    }
    if let Ok(metadata) = fs::metadata(storage::resolve(&archive.join("index.json.gz"))) {
        values.insert(
            "@archive".into(),
            format!("{}:{:?}", metadata.len(), metadata.modified().ok()),
        );
    }
    values
}

fn writer_lease(chain: &Path) -> Result<bool, String> {
    if std::env::var("LIVE_LEASE_MODE").as_deref() == Ok("heartbeat") {
        return Ok(fresh_lease(chain).is_some());
    }
    let lock = match File::open(chain.join("writer.lock")) {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(display_error(error)),
    };
    match FileExt::try_lock_shared(&lock) {
        Ok(()) => {
            FileExt::unlock(&lock).map_err(display_error)?;
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
        Err(error) => Err(display_error(error)),
    }
}

fn fresh_lease(chain: &Path) -> Option<Value> {
    let value: Value =
        serde_json::from_slice(&fs::read(chain.join("writer-lease.json")).ok()?).ok()?;
    let timestamp = value.get("updated_at_ms")?.as_u64()?;
    (value.get("schema")?.as_str()? == "benchmark-writer-lease-v1"
        && value.get("chain_id")?.as_str()? == chain.file_name()?.to_str()?
        && value.get("healthy")?.as_bool()?
        && timestamp <= now_ms() + 1000
        && now_ms().saturating_sub(timestamp) <= 5000)
        .then_some(value)
}

fn run_lifecycle(rows: &[Value], chain: &Path) -> Result<RunLifecycle, String> {
    let unsealed = rows
        .iter()
        .rposition(|row| row.get("type").and_then(Value::as_str) == Some("segment_registered"))
        .is_some_and(|registered| {
            !rows[registered + 1..]
                .iter()
                .any(|row| row.get("type").and_then(Value::as_str) == Some("segment_finished"))
        });
    if !unsealed {
        return Ok(RunLifecycle::Finished);
    }
    if std::env::var("LIVE_LEASE_MODE").as_deref() == Ok("heartbeat") {
        let segment = rows
            .iter()
            .rev()
            .find(|row| row.get("type").and_then(Value::as_str) == Some("segment_registered"))
            .and_then(|row| row.get("segment_id"))
            .and_then(Value::as_str);
        let matches = fresh_lease(chain)
            .is_some_and(|lease| lease.get("segment_id").and_then(Value::as_str) == segment);
        return Ok(if matches {
            RunLifecycle::Live
        } else {
            RunLifecycle::Orphaned
        });
    }
    Ok(if writer_lease(chain)? {
        RunLifecycle::Live
    } else {
        RunLifecycle::Orphaned
    })
}

fn build_native_run(
    app: &App,
    run_id: &str,
    include_detail: bool,
    rows: Vec<Value>,
) -> Result<Option<NativeRun>, String> {
    let chain = app.journals.join(run_id);
    let journal = storage::journal_source(&chain);
    if !journal.is_file() {
        return Ok(None);
    }
    let registration = rows
        .iter()
        .rev()
        .find(|row| row.get("type").and_then(Value::as_str) == Some("segment_registered"))
        .and_then(|row| row.get("payload"))
        .and_then(Value::as_object);
    let Some(registration) = registration else {
        return Ok(None);
    };
    let model = registration
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    if model.is_none() {
        return Ok(None);
    }
    let task = registration
        .get("task")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    if !matches!(
        task.as_str(),
        "parabox-intro"
            | "swarm-farming"
            | "sausage-roll"
            | "emergency-operator"
            | "sokoban"
            | "minesweeper"
            | "kitchen-terminal"
    ) {
        return Ok(None);
    }
    let objects = chain.join("objects");
    let mut events = Vec::new();
    let mut activity = Vec::new();
    let mut experience_markdown = None;
    let mut experience_updated_at = None;
    for row in &rows {
        match row.get("source").and_then(Value::as_str) {
            Some("game") => {
                // The disk index supplies only the latest observation and its
                // latest stateful predecessor, never every historical state.
                events.push(materialize_game_event(row, &objects)?);
            }
            Some("agent") if row.get("type").and_then(Value::as_str) == Some("agent_message") => {
                if let Some(text) = row.pointer("/payload/text").and_then(Value::as_str) {
                    activity.push(json!({
                        "timestamp_ms": row.get("source_timestamp_ms").cloned().unwrap_or(Value::Null),
                        "text": text,
                    }));
                }
            }
            Some("agent")
                if row.get("type").and_then(Value::as_str) == Some("experience_updated") =>
            {
                experience_markdown = row
                    .pointer("/payload/markdown")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                experience_updated_at = row.get("source_timestamp_ms").cloned();
            }
            _ => {}
        }
    }
    let state = events
        .iter()
        .rev()
        .find_map(event_state)
        .unwrap_or_else(|| json!({}));
    let (state_score, total, objective) = score(&task, &state, &events, &app.root);
    let score = events
        .last()
        .and_then(|event| event.get("score"))
        .and_then(Value::as_i64)
        .unwrap_or(state_score);
    let lifecycle = run_lifecycle(&rows, &chain)?;
    let live = lifecycle == RunLifecycle::Live;
    let orphaned = lifecycle == RunLifecycle::Orphaned;
    let disposition = rows
        .iter()
        .rev()
        .find(|row| row.get("type").and_then(Value::as_str) == Some("segment_finished"))
        .and_then(|row| row.pointer("/payload/disposition"))
        .and_then(Value::as_str);
    let completed = authoritative_game_terminal(&state) || (score >= total && total > 0);
    let termination = if live {
        json!({"kind": "live", "resumable": false})
    } else if completed {
        json!({"kind": "completed", "resumable": false})
    } else if orphaned {
        json!({
            "kind": "orphaned",
            "resumable": true,
            "reason": "active segment has no recorder lease",
        })
    } else {
        match disposition {
            Some("agent_stopped") => json!({"kind": "agent_stopped", "resumable": false}),
            Some("error") => {
                json!({"kind": "resumable", "resumable": true, "reason": "infrastructure error"})
            }
            Some("cancelled") => {
                json!({"kind": "stopped", "resumable": true, "reason": "cancelled"})
            }
            _ => json!({"kind": "resumable", "resumable": true}),
        }
    };
    let started_at = rows
        .first()
        .and_then(|row| row.get("source_timestamp_ms"))
        .cloned()
        .unwrap_or(Value::Null);
    let finished_at = rows
        .iter()
        .rev()
        .find(|row| row.get("type").and_then(Value::as_str) == Some("segment_finished"))
        .and_then(|row| row.get("source_timestamp_ms"))
        .cloned()
        .unwrap_or(Value::Null);
    let consumed_ms = rows
        .last()
        .and_then(|row| row.get("effective_elapsed_ms"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let latest_sequence = rows
        .last()
        .and_then(|row| row.get("sequence"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut history = Vec::new();
    let mut previous = None;
    for event in &events {
        let value = event.get("score").and_then(Value::as_i64).unwrap_or(0);
        if previous != Some(value) {
            history.push(json!({
                "timestamp_ms": event.get("timestamp_ms").cloned().unwrap_or(Value::Null),
                "elapsed_ms": event.get("effective_elapsed_ms").cloned().unwrap_or(Value::from(0)),
                "score": value,
            }));
            previous = Some(value);
        }
    }
    let last_score = history
        .iter()
        .rev()
        .find(|point| point.get("score").and_then(Value::as_i64).unwrap_or(0) > 0);
    let (game, task_name) = task_labels(&task);
    let agent = registration
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let effort = registration
        .get("effort")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let job = registration
        .get("job_name")
        .and_then(Value::as_str)
        .unwrap_or(run_id);
    let latest = events.last();
    let execution = execution_clock(&rows, live);
    let detail_revision = format!("{latest_sequence}:{}:{}", live, orphaned);
    let summary = json!({
        "id": run_id,
        "job": job,
        "trial": registration.get("trial").cloned().unwrap_or(Value::Null),
        "task_id": task,
        "task": task_name,
        "game": game,
        "model": model.unwrap(),
        "model_id": model.unwrap(),
        "agent": agent,
        "effort": effort,
        "live": live,
        "status": if live {"running"} else if orphaned && !completed {"orphaned"} else {"finished"},
        "termination": termination,
        "sidecar_only": false,
        "score": score,
        "total": total,
        "objective": objective,
        "started_at": started_at,
        "finished_at": finished_at,
        "last_activity_at": latest.and_then(|event| event.get("timestamp_ms")).cloned().unwrap_or(Value::Null),
        "last_score_at": last_score.and_then(|point| point.get("timestamp_ms")).cloned().unwrap_or(Value::Null),
        "last_score_elapsed_ms": last_score.and_then(|point| point.get("elapsed_ms")).cloned().unwrap_or(Value::Null),
        "consumed_ms": consumed_ms,
        "observed_at": now_ms(),
        "latest_sequence": latest_sequence,
        "detail_revision": detail_revision,
        "execution": execution,
        "latest_action": latest.and_then(|event| event.get("action")).cloned().unwrap_or(Value::Null),
        "latest_result": latest.and_then(|event| event.get("result")).cloned().unwrap_or(Value::Null),
        "score_history": history,
    });
    let mut detail = summary.clone();
    if include_detail {
        let detail_object = detail.as_object_mut().expect("summary object");
        detail_object.insert(
            "state_revision".into(),
            json!(storage::digest(
                &serde_json::to_vec(&json!({"state":state,"assets":asset_references(&events)}))
                    .map_err(display_error)?
            )),
        );
        detail_object.insert("state".into(), state);
        detail_object.insert("asset_refs".into(), asset_references(&events));
        detail_object.insert("replay_groups".into(), json!([]));
        detail_object.insert(
            "recent_activity".into(),
            json!(activity.iter().rev().take(20).cloned().collect::<Vec<_>>()),
        );
        detail_object.insert("live_replay".into(), latest_live_replay(&events, &objects)?);
        detail_object.insert(
            "agent_experience".into(),
            experience_markdown
                .as_deref()
                .map(|markdown| experience(markdown, experience_updated_at))
                .unwrap_or_else(|| serde_json::from_str(EMPTY_EXPERIENCE).expect("experience")),
        );
    }
    Ok(Some(NativeRun {
        id: run_id.to_owned(),
        summary,
        detail,
        objects,
        chain: chain.clone(),
        cache: app.cache.join(run_id),
    }))
}

fn execution_clock(rows: &[Value], live: bool) -> Value {
    let anchor = rows.iter().rev().find(|row| {
        matches!(
            row.get("type").and_then(Value::as_str),
            Some(
                "agent_execution_started"
                    | "agent_execution_finished"
                    | "segment_registered"
                    | "segment_finished"
            )
        )
    });
    let active = live
        && anchor.is_some_and(|row| {
            row.get("type").and_then(Value::as_str) == Some("agent_execution_started")
        });
    json!({"active":active,"anchor_timestamp_ms":anchor.and_then(|row|row.get("source_timestamp_ms")),"anchor_elapsed_ms":anchor.and_then(|row|row.get("effective_elapsed_ms"))})
}

fn authoritative_game_terminal(state: &Value) -> bool {
    state.pointer("/campaign/complete").and_then(Value::as_bool) == Some(true)
        || state.pointer("/shift/status").and_then(Value::as_str) == Some("complete")
        || state.get("complete").and_then(Value::as_bool) == Some(true)
}

fn latest_live_replay(events: &[Value], objects: &Path) -> Result<Value, String> {
    for (index, event) in events.iter().enumerate().rev() {
        if event.get("instruction_trace").is_some() {
            let mut materialized = events.to_vec();
            materialize_descriptor(&mut materialized[index], "instruction_trace", objects)?;
            materialize_descriptor(&mut materialized[index], "state_snapshot", objects)?;
            let mut projection = replay_projection(&materialized, index, index + 1)?;
            projection["sequence"] = event.get("sequence").cloned().unwrap_or(Value::Null);
            return Ok(projection);
        }
        let command = event
            .pointer("/action/command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !["show", "inspect", "status", "submit"].contains(&command.as_str()) {
            break;
        }
    }
    let start = events.len().saturating_sub(60);
    if start == events.len() {
        return Ok(Value::Null);
    }
    let mut projection = replay_projection(&events[start..], 0, events.len() - start)?;
    projection["sequence"] = events
        .last()
        .and_then(|event| event.get("sequence"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(projection)
}

fn asset_references(events: &[Value]) -> Value {
    let mut references = Map::new();
    for event in events {
        let Some(assets) = event.get("assets").and_then(Value::as_object) else {
            continue;
        };
        for (name, descriptor) in assets {
            let Some(object) = descriptor.get("object").and_then(Value::as_str) else {
                continue;
            };
            references.insert(
                name.clone(),
                json!({
                    "id": object,
                    "media_type": "application/json",
                    "bytes": descriptor
                        .get("uncompressed_bytes")
                        .cloned()
                        .unwrap_or(Value::from(0)),
                }),
            );
        }
    }
    Value::Object(references)
}

fn native_replay(
    run: &NativeRun,
    attempt_id: u64,
    after: Option<u64>,
) -> Result<Option<Value>, String> {
    let index = crate::index::RunIndex::open(&run.chain, &run.cache)?;
    let Some(attempt) = index.attempt(attempt_id)? else {
        return Ok(None);
    };
    let (rows, next) = index.game_window(attempt.start, attempt.end, after)?;
    if rows.is_empty() {
        return Err("replay cursor is outside this attempt".into());
    }
    let mut input = Vec::new();
    if let Some(anchor) = index.anchor(rows[0]["sequence"].as_u64().unwrap())? {
        input.push(materialize_game_event(&anchor, &run.objects)?);
    }
    let start = input.len();
    for row in &rows {
        let mut event = materialize_game_event(row, &run.objects)?;
        materialize_descriptor(&mut event, "instruction_trace", &run.objects)?;
        input.push(event);
    }
    let mut projection = replay_projection(&input, start, input.len())?;
    drop(input);
    let activity = index.activity(
        rows.first()
            .and_then(|row| row["source_timestamp_ms"].as_u64()),
        rows.last()
            .and_then(|row| row["source_timestamp_ms"].as_u64()),
    )?;
    let metadata = json!({"schema":"benchmark-live-attempt-replay-v2","run_id":run.id,"attempt_id":attempt_id,
        "kind":attempt.context.kind,"reference":attempt.context.reference,"title":attempt.context.title,"score":attempt.score,"successful":attempt.successful,
        "asset_refs":run.detail["asset_refs"],"activity":activity,"next_after_sequence":next,"page_after_sequence":after});
    projection
        .as_object_mut()
        .unwrap()
        .extend(metadata.as_object().unwrap().clone());
    Ok(Some(projection))
}

fn materialize_game_event(row: &Value, objects: &Path) -> Result<Value, String> {
    let mut event = row
        .get("payload")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    event.insert(
        "sequence".into(),
        row.get("sequence").cloned().unwrap_or(Value::Null),
    );
    event.insert(
        "timestamp_ms".into(),
        row.get("source_timestamp_ms")
            .cloned()
            .unwrap_or(Value::Null),
    );
    event.insert(
        "effective_elapsed_ms".into(),
        row.get("effective_elapsed_ms")
            .cloned()
            .unwrap_or(Value::from(0)),
    );
    let mut value = Value::Object(event);
    if let Some(snapshot) = object_json(value.get("state_snapshot"), objects)? {
        value["state"] = snapshot;
    }
    if event_state(&value).is_none()
        && let Some(trace) = object_json(value.get("instruction_trace"), objects)?
        && let Some(state) = trace
            .as_array()
            .and_then(|frames| frames.last())
            .and_then(|frame| frame.get("state"))
    {
        value["state"] = state.clone();
    }
    Ok(value)
}

fn materialize_descriptor(value: &mut Value, key: &str, objects: &Path) -> Result<(), String> {
    if let Some(bytes) = storage::object_bytes(&value[key], objects)? {
        let decoded: Value = serde_json::from_slice(&bytes).map_err(display_error)?;
        match key {
            "instruction_trace" => {
                value["steps"] = decoded;
            }
            "state_snapshot" => {
                value["state"] = decoded;
            }
            _ => {}
        }
        value.as_object_mut().unwrap().remove(key);
    }
    Ok(())
}

fn object_json(descriptor: Option<&Value>, objects: &Path) -> Result<Option<Value>, String> {
    let Some(descriptor) = descriptor else {
        return Ok(None);
    };
    storage::object_bytes(descriptor, objects)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(display_error))
        .transpose()
}

fn event_state(event: &Value) -> Option<Value> {
    event
        .get("state")
        .filter(|value| value.as_object().is_some_and(|object| !object.is_empty()))
        .cloned()
}

fn score(task: &str, state: &Value, events: &[Value], root: &Path) -> (i64, i64, String) {
    match task {
        "parabox-intro" => {
            let score = state
                .pointer("/campaign/score")
                .or_else(|| state.pointer("/campaign/solved"))
                .and_then(Value::as_i64)
                .or_else(|| {
                    events
                        .last()
                        .and_then(|event| event.get("score"))
                        .and_then(Value::as_i64)
                })
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/total")
                .and_then(Value::as_i64)
                .unwrap_or(364);
            let reference = state
                .pointer("/level/reference")
                .or_else(|| state.pointer("/level/id"))
                .and_then(Value::as_str)
                .or_else(|| {
                    events
                        .last()
                        .and_then(|event| event.get("selected"))
                        .and_then(Value::as_str)
                })
                .unwrap_or_default();
            let title = state
                .pointer("/level/title")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| parabox_title(root, reference))
                .unwrap_or_else(|| "Waiting for state".into());
            (
                score,
                total,
                if reference.is_empty() {
                    title
                } else {
                    format!("{reference} / {title}")
                },
            )
        }
        "sausage-roll" => {
            let score = state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/total")
                .and_then(Value::as_i64)
                .unwrap_or(86);
            let objective = if state.get("mode").and_then(Value::as_str) == Some("overworld") {
                "Land's End / 大地图".into()
            } else {
                let id = state
                    .pointer("/level/id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let title = state
                    .pointer("/level/title")
                    .and_then(Value::as_str)
                    .unwrap_or("Waiting for state");
                if id.is_empty() {
                    title.into()
                } else {
                    format!("{id} / {title}")
                }
            };
            (score, total, objective)
        }
        "emergency-operator" => {
            let score = state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/max_score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let status = state
                .pointer("/shift/status")
                .and_then(Value::as_str)
                .unwrap_or("not_started");
            (
                score,
                total,
                if status == "not_started" {
                    "Start shift".into()
                } else {
                    "Monitor dispatch".into()
                },
            )
        }
        "sokoban" => {
            let score = state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/max_score")
                .and_then(Value::as_i64)
                .unwrap_or(305);
            let id = state
                .pointer("/level/id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let title = state
                .pointer("/level/title")
                .and_then(Value::as_str)
                .unwrap_or("Select a level");
            (
                score,
                total,
                if id.is_empty() {
                    title.into()
                } else {
                    format!("{id} / {title}")
                },
            )
        }
        "minesweeper" => {
            let score = state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total = state
                .pointer("/campaign/max_score")
                .and_then(Value::as_i64)
                .unwrap_or(50);
            let id = state
                .pointer("/level/id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let title = state
                .pointer("/level/title")
                .and_then(Value::as_str)
                .unwrap_or("Select a field");
            (
                score,
                total,
                if id.is_empty() {
                    title.into()
                } else {
                    format!("{id} / {title}")
                },
            )
        }
        "kitchen-terminal" => {
            let score = state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let level = state
                .pointer("/campaign/level")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let scene = state
                .pointer("/campaign/scene")
                .and_then(Value::as_str)
                .unwrap_or("Kitchen shift");
            (score, 338, format!("Level {level} / {scene}"))
        }
        _ => {
            let score = state.get("score").and_then(Value::as_i64).unwrap_or(0);
            (score, 1_000_000, "Make curry".into())
        }
    }
}

fn parabox_title(root: &Path, reference: &str) -> Option<String> {
    let index = root.join("games/parabox-intro/data/campaign/index.tsv");
    fs::read_to_string(index).ok()?.lines().find_map(|line| {
        let mut fields = line.split('\t');
        let id = fields.next()?;
        let title = fields.next()?;
        (id == reference).then(|| title.to_owned())
    })
}

fn task_labels(task: &str) -> (&'static str, &'static str) {
    match task {
        "parabox-intro" => ("parabox", "Patrick's Parabox"),
        "swarm-farming" => ("swarm", "Swarm Farming"),
        "sausage-roll" => ("sausage", "Stephen's Sausage Roll"),
        "emergency-operator" => ("operator", "Emergency Operator"),
        "sokoban" => ("sokoban", "Sokoban Classics"),
        "minesweeper" => ("minesweeper", "No-Guess Minesweeper"),
        "kitchen-terminal" => ("kitchen", "Overcooked Kitchen"),
        _ => ("unknown", "Unknown"),
    }
}

fn experience(markdown: &str, updated_at: Option<Value>) -> Value {
    let mut categories = BTreeMap::<&str, Vec<String>>::from([
        ("plan", Vec::new()),
        ("verified", Vec::new()),
        ("rejected", Vec::new()),
        ("solved", Vec::new()),
    ]);
    let mut heading = String::new();
    for raw in markdown.lines() {
        let line = raw.trim();
        if line.starts_with('#') {
            heading = line.trim_start_matches('#').trim().to_ascii_lowercase();
            continue;
        }
        let Some(item) = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("+ "))
            .map(str::trim)
            .filter(|item| !item.is_empty())
        else {
            continue;
        };
        let lower = item.to_ascii_lowercase();
        let category = if lower.contains("solved")
            || lower.contains("已解")
            || heading.contains("solved")
            || heading.contains("已解")
        {
            "solved"
        } else if [
            "reject",
            "dead-end",
            "dead end",
            "invalid",
            "死路",
            "不可行",
        ]
        .iter()
        .any(|marker| lower.contains(marker) || heading.contains(marker))
        {
            "rejected"
        } else if [
            "verified",
            "confirmed",
            "reusable",
            "mechanic",
            "rule",
            "规律",
            "经验",
            "机制",
        ]
        .iter()
        .any(|marker| lower.contains(marker) || heading.contains(marker))
        {
            "verified"
        } else {
            "plan"
        };
        let normalized = item.split_whitespace().collect::<Vec<_>>().join(" ");
        let normalized = normalized.chars().take(700).collect::<String>();
        let entries = categories.get_mut(category).expect("experience category");
        if !entries.contains(&normalized) {
            entries.push(normalized);
        }
    }
    json!({
        "updated_at": updated_at.unwrap_or(Value::Null),
        "source_count": 1,
        "counts": {
            "plan": categories["plan"].len(),
            "verified": categories["verified"].len(),
            "rejected": categories["rejected"].len(),
            "solved": categories["solved"].len(),
        },
        "plan": categories["plan"].iter().rev().take(8).cloned().collect::<Vec<_>>(),
        "verified": categories["verified"].iter().rev().take(8).cloned().collect::<Vec<_>>(),
        "rejected": categories["rejected"].iter().rev().take(8).cloned().collect::<Vec<_>>(),
        "solved": categories["solved"].iter().rev().take(8).cloned().collect::<Vec<_>>(),
    })
}

fn json_response(status: StatusCode, value: Value) -> Response {
    match serde_json::to_vec(&value) {
        Ok(body) => response(status, body, "no-store"),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

fn error_response(status: StatusCode, error: impl Into<String>) -> Response {
    let body = serde_json::to_vec(&json!({"error": error.into()})).expect("error JSON");
    response(status, body, "no-store")
}

fn response(status: StatusCode, body: Vec<u8>, cache_control: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers.insert("cache-control", HeaderValue::from_static(cache_control));
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    (status, headers, Body::from(body)).into_response()
}

fn read_gzip(path: &Path) -> Result<Vec<u8>, String> {
    storage::read(path)
}

fn read_gzip_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&read_gzip(path)?).map_err(display_error)
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn number(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_trace_only_score_event_updates_summary_and_current_state() {
        let root =
            std::env::temp_dir().join(format!("observer-trace-fallback-{}", std::process::id()));
        let journals = root.join(".harbor/run-journals");
        let chain = journals.join("legacy");
        let objects = chain.join("objects");
        fs::create_dir_all(&objects).unwrap();
        let initial = storage::store_object(
            &objects,
            br#"{"campaign":{"score":0,"total":364},"level":{"reference":"a1","title":"First"}}"#,
        )
        .unwrap();
        let trace=storage::store_object(&objects,br#"[{"state":{"campaign":{"score":1,"total":364},"level":{"reference":"a2","title":"Second"}},"score":1,"score_delta":1}]"#).unwrap();
        let rows = [
            json!({"sequence":1,"source":"runtime","type":"segment_registered","payload":{"task":"parabox-intro","model":"test"}}),
            json!({"sequence":2,"source":"game","type":"game_event","payload":{"action":{"command":"show"},"score":0,"state_snapshot":initial}}),
            json!({"sequence":3,"source":"game","type":"score_changed","payload":{"action":{"command":"move"},"score":1,"score_delta":1,"selected_before":"a1","selected":"a2","instruction_trace":trace}}),
            json!({"sequence":4,"source":"runtime","type":"segment_finished","payload":{}}),
        ];
        fs::write(
            chain.join("journal.jsonl"),
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let watcher = notify::recommended_watcher(|_: notify::Result<notify::Event>| {}).unwrap();
        let app = App {
            root: root.clone(),
            archive: root.join(".harbor/live-archive"),
            journals,
            cache: root.join("cache"),
            run_locks: Arc::new(Mutex::new(HashMap::new())),
            summaries: Arc::new(Mutex::new(HashMap::new())),
            details: Arc::new(Mutex::new(HashMap::new())),
            subscription: Arc::new(Mutex::new(CachedSubscription::default())),
            revision: Arc::new(AtomicU64::new(1)),
            ready: Arc::new(AtomicBool::new(true)),
            shutdown: watch::channel(false).0,
            changes: broadcast::channel(8).0,
            _watcher: Some(Arc::new(Mutex::new(watcher))),
        };
        assert_eq!(native_summary(&app, "legacy").unwrap().unwrap()["score"], 1);
        let run = native_run(&app, "legacy").unwrap().unwrap();
        assert_eq!(run.detail["state"]["campaign"]["score"], 1);
        assert_eq!(run.detail["score"], 1);
        drop(app);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn subscriptions_send_only_changed_runs_and_new_score_points() {
        let mut cursor = SubscriptionCursor::default();
        let mut snapshot = json!({"revision":1,"runs":[{"id":"one","latest_sequence":1,"observed_at":1,"score_history":[{"score":0}]}]});
        assert_eq!(
            cursor.project(&snapshot, Some("one")).unwrap()["reset"],
            true
        );
        snapshot["runs"][0]["observed_at"] = json!(2);
        assert!(cursor.project(&snapshot, Some("one")).is_none());
        snapshot["runs"][0]["latest_sequence"] = json!(2);
        snapshot["runs"][0]["score_history"]
            .as_array_mut()
            .unwrap()
            .push(json!({"score":1}));
        let update = cursor.project(&snapshot, Some("one")).unwrap();
        assert_eq!(update["runs"].as_array().unwrap().len(), 1);
        assert!(update["runs"][0].get("score_history").is_none());
        assert_eq!(
            update["runs"][0]["score_history_delta"],
            json!([{"score":1}])
        );
        snapshot["runs"] = json!([]);
        assert_eq!(
            cursor.project(&snapshot, None).unwrap()["removed"],
            json!(["one"])
        );
    }

    #[test]
    fn summary_scan_keeps_visible_agent_rows_and_only_reads_the_append() {
        use std::io::Write as _;

        let path = std::env::temp_dir().join(format!(
            "gateway-summary-{}-{}.jsonl",
            std::process::id(),
            now_ms()
        ));
        let first = [
            json!({"sequence": 1, "source": "runtime", "type": "chain_created"}),
            json!({"sequence": 2, "source": "agent", "type": "agent_action", "payload": {"tool": "shell"}}),
            json!({"sequence": 3, "source": "agent", "type": "agent_message", "payload": {"text": "visible reasoning"}}),
            json!({"sequence": 4, "source": "game", "type": "game_event"}),
        ]
        .into_iter()
        .map(|row| serde_json::to_string(&row).expect("row"))
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        fs::write(&path, first).expect("initial journal");
        let first_length = fs::metadata(&path).expect("metadata").len();
        let mut rows = Vec::new();
        append_relevant_rows(&path, 0, first_length, &mut rows).expect("initial scan");
        assert_eq!(
            rows.iter()
                .filter_map(|row| row.get("sequence").and_then(Value::as_u64))
                .collect::<Vec<_>>(),
            [1, 3, 4]
        );

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("append journal");
        for row in [
            json!({"sequence": 5, "source": "agent", "type": "agent_action", "payload": {"tool": "shell"}}),
            json!({"sequence": 6, "source": "runtime", "type": "segment_finished"}),
        ] {
            writeln!(file, "{}", serde_json::to_string(&row).expect("row")).expect("append row");
        }
        let final_length = fs::metadata(&path).expect("metadata").len();
        append_relevant_rows(&path, first_length, final_length, &mut rows)
            .expect("incremental scan");
        assert_eq!(
            rows.iter()
                .filter_map(|row| row.get("sequence").and_then(Value::as_u64))
                .collect::<Vec<_>>(),
            [1, 3, 4, 6]
        );
        fs::remove_file(path).expect("remove journal");
    }

    #[test]
    fn live_projection_drops_historical_agent_flood_and_tails_visible_messages() {
        use std::io::Write as _;

        let chain = std::env::temp_dir().join(format!(
            "gateway-projection-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&chain).expect("chain");
        let journal = chain.join("journal.jsonl");
        let mut file = File::create(&journal).expect("journal");
        writeln!(
            file,
            "{}",
            json!({"sequence": 1, "source": "runtime", "type": "chain_created"})
        )
        .expect("runtime");
        for sequence in 2..1_002 {
            writeln!(
                file,
                "{}",
                json!({"sequence": sequence, "source": "agent", "type": "agent_action"})
            )
            .expect("agent flood");
        }
        writeln!(
            file,
            "{}",
            json!({"sequence": 1_002, "source": "agent", "type": "agent_message"})
        )
        .expect("historical message");
        writeln!(
            file,
            "{}",
            json!({"sequence": 1_003, "source": "game", "type": "game_event"})
        )
        .expect("game");
        file.sync_all().expect("sync");
        let initial_length = fs::metadata(&journal).expect("metadata").len();
        sync_live_projection(&chain, initial_length).expect("initial projection");
        let initial = fs::read_to_string(chain.join(LIVE_PROJECTION)).expect("projection");
        assert_eq!(initial.lines().count(), 3);
        assert!(!initial.contains("agent_action"));
        assert!(initial.contains("agent_message"));

        let mut file = OpenOptions::new()
            .append(true)
            .open(&journal)
            .expect("append");
        writeln!(
            file,
            "{}",
            json!({"sequence": 1_004, "source": "agent", "type": "agent_action"})
        )
        .expect("new action");
        writeln!(
            file,
            "{}",
            json!({"sequence": 1_005, "source": "agent", "type": "agent_message"})
        )
        .expect("new message");
        let final_length = fs::metadata(&journal).expect("metadata").len();
        sync_live_projection(&chain, final_length).expect("incremental projection");
        let final_projection = fs::read_to_string(chain.join(LIVE_PROJECTION)).expect("projection");
        assert_eq!(final_projection.lines().count(), 4);
        assert!(!final_projection.contains(r#""sequence":1004"#));
        assert!(final_projection.contains(r#""sequence":1005"#));
        fs::remove_dir_all(chain).expect("cleanup");
    }

    #[test]
    fn shared_assets_become_content_addressed_references() {
        let references = asset_references(&[json!({
            "assets": {
                "overworld_map": {
                    "encoding": "gzip",
                    "object": "abc123",
                    "uncompressed_bytes": 4096
                }
            }
        })]);
        assert_eq!(references["overworld_map"]["id"], "abc123");
        assert_eq!(references["overworld_map"]["bytes"], 4096);
        assert_eq!(
            references["overworld_map"]["media_type"],
            "application/json"
        );
    }

    #[test]
    fn experience_uses_only_explicit_markdown_bullets() {
        let value = experience(
            "# Current plan\n- try the west route\n# Verified mechanics\n- confirmed: boxes preserve direction\n# Rejected\n- dead-end at the north wall",
            Some(Value::from(42)),
        );
        assert_eq!(value["counts"]["plan"], 1);
        assert_eq!(value["counts"]["verified"], 1);
        assert_eq!(value["counts"]["rejected"], 1);
        assert_eq!(value["updated_at"], 42);
    }

    #[test]
    fn safe_ids_reject_paths() {
        assert!(safe_id("parabox-run_1"));
        assert!(!safe_id("../journal"));
        assert!(!safe_id("run/child"));
    }

    #[test]
    fn unsealed_segment_requires_a_live_writer_lease() {
        let chain = std::env::temp_dir().join(format!(
            "gateway-writer-lease-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&chain).expect("chain");
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(chain.join("writer.lock"))
            .expect("writer lock");
        let registered = vec![json!({"type": "segment_registered"})];

        assert_eq!(
            run_lifecycle(&registered, &chain).expect("orphaned lifecycle"),
            RunLifecycle::Orphaned
        );
        FileExt::try_lock_exclusive(&lock).expect("active writer lease");
        assert_eq!(
            run_lifecycle(&registered, &chain).expect("live lifecycle"),
            RunLifecycle::Live
        );
        assert_eq!(
            run_lifecycle(
                &[
                    json!({"type": "segment_registered"}),
                    json!({"type": "segment_finished"}),
                ],
                &chain,
            )
            .expect("finished lifecycle"),
            RunLifecycle::Finished
        );

        FileExt::unlock(&lock).expect("unlock writer lease");
        fs::remove_dir_all(chain).expect("cleanup");
    }

    #[test]
    fn sokoban_uses_campaign_score_and_level_objective() {
        let state = json!({
            "campaign": {"score": 25, "max_score": 305},
            "level": {"id": "novoban-025", "title": "Novoban 25"}
        });
        assert_eq!(
            score("sokoban", &state, &[], Path::new("/missing")),
            (25, 305, "novoban-025 / Novoban 25".into())
        );
        assert_eq!(task_labels("sokoban"), ("sokoban", "Sokoban Classics"));
    }

    #[test]
    fn minesweeper_uses_campaign_score_and_level_objective() {
        let state = json!({
            "campaign": {"score": 2, "max_score": 12},
            "level": {"id": "cadet-02", "title": "First Perimeter"}
        });
        assert_eq!(
            score("minesweeper", &state, &[], Path::new("/missing")),
            (2, 12, "cadet-02 / First Perimeter".into())
        );
        assert_eq!(
            task_labels("minesweeper"),
            ("minesweeper", "No-Guess Minesweeper")
        );
    }

    #[test]
    fn kitchen_uses_shift_score_and_scene_objective() {
        let state = json!({
            "campaign": {"score": 52, "level": 1, "scene": "Skyscraper_Test_1p"}
        });
        assert_eq!(
            score("kitchen-terminal", &state, &[], Path::new("/missing")),
            (52, 338, "Level 1 / Skyscraper_Test_1p".into())
        );
        assert_eq!(
            task_labels("kitchen-terminal"),
            ("kitchen", "Overcooked Kitchen")
        );
    }

    #[test]
    fn partial_score_campaign_terminal_is_not_an_agent_stop() {
        assert!(authoritative_game_terminal(&json!({
            "campaign": {"complete": true, "score": 1, "max_score": 5}
        })));
        assert!(!authoritative_game_terminal(&json!({
            "campaign": {"complete": false, "score": 1, "max_score": 5}
        })));
        assert!(authoritative_game_terminal(&json!({
            "shift": {"status": "complete"}
        })));
    }
}
