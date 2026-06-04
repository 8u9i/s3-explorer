use std::collections::BTreeMap;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::error::AppResult;
use crate::routes::{
    build_index_page, guess_content_type, human_size, is_text_key, render_crumbs, render_files,
    render_folders, FileEntry, FolderEntry, IndexPage,
};
use crate::state::{public_base_url, AppState};

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_max")]
    pub max_keys: Option<i32>,
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
}

fn default_max() -> Option<i32> {
    Some(1000)
}

pub async fn list_objects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> AppResult<axum::response::Response> {
    let bucket = state.s3.config.bucket.clone();
    let mut req = state
        .s3
        .client
        .list_objects_v2()
        .bucket(&bucket)
        .delimiter("/")
        .max_keys(q.max_keys.unwrap_or(1000));

    let prefix = normalize_prefix(&q.prefix);
    if !prefix.is_empty() {
        req = req.prefix(&prefix);
    }
    if let Some(token) = &q.cursor {
        if !token.is_empty() {
            req = req.continuation_token(token);
        }
    }

    let resp = req.send().await?;
    let mut folders: Vec<FolderEntry> = Vec::new();
    let mut files: Vec<FileEntry> = Vec::new();
    let mut total_size: i64 = 0;

    for cp in resp.common_prefixes() {
        if let Some(p) = cp.prefix() {
            if !p.is_empty() {
                let name = p.trim_end_matches('/').to_string();
                let short = name
                    .rsplit_once('/')
                    .map(|x| x.1.to_string())
                    .unwrap_or_else(|| name.clone());
                folders.push(FolderEntry {
                    name: short,
                    prefix: p.to_string(),
                    enc: urlencoding::encode(p).into_owned(),
                });
            }
        }
    }

    for obj in resp.contents() {
        if obj.key() == Some(&prefix) {
            continue;
        }
        let key = obj.key().unwrap_or("").to_string();
        let size = obj.size().unwrap_or(0) as i64;
        let last_modified = obj
            .last_modified()
            .map(|d| d.fmt(aws_smithy_types::date_time::Format::DateTime).unwrap_or_default())
            .unwrap_or_default();
        let etag = obj.e_tag().unwrap_or("").trim_matches('"').to_string();
        let ct = guess_content_type(&key);
        let is_image = ct.starts_with("image/");
        let is_text = is_text_key(&key, &ct);
        let is_pdf = ct == "application/pdf";
        let is_video = ct.starts_with("video/");
        let is_audio = ct.starts_with("audio/");
        let depth = key.matches('/').count();

        total_size += size;
        files.push(FileEntry {
            key,
            size,
            last_modified,
            content_type: ct,
            etag,
            display_size: human_size(size),
            is_image,
            is_text,
            is_pdf,
            is_video,
            is_audio,
            depth,
        });
    }

    let next_cursor = resp.next_continuation_token().map(|s| s.to_string());
    let has_more = resp.is_truncated() == Some(true);
    let total_files = files.len() as i64;
    let total_folders = folders.len() as i64;

    let mut search_results: Vec<FileEntry> = Vec::new();
    let mut searching = false;
    let search = q.search.clone().unwrap_or_default();
    if !search.is_empty() && search.len() >= 2 {
        searching = true;
        search_results = perform_search(&state, &search, 200).await?;
        for f in &mut search_results {
            f.is_image = f.content_type.starts_with("image/");
            f.is_text = is_text_key(&f.key, &f.content_type);
            f.is_pdf = f.content_type == "application/pdf";
            f.is_video = f.content_type.starts_with("video/");
            f.is_audio = f.content_type.starts_with("audio/");
            f.display_size = human_size(f.size);
        }
    }

    let base = public_base_url(&headers);
    let folder_rows = render_folders(&folders);
    let file_rows = render_files(&files, &base);
    let search_result_rows = if searching {
        render_search_results(&search_results, &base)
    } else {
        String::new()
    };
    let (parent_href, has_parent) = compute_parent(&prefix);
    let crumb_segments = render_crumbs(&prefix);
    let prefix_display = if prefix.is_empty() { "root".to_string() } else { prefix.clone() };
    let is_empty = folders.is_empty() && files.is_empty() && !searching;

    let page = build_index_page(IndexPage {
        prefix: prefix.clone(),
        prefix_display,
        has_parent,
        parent_href,
        folder_rows,
        file_rows,
        is_empty,
        total_files,
        total_folders,
        total_size_label: human_size(total_size),
        next_cursor: next_cursor.unwrap_or_default(),
        has_more,
        search: search.clone(),
        search_count: search_results.len() as i64,
        search_result_rows,
        searching,
        base_url: base,
        bucket: state.s3.config.bucket.clone(),
        endpoint: state.s3.config.endpoint.clone(),
        crumb_segments,
        max_upload_bytes: state.s3.config.max_upload_bytes,
    });

    if q.view.as_deref() == Some("json") {
        return Ok((
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            axum::Json(serde_json::json!({
                "prefix": page.prefix,
                "folders": folders.iter().map(|f| serde_json::json!({"name": f.name, "prefix": f.prefix})).collect::<Vec<_>>(),
                "files": files.iter().map(|f| serde_json::json!({
                    "key": f.key,
                    "size": f.size,
                    "last_modified": f.last_modified,
                    "content_type": f.content_type,
                    "etag": f.etag,
                })).collect::<Vec<_>>(),
                "next_cursor": page.next_cursor,
                "has_more": page.has_more,
            })),
        )
            .into_response());
    }

    crate::routes::template_into_response(&page).map_err(Into::into)
}

