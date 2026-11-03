//! In-memory WebDAV server on loopback for the stub-backed sync tests (`web_api_sync.rs`,
//! `web_api_sync_auto.rs`). Pulled in with `#[path]` by those test binaries alone, so other
//! binaries do not compile it. The dead-code allow is on the `mod` of `web_api_sync_auto.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicU16, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};

/// One request the stub received.
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub path: String,
    /// `Authorization` header, empty when absent.
    pub auth: String,
    /// `x-amz-date` header (S3), empty when absent.
    pub amz_date: String,
}

struct StubState {
    /// In-memory remote: request path -> stored body (PUT writes it, GET / HEAD read it).
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    /// Collections that exist, without a trailing slash. `/dav` (the base URL of the tests)
    /// exists from the start; MKCOL adds the rest. PUT needs its parent in here.
    collections: Mutex<BTreeSet<String>>,
    seen: Mutex<Vec<Seen>>,
    /// Held by every PUT / GET / HEAD before it answers; widens the window in which two
    /// unserialised operations would overlap. No assertion depends on it elapsing.
    transfer_delay: Duration,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    /// Status every PUT answers with instead of storing the body; 0 = behave normally.
    put_status: AtomicU16,
}

struct InFlight<'a>(&'a StubState);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Path-style S3 bucket URL of `s3_settings()` against a custom endpoint: `HEAD /bucket/`.
pub const S3_BUCKET_PATH: &str = "/bucket/";

/// Where the objects of that bucket live: S3 has no collections, so a PUT under it needs no
/// parent (a WebDAV PUT without one is 409).
const S3_OBJECT_PREFIX: &str = S3_BUCKET_PATH;

/// The moshi-hook probes loopback listeners with `GET /` about every 16 s: that request is
/// answered 404 and never recorded. Everything else is, and the assertions filter by method
/// and path.
async fn stub_handler(State(st): State<Arc<StubState>>, req: Request<Body>) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let method = parts.method.as_str().to_string();
    let path = parts.uri.path().to_string();
    let reply = |status: StatusCode| Response::builder().status(status);
    if method == "GET" && path == "/" {
        return reply(StatusCode::NOT_FOUND).body(Body::empty()).unwrap();
    }
    let header_of = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    st.seen.lock().unwrap().push(Seen {
        method: method.clone(),
        path: path.clone(),
        auth: header_of("authorization"),
        amz_date: header_of("x-amz-date"),
    });

    let now = st.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    st.max_in_flight.fetch_max(now, Ordering::SeqCst);
    let _in_flight = InFlight(&st);
    if matches!(method.as_str(), "PUT" | "GET" | "HEAD") && !st.transfer_delay.is_zero() {
        tokio::time::sleep(st.transfer_delay).await;
    }

    let stored = st.files.lock().unwrap().get(&path).cloned();
    // Like a real server: MKCOL is 405 on an existing collection and 409 without its parent,
    // PUT is 409 without its parent collection, PROPFIND is 404 on what does not exist.
    let key = path.trim_end_matches('/').to_string();
    let parent_exists = |key: &str| {
        let parent = key.rsplit_once('/').map_or("", |(parent, _)| parent);
        parent.is_empty() || st.collections.lock().unwrap().contains(parent)
    };
    match method.as_str() {
        "PROPFIND" if stored.is_some() || st.collections.lock().unwrap().contains(&key) => {
            reply(StatusCode::MULTI_STATUS).body(Body::empty())
        }
        "PROPFIND" => reply(StatusCode::NOT_FOUND).body(Body::empty()),
        "MKCOL" if st.collections.lock().unwrap().contains(&key) => {
            reply(StatusCode::METHOD_NOT_ALLOWED).body(Body::empty())
        }
        "MKCOL" if !parent_exists(&key) => reply(StatusCode::CONFLICT).body(Body::empty()),
        "MKCOL" => {
            st.collections.lock().unwrap().insert(key);
            reply(StatusCode::CREATED).body(Body::empty())
        }
        "PUT" if st.put_status.load(Ordering::SeqCst) != 0 => {
            let status = st.put_status.load(Ordering::SeqCst);
            reply(StatusCode::from_u16(status).unwrap()).body(Body::empty())
        }
        "PUT" if !parent_exists(&key) && !key.starts_with(S3_OBJECT_PREFIX) => {
            reply(StatusCode::CONFLICT).body(Body::empty())
        }
        "PUT" => {
            st.files.lock().unwrap().insert(path, bytes.to_vec());
            reply(StatusCode::CREATED).body(Body::empty())
        }
        "GET" => match stored {
            Some(b) => reply(StatusCode::OK)
                .header("etag", "\"stub-etag\"")
                .body(Body::from(b)),
            None => reply(StatusCode::NOT_FOUND).body(Body::empty()),
        },
        "HEAD" if stored.is_some() || path == S3_BUCKET_PATH => reply(StatusCode::OK)
            .header("etag", "\"stub-etag\"")
            .body(Body::empty()),
        "HEAD" => reply(StatusCode::NOT_FOUND).body(Body::empty()),
        _ => reply(StatusCode::METHOD_NOT_ALLOWED).body(Body::empty()),
    }
    .unwrap()
}

