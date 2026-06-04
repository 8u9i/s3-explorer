use askama::Template;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::error::AppError;

pub mod copy;
pub mod delete;
pub mod download;
pub mod edit;
pub mod health;
pub mod list;
pub mod preview;
pub mod thumb;
pub mod upload;

pub fn template_into_response<T: Template>(t: &T) -> Result<Response, AppError> {
    match t.render() {
        Ok(html) => Ok((
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response()),
        Err(e) => Err(AppError::Internal(format!("template render: {e}"))),
    }
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexPage {
    pub prefix: String,
    pub prefix_display: String,
    pub has_parent: bool,
    pub parent_href: String,
    pub folder_rows: String,
    pub file_rows: String,
    pub is_empty: bool,
    pub total_files: i64,
    pub total_folders: i64,
    pub total_size_label: String,
    pub next_cursor: String,
    pub has_more: bool,
    pub search: String,
    pub search_count: i64,
    pub search_result_rows: String,
    pub searching: bool,
    #[allow(dead_code)]
    pub base_url: String,
    pub bucket: String,
    pub endpoint: String,
    pub crumb_html: String,
    #[allow(dead_code)]
    pub max_upload_bytes: usize,
}

pub fn render_folders(folders: &[FolderEntry]) -> String {
    let mut out = String::new();
    for f in folders {
        out.push_str(&format!(
            r#"<li class="folder" data-prefix="{prefix}"><span class="icon">&#x1F4C1;</span><a class="name" href="/browse?prefix={enc}">{name}</a><span class="actions-cell"><form method="post" action="/api/delete-prefix/{enc}" onsubmit="return confirm('Delete entire folder {name}?')" class="inline"><button class="btn small danger" type="submit">Delete</button></form></span></li>"#,
            prefix = html_escape(&f.prefix),
            enc = html_escape(&f.enc),
            name = html_escape(&f.name),
        ));
    }
    out
}

pub fn render_files(files: &[FileEntry], base: &str) -> String {
    let mut out = String::new();
    for f in files {
        let enc = urlencoding::encode(&f.key).into_owned();
        let short = f.key.rsplit_once('/').map(|(_, b)| b).unwrap_or(&f.key).to_string();
        let preview_url = format!("{}/preview/{}", base, enc);
        let presign_url = format!("{}/api/presign?key={}", base, enc);
        let _proxy_url = format!("{}/files/{}", base, enc);
        let download_url = format!("{}/files/{}?download=1", base, enc);
        let copy_url = format!("{}/copy?from={}", base, enc);
        let edit_btn = if f.is_text {
            format!(
                r#"<a class="btn small" href="{edit_url}">Edit</a>"#,
                edit_url = format!("{}/edit?key={}", base, enc)
            )
        } else {
            String::new()
        };
        let short_modified: String = f.last_modified.get(..19).unwrap_or("").to_string();
        let icon = icon_for(&f.key, &f.content_type);
        out.push_str(&format!(
            r#"<li class="file" data-key="{key}" data-ct="{ct}"><span class="icon">{icon}</span><a class="name" href="{preview}">{name}</a><span class="size" title="{size} bytes">{size_label}</span><span class="date" title="{modified}">{short_modified}</span><span class="actions-cell"><a class="btn small" href="{preview}">Preview</a><a class="btn small ghost" href="{presign}&amp;download=1">Presign DL</a><a class="btn small ghost" href="{download}">Proxy DL</a><button class="btn small ghost copy-presign" data-url="{presign}" type="button">Copy URL</button><a class="btn small" href="{copy}">Copy/Rename</a>{edit_btn}<button class="btn small danger" type="button" data-delete="{key}">Delete</button></span></li>"#,
            key = html_escape(&f.key),
            ct = html_escape(&f.content_type),
            icon = icon,
            preview = preview_url,
            name = html_escape(&short),
            size = f.size,
            size_label = html_escape(&f.display_size),
            modified = html_escape(&f.last_modified),
            short_modified = html_escape(&short_modified),
            presign = presign_url,
            download = download_url,
            copy = copy_url,
            edit_btn = edit_btn,
        ));
    }
    out
}

pub fn render_crumbs(prefix: &str) -> String {
    if prefix.is_empty() {
        return String::new();
    }
    let trimmed = prefix.trim_end_matches('/');
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    let mut out = String::new();
    let mut acc = String::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            acc.push('/');
        }
        acc.push_str(p);
        let full = if i == parts.len() - 1 {
            acc.clone()
        } else {
            format!("{}/", acc)
        };
        let enc = urlencoding::encode(&full).into_owned();
        out.push_str(&format!(
            r#"<span class="sep">/</span><a href="/browse?prefix={enc}">{name}</a>"#,
            enc = html_escape(&enc),
            name = html_escape(p)
        ));
    }
    out
}

