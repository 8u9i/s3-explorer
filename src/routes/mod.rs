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
    pub crumb_segments: Vec<CrumbSegment>,
    #[allow(dead_code)]
    pub max_upload_bytes: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CrumbSegment {
    pub label: String,
    pub enc: String,
    pub is_current: bool,
}

pub fn render_crumbs(prefix: &str) -> Vec<CrumbSegment> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let trimmed = prefix.trim_end_matches('/');
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    let mut out = Vec::with_capacity(parts.len());
    let mut acc = String::new();
    let last_idx = parts.len().saturating_sub(1);
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            acc.push('/');
        }
        acc.push_str(p);
        let full = if i == last_idx {
            acc.clone()
        } else {
            format!("{}/", acc)
        };
        out.push(CrumbSegment {
            label: (*p).to_string(),
            enc: urlencoding::encode(&full).into_owned(),
            is_current: i == last_idx,
        });
    }
    out
}

pub fn render_folders(folders: &[FolderEntry]) -> String {
    let mut out = String::new();
    for f in folders {
        out.push_str(&format!(
            r#"<li class="file-row file-row--folder" data-prefix="{prefix}"><span class="file-icon file-icon--folder" aria-hidden="true">{icon_folder}</span><a class="file-row__name" href="/browse?prefix={enc}"><span class="file-row__name-main">{name}</span><span class="file-row__name-meta">folder</span></a><span class="file-row__size">—</span><span class="file-row__date">—</span><span class="file-row__actions"><form method="post" action="/api/delete-prefix/{enc}" class="delete-prefix-form" data-name="{name}"><button class="btn small danger" type="submit">Delete</button></form></span></li>"#,
            prefix = html_escape(&f.prefix),
            enc = html_escape(&f.enc),
            name = html_escape(&f.name),
            icon_folder = ICON_FOLDER,
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
        let copy_url = format!("{}/copy?from={}", base, enc);
        let edit_url = format!("{}/edit?key={}", base, enc);
        let edit_btn = if f.is_text {
            format!(
                r#"<a class="btn small" href="{edit_url}" aria-label="Edit {short_aria}">Edit</a>"#,
                edit_url = edit_url,
                short_aria = html_escape(&short),
            )
        } else {
            String::new()
        };
        let short_modified: String = f.last_modified.get(..19).unwrap_or("").to_string();
        let icon = icon_svg_for(&f.key, &f.content_type);
        let icon_class = icon_class_for(&f.key, &f.content_type);
        out.push_str(&format!(
            r#"<li class="file-row file-row--file" data-key="{key}" data-ct="{ct}"><span class="file-icon {icon_class}" aria-hidden="true">{icon}</span><a class="file-row__name" href="{preview}"><span class="file-row__name-main">{name}</span><span class="file-row__name-meta">{ct_label}</span></a><span class="file-row__size" title="{size} bytes">{size_label}</span><span class="file-row__date" title="{modified}"><time datetime="{modified}">{short_modified}</time></span><span class="file-row__actions"><a class="btn small" href="{preview}" aria-label="Preview {short_aria}">Preview</a><a class="btn small ghost" href="{presign_dl}" aria-label="Download {short_aria} (presigned)">Presign</a><a class="btn small ghost copy-presign" data-url="{presign}" type="button" aria-label="Copy presigned URL of {short_aria}">URL</a><a class="btn small" href="{copy}" aria-label="Copy or rename {short_aria}">Copy</a>{edit_btn}<button class="btn small danger" type="button" data-delete="{key}" aria-label="Delete {short_aria}">Delete</button></span></li>"#,
            key = html_escape(&f.key),
            ct = html_escape(&f.content_type),
            ct_label = html_escape(&f.content_type),
            icon = icon,
            icon_class = icon_class,
            preview = preview_url,
            presign = presign_url,
            presign_dl = format!("{presign_url}&download=1"),
            copy = copy_url,
            name = html_escape(&short),
            short_aria = html_escape(&short),
            size = f.size,
            size_label = html_escape(&f.display_size),
            modified = html_escape(&f.last_modified),
            short_modified = html_escape(&short_modified),
            edit_btn = edit_btn,
        ));
    }
    out
}

/// Inline SVG icons used by the file/folder list. Each is sized to fit a
/// 20×20 CSS box and uses `currentColor` for theming.
pub const ICON_FOLDER: &str = r#"<svg viewBox="0 0 20 20" fill="currentColor"><path d="M2 6a2 2 0 012-2h4l2 2h6a2 2 0 012 2v6a2 2 0 01-2 2H4a2 2 0 01-2-2V6z"/></svg>"#;
pub const ICON_FILE: &str = r#"<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M5 3h7l4 4v10a1 1 0 01-1 1H5a1 1 0 01-1-1V4a1 1 0 011-1z"/><path d="M12 3v4h4"/></svg>"#;
pub const ICON_IMAGE: &str = r#"<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3" y="4" width="14" height="12" rx="1.5"/><circle cx="8" cy="9" r="1.4" fill="currentColor"/><path d="M3 14l4-4 4 3 3-2 3 3"/></svg>"#;
pub const ICON_VIDEO: &str = r#"<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3" y="5" width="14" height="10" rx="1.5"/><path d="M8 8l5 2.5L8 13V8z" fill="currentColor" stroke="none"/></svg>"#;
pub const ICON_AUDIO: &str = r#"<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M9 4v10a3 3 0 11-3-3"/><path d="M9 4l6-1v9"/></svg>"#;
pub const ICON_PDF: &str = r#"<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M5 3h7l4 4v10a1 1 0 01-1 1H5a1 1 0 01-1-1V4a1 1 0 011-1z"/><text x="10" y="14" text-anchor="middle" font-size="4" font-family="ui-sans-serif" font-weight="700" fill="currentColor" stroke="none">PDF</text></svg>"#;
pub const ICON_CODE: &str = r#"<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M7 6L3 10l4 4M13 6l4 4-4 4"/></svg>"#;
pub const ICON_ARCHIVE: &str = r#"<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3" y="4" width="14" height="4" rx="1"/><path d="M4 8v8a1 1 0 001 1h10a1 1 0 001-1V8"/><path d="M10 11v4"/></svg>"#;
pub const ICON_TEXT: &str = r#"<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M5 5h10M5 9h10M5 13h7M5 17h5"/></svg>"#;

pub fn icon_class_for(key: &str, ct: &str) -> &'static str {
    if ct.starts_with("image/") {
        return "file-icon--image";
    }
    if ct == "application/pdf" {
        return "file-icon--pdf";
    }
    if ct.starts_with("video/") {
        return "file-icon--video";
    }
    if ct.starts_with("audio/") {
        return "file-icon--audio";
    }
    if is_code_key(key) {
        return "file-icon--code";
    }
    if is_archive_key(key) {
        return "file-icon--archive";
    }
    if ct.starts_with("text/") || is_text_key(key, ct) {
        return "file-icon--text";
    }
    "file-icon--text"
}

pub fn icon_svg_for(key: &str, ct: &str) -> &'static str {
    if ct.starts_with("image/") {
        return ICON_IMAGE;
    }
    if ct == "application/pdf" {
        return ICON_PDF;
    }
    if ct.starts_with("video/") {
        return ICON_VIDEO;
    }
    if ct.starts_with("audio/") {
        return ICON_AUDIO;
    }
    if is_code_key(key) {
        return ICON_CODE;
    }
    if is_archive_key(key) {
        return ICON_ARCHIVE;
    }
    if ct.starts_with("text/") || is_text_key(key, ct) {
        return ICON_TEXT;
    }
    ICON_FILE
}