/// In-memory WebDAV server on loopback (also answers an S3 `HEAD /bucket/`), speaking the
/// methods `services/webdav.rs` uses: PROPFIND, MKCOL, PUT, GET, HEAD.
pub struct DavStub {
    port: u16,
    state: Arc<StubState>,
    task: tokio::task::JoinHandle<()>,
}

impl DavStub {
    pub async fn start() -> Self {
        Self::start_with_delay(Duration::ZERO).await
    }

    pub async fn start_with_delay(transfer_delay: Duration) -> Self {
        let state = Arc::new(StubState {
            files: Mutex::new(BTreeMap::new()),
            collections: Mutex::new(BTreeSet::from(["/dav".to_string()])),
            seen: Mutex::new(Vec::new()),
            transfer_delay,
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
            put_status: AtomicU16::new(0),
        });
        let app = axum::Router::new()
            .fallback(stub_handler)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { port, state, task }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/dav", self.port)
    }

    /// S3 `endpoint` value pointing at the stub (path style, plain http).
    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn requests(&self, method: &str) -> Vec<Seen> {
        self.state
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == method)
            .cloned()
            .collect()
    }

    /// Authorization headers of the `PROPFIND /dav` requests received so far.
    pub fn propfind_auth(&self) -> Vec<String> {
        self.requests("PROPFIND")
            .into_iter()
            .filter(|r| r.path == "/dav")
            .map(|r| r.auth)
            .collect()
    }

    /// `(method, path)` of everything under `/dav`, in arrival order.
    pub fn dav_log(&self) -> Vec<(String, String)> {
        self.state
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/dav" || r.path.starts_with("/dav/"))
            .map(|r| (r.method.clone(), r.path.clone()))
            .collect()
    }

    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.state.files.lock().unwrap().get(path).cloned()
    }

    /// Every PUT from now on answers `status` (and stores nothing); `None` restores normal
    /// behaviour. The request is still recorded.
    #[allow(dead_code)] // used by `web_api_sync_auto.rs` only
    pub fn fail_puts_with(&self, status: Option<u16>) {
        self.state
            .put_status
            .store(status.unwrap_or(0), Ordering::SeqCst);
    }

    pub fn forget_requests(&self) {
        self.state.seen.lock().unwrap().clear();
        self.state.max_in_flight.store(0, Ordering::SeqCst);
    }

    pub fn max_in_flight(&self) -> usize {
        self.state.max_in_flight.load(Ordering::SeqCst)
    }
}

impl Drop for DavStub {
    fn drop(&mut self) {
        self.task.abort();
    }
}