pub fn icon_for(key: &str, ct: &str) -> String {
    if ct.starts_with("image/") {
        return "\u{1F5BC}".into();
    }
    if ct == "application/pdf" {
        return "\u{1F4D5}".into();
    }
    if ct.starts_with("video/") {
        return "\u{1F3AC}".into();
    }
    if ct.starts_with("audio/") {
        return "\u{1F3B5}".into();
    }
    if ct.starts_with("text/") || is_text_key(key, ct) {
        return "\u{1F4DD}".into();
    }
    if key.to_lowercase().ends_with(".zip")
        || key.to_lowercase().ends_with(".tar")
        || key.to_lowercase().ends_with(".gz")
    {
        return "\u{1F4E6}".into();
    }
    "\u{1F4C4}".into()
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn human_size(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut i = 0;
    while size >= 1024.0 && i < UNITS.len() - 1 {
        size /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", size, UNITS[i])
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct FolderEntry {
    pub name: String,
    pub prefix: String,
    pub enc: String,
}

#[derive(Clone, Debug, Default)]
pub struct FileEntry {
    pub key: String,
    pub size: i64,
    pub last_modified: String,
    pub content_type: String,
    pub etag: String,
    pub display_size: String,
    pub is_image: bool,
    pub is_text: bool,
    pub is_pdf: bool,
    pub is_video: bool,
    pub is_audio: bool,
    #[allow(dead_code)]
    pub depth: usize,
}

impl FileEntry {
    #[allow(dead_code)]
    pub fn from_head(
        key: String,
        size: i64,
        content_type: String,
        last_modified: String,
        etag: String,
    ) -> Self {
        let is_image = content_type.starts_with("image/");
        let is_text = is_text_key(&key, &content_type);
        let is_pdf = content_type == "application/pdf";
        let is_video = content_type.starts_with("video/");
        let is_audio = content_type.starts_with("audio/");
        Self {
            key,
            size,
            last_modified,
            content_type,
            etag,
            display_size: human_size(size),
            is_image,
            is_text,
            is_pdf,
            is_video,
            is_audio,
            depth: 0,
        }
    }
}

pub fn build_index_page(data: IndexPage) -> IndexPage {
    data
}

pub fn guess_content_type(key: &str) -> String {
    mime_guess::from_path(key)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

pub fn is_text_key(key: &str, ct: &str) -> bool {
    if ct.starts_with("text/") {
        return true;
    }
    if matches!(
        ct,
        "application/json"
            | "application/xml"
            | "application/javascript"
            | "application/x-yaml"
            | "application/yaml"
            | "application/toml"
    ) {
        return true;
    }
    let lower = key.to_lowercase();
    matches!(
        lower.rsplit_once('.').map(|(_, e)| e),
        Some(
            "txt"
                | "md"
                | "markdown"
                | "json"
                | "xml"
                | "html"
                | "htm"
                | "css"
                | "js"
                | "ts"
                | "rs"
                | "py"
                | "go"
                | "java"
                | "c"
                | "cpp"
                | "h"
                | "hpp"
                | "sh"
                | "bash"
                | "zsh"
                | "yml"
                | "yaml"
                | "toml"
                | "ini"
                | "cfg"
                | "conf"
                | "env"
                | "log"
                | "csv"
                | "tsv"
                | "sql"
                | "vue"
                | "svelte"
                | "jsx"
                | "tsx"
                | "rb"
                | "php"
                | "kt"
                | "swift"
                | "dart"
                | "lua"
                | "r"
        )
    )
}
