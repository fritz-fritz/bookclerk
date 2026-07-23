//! Wiremock fixtures for GraphicAudio login, products, and links.

use libation_graphicaudio::{
    fetch_title_materials, load_auth, GraphicAudioClient, GraphicAudioSource, LOGIN_PATH,
    PRODUCTS_PATH,
};
use libation_library::LibraryStore;
use libation_source::{ContentSource, LoginOptions, ScanOptions, SourceFetch, SourceKind};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn login_saves_ga_auth_file() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(LOGIN_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "ga-token-xyz"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let source = GraphicAudioSource::with_base_url(server.uri());
    let account = source
        .login(
            dir.path(),
            LoginOptions {
                marketplace: "us".into(),
                label: Some("GA".into()),
                email: Some("reader@example.com".into()),
                password: Some("secret".into()),
                force: true,
            },
        )
        .await
        .unwrap();

    assert_eq!(account.source, SourceKind::GraphicAudio);
    assert_eq!(account.account_id, "reader@example.com");

    let auth = load_auth(&libation_graphicaudio::auth_file_for_account(
        dir.path(),
        Some("GA"),
        "reader@example.com",
    ))
    .unwrap();
    assert_eq!(auth.token, "ga-token-xyz");
    assert!(!auth.client_id.is_empty());
}

#[tokio::test]
async fn scan_skips_samples_upserts_owned() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PRODUCTS_PATH))
        .and(header("authorization", "tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "Id": "1",
                "Type": "sample",
                "ProductName": "Sample Only",
                "Author": "A"
            },
            {
                "Id": "99",
                "Type": "owned",
                "ProductName": "Owned Title",
                "Author": "Ada Author",
                "Series": "Saga",
                "Episode": "2",
                "Genre": "Sci-Fi",
                "Purchased Date": "2024-06-01"
            }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let auth_path = libation_graphicaudio::auth_file_for_account(dir.path(), None, "a@ex.com");
    libation_graphicaudio::save_auth(
        &auth_path,
        &libation_graphicaudio::GraphicAudioAuthFile {
            token: "tok".into(),
            client_id: "dev".into(),
            email: "a@ex.com".into(),
            marketplace: "us".into(),
            label: None,
        },
    )
    .unwrap();

    let db = dir.path().join("library.db");
    let store = LibraryStore::open(&db).unwrap();
    let source = GraphicAudioSource::with_base_url(server.uri());
    let summary = source
        .scan(dir.path(), &store, ScanOptions::default())
        .await
        .unwrap();
    assert_eq!(summary.books_upserted, 1);
    let books = store.list_books(None).unwrap();
    assert_eq!(books.len(), 1);
    assert_eq!(books[0].product_id, "99");
    assert_eq!(books[0].source, "graphicaudio");
}

#[tokio::test]
async fn fetch_title_downloads_hi_mp3() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/links"))
        .and(query_param("product", "99"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Lo": format!("{}/media/lo.mp3", server.uri()),
            "Hi": format!("{}/media/hi.mp3", server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/media/hi.mp3"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ID3\x03hi-audio"))
        .expect(1)
        .mount(&server)
        .await;

    let client = GraphicAudioClient::new(server.uri()).with_token("tok");
    let cache = tempfile::tempdir().unwrap();
    let plain = fetch_title_materials(&client, "99", cache.path())
        .await
        .unwrap();
    assert_eq!(plain.parts.len(), 1);
    let bytes = std::fs::read(&plain.parts[0].path).unwrap();
    assert!(bytes.starts_with(b"ID3"));
}

#[tokio::test]
async fn fetch_title_via_content_source() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/links"))
        .and(query_param("product", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "Hi": format!("{}/m.mp3", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/m.mp3"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ID3x"))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    libation_graphicaudio::save_auth(
        &libation_graphicaudio::auth_file_for_account(dir.path(), None, "u@ex.com"),
        &libation_graphicaudio::GraphicAudioAuthFile {
            token: "tok".into(),
            client_id: "dev".into(),
            email: "u@ex.com".into(),
            marketplace: "us".into(),
            label: None,
        },
    )
    .unwrap();

    let source = GraphicAudioSource::with_base_url(server.uri());
    let cache = dir.path().join("cache");
    let fetch = source
        .fetch_title(
            dir.path(),
            "u@ex.com",
            "7",
            &libation_source::FetchOptions {
                download: libation_source::DownloadOptions::default(),
                cache_dir: cache,
            },
        )
        .await
        .unwrap();
    match fetch {
        SourceFetch::Plain(p) => assert_eq!(p.parts.len(), 1),
        SourceFetch::Encrypted(_) => panic!("expected plain"),
    }
}