fn is_code_key(key: &str) -> bool {
    matches!(
        key.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref(),
        Some("js" | "ts" | "jsx" | "tsx" | "rs" | "py" | "go" | "java" | "kt" | "swift" | "c" | "cpp" | "h" | "hpp" | "rb" | "php" | "vue" | "svelte" | "html" | "htm" | "css" | "scss" | "sass" | "less" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "lua" | "dart" | "sql")
    )
}

fn is_archive_key(key: &str) -> bool {
    matches!(
        key.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref(),
        Some("zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "zst")
    )
}

pub fn icon_for(key: &str, ct: &str) -> String {
    icon_svg_for(key, ct).to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    fn sample_folder(name: &str, prefix: &str) -> FolderEntry {
        FolderEntry {
            name: name.to_string(),
            prefix: prefix.to_string(),
            enc: urlencoding::encode(prefix).into_owned(),
        }
    }

    fn sample_file(key: &str, size: i64, ct: &str) -> FileEntry {
        let is_image = ct.starts_with("image/");
        let is_text = is_text_key(key, ct);
        let is_pdf = ct == "application/pdf";
        let is_video = ct.starts_with("video/");
        let is_audio = ct.starts_with("audio/");
        FileEntry {
            key: key.to_string(),
            size,
            last_modified: "2026-01-02T03:04:05.000Z".to_string(),
            content_type: ct.to_string(),
            etag: "abc".to_string(),
            display_size: human_size(size),
            is_image,
            is_text,
            is_pdf,
            is_video,
            is_audio,
            depth: 0,
        }
    }

    #[test]
    fn index_page_renders_with_segments_and_rows() {
        let folders = vec![sample_folder("photos", "photos/")];
        let files = vec![
            sample_file("readme.md", 1024, "text/markdown"),
            sample_file("logo.png", 2048, "image/png"),
        ];
        let folder_rows = render_folders(&folders);
        let file_rows = render_files(&files, "");
        let crumbs = render_crumbs("photos/sub/");
        let page = IndexPage {
            prefix: "photos/sub/".into(),
            prefix_display: "photos/sub/".into(),
            has_parent: true,
            parent_href: urlencoding::encode("photos").into_owned(),
            folder_rows,
            file_rows,
            is_empty: false,
            total_files: files.len() as i64,
            total_folders: folders.len() as i64,
            total_size_label: "3.00 KB".into(),
            next_cursor: String::new(),
            has_more: false,
            search: String::new(),
            search_count: 0,
            search_result_rows: String::new(),
            searching: false,
            base_url: String::new(),
            bucket: "test-bucket".into(),
            endpoint: "https://example.com".into(),
            crumb_segments: crumbs,
            max_upload_bytes: 1024,
        };
        let html = page.render().expect("index render");
        assert!(html.contains("file-row--folder"));
        assert!(html.contains("file-row--file"));
        assert!(html.contains("crumb__link"));
        assert!(html.contains("sub"));
        assert!(html.contains("readme.md"));
        assert!(html.contains("aria-current=\"page\""));
        assert!(html.contains("file-icon--image"));
        assert!(html.contains("Skip to main content"));
    }

    #[test]
    fn render_crumbs_marks_last_segment_current() {
        let c = render_crumbs("a/b/c/");
        assert_eq!(c.len(), 3);
        assert!(!c[0].is_current);
        assert!(!c[1].is_current);
        assert!(c[2].is_current);
        assert_eq!(c[2].label, "c");
    }

    #[test]
    fn render_crumbs_empty_when_root() {
        assert!(render_crumbs("").is_empty());
    }
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

