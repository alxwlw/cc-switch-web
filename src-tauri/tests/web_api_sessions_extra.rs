mod support;

use axum::http::{header, Request, StatusCode};
use base64::Engine;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_call_raw, web_router};
use tower::ServiceExt;

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2, 3, 4, 5, 250, 251, 252];

/// One Claude session (`~/.claude/projects/p/s.jsonl`): user text, assistant reply, and a
/// user turn with an inline base64 image (line 3). Returns (path, image ref JSON).
fn prepare_session(n_turns: usize, media_type: &str) -> (String, Value) {
    let home = ensure_test_home();
    let dir = home.join(".claude/projects/p");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.jsonl");
    let data = base64::engine::general_purpose::STANDARD.encode(PNG);
    let mut body = String::new();
    for i in 0..n_turns {
        body.push_str(
            &json!({"type":"user","uuid":format!("u{i}"),"timestamp":"2026-10-03T10:00:00Z",
                "message":{"role":"user","content":format!("question {i}")}})
            .to_string(),
        );
        body.push('\n');
        body.push_str(
            &json!({"type":"assistant","uuid":format!("a{i}"),"timestamp":"2026-10-03T10:00:01Z",
                "message":{"id":format!("m{i}"),"role":"assistant","model":"claude-opus-5-5",
                    "content":[{"type":"text","text":format!("answer {i}")}]}})
            .to_string(),
        );
        body.push('\n');
    }
    let offset = body.len();
    let image_line = json!({"type":"user","uuid":"uimg","timestamp":"2026-10-03T10:00:02Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"look"},
            {"type":"image","source":{"type":"base64","media_type":media_type,"data":data}}]}})
    .to_string();
    body.push_str(&image_line);
    body.push('\n');
    std::fs::write(&path, body).unwrap();
    let image = json!({
        "source": {"kind":"inline","content":{"kind":"jsonl","offset":offset,
            "len":image_line.len(),"pointer":"/message/content/1/source/data"}},
        "mediaType": media_type,
        "size": PNG.len(),
    });
    (path.to_string_lossy().into_owned(), image)
}

fn ndjson(bytes: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(text.ends_with('\n'), "every line is newline-terminated");
    text.lines()
        .map(|l| serde_json::from_str(l).expect("each line is one JSON chunk"))
        .collect()
}

fn image_req(path: &str, image: &Value) -> Value {
    json!({"providerId":"claude","sourcePath":path,"image":image})
}

fn stream_uri(path: &str) -> String {
    format!(
        "/api/sessions/stream?providerId=claude&sourcePath={}",
        urlencode(path)
    )
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test]
#[serial]
async fn image_inline_returns_exact_bytes_and_media_type() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let (path, image) = prepare_session(1, "image/png");
    let router = web_router();
    let (status, headers, bytes) = web_call_raw(
        &router,
        "POST",
        "/api/sessions/image",
        Some(image_req(&path, &image)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(bytes.as_ref(), PNG);
}

#[tokio::test]
#[serial]
async fn image_non_image_media_type_is_octet_stream() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let (path, image) = prepare_session(1, "text/html");
    let router = web_router();
    let (status, headers, bytes) = web_call_raw(
        &router,
        "POST",
        "/api/sessions/image",
        Some(image_req(&path, &image)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(bytes.as_ref(), PNG);
}

#[tokio::test]
#[serial]
async fn image_local_file_outside_allowed_dirs_is_refused() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let (path, _) = prepare_session(1, "image/png");
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("secret.png");
    std::fs::write(&file, PNG).unwrap();
    let image = json!({"source":{"kind":"local_file","path":file.to_string_lossy()},
        "mediaType":"image/png","size":PNG.len()});
    let router = web_router();
    let (status, body) = web_call(
        &router,
        "POST",
        "/api/sessions/image",
        Some(image_req(&path, &image)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert!(body["error"].as_str().is_some_and(|e| !e.is_empty()));
}

#[tokio::test]
#[serial]
async fn image_source_path_outside_roots_is_refused() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let (_, image) = prepare_session(1, "image/png");
    let router = web_router();
    let (status, body) = web_call(
        &router,
        "POST",
        "/api/sessions/image",
        Some(image_req("/etc/passwd", &image)),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
}

#[tokio::test]
#[serial]
async fn stream_source_path_outside_roots_gives_single_error_line() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    prepare_session(1, "image/png");
    let router = web_router();
    let (status, headers, bytes) =
        web_call_raw(&router, "GET", &stream_uri("/etc/passwd"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-ndjson");
    let lines = ndjson(&bytes);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "error");
    assert!(lines[0]["message"].as_str().is_some_and(|m| !m.is_empty()));
}

#[tokio::test]
#[serial]
async fn stream_missing_source_gives_single_error_line_matching_messages_route() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let (path, _) = prepare_session(1, "image/png");
    let missing = format!("{path}.gone");
    let router = web_router();
    let (_, _, bytes) = web_call_raw(&router, "GET", &stream_uri(&missing), None).await;
    let lines = ndjson(&bytes);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "error");
    // The text is the service's, verbatim (same as the one-shot messages route).
    let (_, body) = web_call(
        &router,
        "GET",
        &format!(
            "/api/sessions/messages?providerId=claude&sourcePath={}",
            urlencode(&missing)
        ),
        None,
    )
    .await;
    assert_eq!(lines[0]["message"], body["error"]);
}

#[tokio::test]
#[serial]
async fn stream_prepared_session_is_header_messages_done() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    // 200 turns x 2 messages + image turn: more than one 150-message chunk.
    let (path, _) = prepare_session(200, "image/png");
    let router = web_router();
    let (status, headers, bytes) = web_call_raw(&router, "GET", &stream_uri(&path), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-ndjson");
    let lines = ndjson(&bytes);
    assert_eq!(lines.first().unwrap()["type"], "header");
    assert_eq!(lines.last().unwrap()["type"], "done");
    let total = lines[0]["total"].as_u64().unwrap();
    assert_eq!(total, 401);
    let mut next = 0u64;
    let mut chunks = 0;
    for line in &lines[1..lines.len() - 1] {
        assert_eq!(line["type"], "messages");
        assert_eq!(
            line["start"].as_u64().unwrap(),
            next,
            "starts are contiguous"
        );
        next += line["messages"].as_array().unwrap().len() as u64;
        chunks += 1;
    }
    assert!(chunks >= 2, "multiple chunks, got {chunks}");
    assert_eq!(next, total, "sum of messages equals header.total");
    assert!(lines.last().unwrap()["payloadBytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
#[serial]
async fn stream_client_drop_after_first_frame_does_not_break_router() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let (path, _) = prepare_session(200, "image/png");
    let router = web_router();
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(stream_uri(&path))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = resp.into_body();
    let first = body.frame().await.expect("a first frame").unwrap();
    assert!(first.into_data().is_ok_and(|d| d.ends_with(b"\n")));
    drop(body);
    let (status, headers, bytes) = web_call_raw(&router, "GET", &stream_uri(&path), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-ndjson");
    assert_eq!(ndjson(&bytes).last().unwrap()["type"], "done");
}
