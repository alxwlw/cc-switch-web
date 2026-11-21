mod support;

use axum::http::StatusCode;
use base64::Engine;
use serde_json::json;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

const URI: &str = "/api/custom-icons";

/// Real PNG (the service fully decodes), `shade` makes distinct images.
fn png_data_uri(shade: u8) -> String {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[shade; 16]).unwrap();
    }
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(out)
    )
}

#[tokio::test]
async fn add_list_rename_delete_roundtrip_keeps_data_uri_verbatim() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();
    let (a, b) = (png_data_uri(10), png_data_uri(200));

    let (status, body) = web_call(
        &router,
        "POST",
        URI,
        Some(json!({"name": "  first ", "data": a})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["data"]["existed"], false);
    assert_eq!(body["data"]["icon"]["name"], "first");
    assert_eq!(body["data"]["icon"]["data"], a);
    let id_a = body["data"]["icon"]["id"].as_str().unwrap().to_string();
    assert!(body["data"]["icon"]["createdAt"].as_i64().unwrap() > 0);

    let (_, body) = web_call(
        &router,
        "POST",
        URI,
        Some(json!({"name": "second", "data": b})),
    )
    .await;
    let id_b = body["data"]["icon"]["id"].as_str().unwrap().to_string();
    assert_ne!(id_a, id_b);

    // newest first
    let (status, list) = web_call(&router, "GET", URI, None).await;
    assert_eq!(status, StatusCode::OK);
    let list = list["data"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["id"], id_b.as_str());
    assert_eq!(list[1]["id"], id_a.as_str());
    assert_eq!(list[1]["data"], a);

    let (status, body) = web_call(
        &router,
        "PUT",
        &format!("{URI}/{id_a}"),
        Some(json!({"name": "renamed"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["id"], id_a.as_str());
    assert_eq!(body["data"]["name"], "renamed");
    assert_eq!(body["data"]["data"], a);

    let (status, body) = web_call(&router, "DELETE", &format!("{URI}/{id_a}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"], true);
    let (_, list) = web_call(&router, "GET", URI, None).await;
    let list = list["data"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], id_b.as_str());
    assert_eq!(list[0]["name"], "second");
}

// Desktop contract: same image again is not an error, the original entry comes back.
#[tokio::test]
async fn adding_same_image_returns_existing_entry_with_original_name() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();
    let a = png_data_uri(10);

    let (_, first) = web_call(
        &router,
        "POST",
        URI,
        Some(json!({"name": "orig", "data": a})),
    )
    .await;
    let (status, again) = web_call(
        &router,
        "POST",
        URI,
        Some(json!({"name": "other", "data": a})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["data"]["existed"], true);
    assert_eq!(again["data"]["icon"]["id"], first["data"]["icon"]["id"]);
    assert_eq!(again["data"]["icon"]["name"], "orig");
    let (_, list) = web_call(&router, "GET", URI, None).await;
    assert_eq!(list["data"].as_array().unwrap().len(), 1);
}

// Service validation errors are server-side `fail` (500) like every sibling handler;
// bad_request is only for unparsable app/path input. Messages pass through verbatim.
#[tokio::test]
async fn validation_errors_pass_service_message_verbatim() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();
    let a = png_data_uri(10);

    let (status, body) = web_call(
        &router,
        "POST",
        URI,
        Some(json!({"name": "   ", "data": a})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "请给图标起个名字 (Please name the icon)");

    let (status, body) = web_call(
        &router,
        "POST",
        URI,
        Some(json!({"name": "x", "data": "data:image/png;base64,!!!"})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body["error"],
        "自定义图标无效，请重新上传 (The custom icon is invalid, please upload it again)"
    );

    let (status, body) = web_call(
        &router,
        "PUT",
        &format!("{URI}/nope"),
        Some(json!({"name": "n"})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body["error"],
        "这个图标已经不在「我的图标」里了 (This icon is no longer in My Icons)"
    );

    let (_, list) = web_call(&router, "GET", URI, None).await;
    assert_eq!(list["data"].as_array().unwrap().len(), 0);
}

// Desktop contract: deleting an id that is gone is a no-op success.
#[tokio::test]
async fn delete_missing_icon_is_noop_success_and_keeps_others() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();
    web_call(
        &router,
        "POST",
        URI,
        Some(json!({"name": "keep", "data": png_data_uri(10)})),
    )
    .await;

    let (status, body) = web_call(&router, "DELETE", &format!("{URI}/missing"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    let (_, list) = web_call(&router, "GET", URI, None).await;
    assert_eq!(list["data"][0]["name"], "keep");
}