fn render_search_results(results: &[FileEntry], base: &str) -> String {
    let mut out = String::new();
    for f in results {
        let enc = urlencoding::encode(&f.key).into_owned();
        let preview_url = format!("{}/preview/{}", base, enc);
        let presign_url = format!("{}/api/presign?key={}", base, enc);
        let proxy_url = format!("{}/files/{}", base, enc);
        out.push_str(&format!(
            r#"<li class="file"><span class="icon">{icon}</span><a class="name" href="{preview}">{name}</a><span class="size">{size}</span><span class="actions-cell"><a class="btn small ghost" href="{presign}">Presign</a><a class="btn small ghost" href="{proxy}">Proxy</a></span></li>"#,
            icon = crate::routes::icon_for(&f.key, &f.content_type),
            preview = preview_url,
            name = crate::routes::html_escape(f.key.rsplit_once('/').map(|(_, b)| b).unwrap_or(&f.key)),
            size = crate::routes::html_escape(&f.display_size),
            presign = presign_url,
            proxy = proxy_url,
        ));
    }
    out
}

async fn perform_search(
    state: &AppState,
    needle: &str,
    max: i32,
) -> AppResult<Vec<FileEntry>> {
    let needle_l = needle.to_lowercase();
    let bucket = state.s3.config.bucket.clone();
    let mut continuation: Option<String> = None;
    let mut results: Vec<FileEntry> = Vec::new();
    let mut seen_keys: BTreeMap<String, ()> = BTreeMap::new();

    loop {
        let mut req = state
            .s3
            .client
            .list_objects_v2()
            .bucket(&bucket)
            .max_keys(1000);
        if let Some(tok) = &continuation {
            if !tok.is_empty() {
                req = req.continuation_token(tok);
            }
        }
        let resp = req.send().await?;
        for obj in resp.contents() {
            let key = obj.key().unwrap_or("").to_string();
            if key.to_lowercase().contains(&needle_l) && seen_keys.insert(key.clone(), ()).is_none()
            {
                let size = obj.size().unwrap_or(0) as i64;
                let last_modified = obj
                    .last_modified()
                    .map(|d| d.fmt(aws_smithy_types::date_time::Format::DateTime).unwrap_or_default())
                    .unwrap_or_default();
                let etag = obj.e_tag().unwrap_or("").trim_matches('"').to_string();
                let ct = guess_content_type(&key);
                results.push(FileEntry {
                    key,
                    size,
                    last_modified,
                    content_type: ct,
                    etag,
                    display_size: String::new(),
                    is_image: false,
                    is_text: false,
                    is_pdf: false,
                    is_video: false,
                    is_audio: false,
                    depth: 0,
                });
                if results.len() as i32 >= max {
                    return Ok(results);
                }
            }
        }
        if resp.is_truncated() == Some(true) {
            continuation = resp.next_continuation_token().map(|s| s.to_string());
            if continuation.is_none() {
                break;
            }
        } else {
            break;
        }
    }
    Ok(results)
}

fn normalize_prefix(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let mut s = p.replace('\\', "/");
    if !s.ends_with('/') {
        s.push('/');
    }
    if s.starts_with('/') {
        s.remove(0);
    }
    s
}

fn compute_parent(prefix: &str) -> (String, bool) {
    if prefix.is_empty() {
        return (String::new(), false);
    }
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        return (String::new(), false);
    }
    let parent = match trimmed.rfind('/') {
        Some(0) => String::new(),
        Some(i) => trimmed[..i].to_string(),
        None => String::new(),
    };
    (urlencoding::encode(&parent).into_owned(), true)
}
