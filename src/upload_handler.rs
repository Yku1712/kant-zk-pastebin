// Upload file handler — split from 5895-line handlers.rs to avoid edit tool issues.
// Parses multipart/form-data manually (actix-multipart rejects MHT files with
// "Nested multipart is not supported").

use crate::handlers::capture_error_case;
use crate::handlers::{archive_name_title, clean_field, file_description, write_index_entry};
use crate::ipfs;
use crate::tagging;
use actix_web::{web, HttpRequest, HttpResponse, Result};
use chrono::Utc;
use log::{error, info};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, env, fs};

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() { return None; }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn parse_multipart_bytes(body: &[u8], boundary: &str) -> std::result::Result<Vec<(String, Option<String>, Vec<u8>)>, String> {
    let crlf_delim = format!("\r\n--{}", boundary).into_bytes();
    let mut norm = Vec::with_capacity(body.len() + 2);
    norm.extend_from_slice(b"\r\n");
    norm.extend_from_slice(body);
    let mut out = Vec::new();
    let mut pos = match find_bytes(&norm, &crlf_delim) { Some(p) => p + crlf_delim.len(), None => return Ok(out) };
    loop {
        if pos + 2 <= norm.len() && &norm[pos..pos + 2] == b"--" { break; }
        let hdr_start = if norm[pos..].starts_with(b"\r\n") { pos + 2 } else { pos };
        let hdr_end = match find_bytes(&norm[hdr_start..], b"\r\n\r\n") { Some(h) => hdr_start + h, None => break };
        let data_start = hdr_end + 4;
        let next = match find_bytes(&norm[data_start..], &crlf_delim) { Some(n) => data_start + n, None => break };
        let data_end = next;
        let hdr = std::str::from_utf8(&norm[hdr_start..hdr_end]).map_err(|_| "header not UTF-8".to_string())?;
        let mut name = String::new();
        let mut filename: Option<String> = None;
        for line in hdr.lines() {
            let l = line.to_ascii_lowercase();
            if l.starts_with("content-disposition:") {
                for part in line.split(';') {
                    let p = part.trim();
                    if let Some(v) = p.strip_prefix("name=\"") { name = v.trim_end_matches('"').to_string(); }
                    else if let Some(v) = p.strip_prefix("filename=\"") { filename = Some(v.trim_end_matches('"').to_string()); }
                }
            }
        }
        out.push((name, filename, norm[data_start..data_end].to_vec()));
        pos = next + crlf_delim.len();
    }
    Ok(out)
}


#[cfg(test)]
static C13_FORCE_PUBLISH_FAILURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// POST /upload - Upload file (multipart or raw body)
pub async fn upload_file(req: HttpRequest, body: web::Bytes) -> Result<HttpResponse> {
    let uucp_dir = env::var("UUCP_SPOOL").unwrap_or_else(|_| "/var/spool/uucp".to_string());
    let ts = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let mut file_data: Vec<u8> = Vec::new();
    let mut orig_name = String::new();
    let mut user_title = String::new();
    let mut user_description = String::new();
    let mut content_text = String::new();

    let ct = req.headers().get("Content-Type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();

    if ct.starts_with("multipart/form-data") {
        let boundary = ct.split(';').filter_map(|p| p.trim().strip_prefix("boundary=").map(|s| s.trim().to_string())).next();
        if let Some(ref b) = boundary {
            let parts = parse_multipart_bytes(&body, b).map_err(|e| actix_web::error::ErrorBadRequest(e))?;
            for (name, filename, data) in parts {
                match name.as_str() {
                    "file" => { orig_name = filename.unwrap_or_else(|| "upload".to_string()); file_data = data; }
                    "content" => content_text = String::from_utf8(data).unwrap_or_default(),
                    "title" => user_title = clean_field(&String::from_utf8_lossy(&data)),
                    "description" => user_description = clean_field(&String::from_utf8_lossy(&data)),
                    _ => {}
                }
            }
        } else {
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({"error": "missing boundary"})));
        }
    } else {
        let bod = String::from_utf8_lossy(&body).to_string();
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&bod) {
            content_text = j.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            user_title = j.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            user_description = j.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
    }

    if file_data.is_empty() && content_text.is_empty() {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({"error": "no file or content"})));
    }

    if file_data.is_empty() && !content_text.is_empty() {
        file_data = content_text.clone().into_bytes();
        orig_name = if user_title.is_empty() { "content.txt".to_string() } else { user_title.clone() };
    }

    let ext = orig_name.rsplit('.').next().unwrap_or("bin");
    let mime = if orig_name.to_lowercase().ends_with(".mth") || orig_name.to_lowercase().ends_with(".mht") || orig_name.to_lowercase().ends_with(".html") {
        "text/html".parse::<mime_guess::Mime>().unwrap_or_else(|_| mime_guess::from_ext(ext).first_or_octet_stream())
    } else { mime_guess::from_ext(ext).first_or_octet_stream() };
    let mime_str = mime.to_string();

    let save_title = if user_title.is_empty() { archive_name_title(&orig_name) } else { user_title.clone() };
    let save_slug = tagging::slugify(&save_title);
    let filename = format!("{}_{}_{}.{}", ts, save_slug, uuid::Uuid::new_v4(), ext);
    let id = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(&filename).to_string();
    let uucp = format!("{}/{}", uucp_dir, filename);

    // C13 experimental: stage content outside the public spool root.
    let staged = match crate::c13_staging::stage_bytes(
        std::path::Path::new(&uucp_dir),
        &file_data,
    ) {
        Ok(file) => file,
        Err(e) => return Ok(HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": format!("staging failed: {}", e)}))),
    };



    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let hash = hasher.finalize();
    let local_cid = format!("bafk{}", hex::encode(&hash[..16]));
    let witness = hex::encode(&hash);
    let ipfs_cid = ipfs::ipfs_add_bytes(&file_data);

    let final_title = save_title.clone();
    let final_description = if user_description.is_empty() { file_description(&orig_name, &mime_str, file_data.len(), &file_data) } else { user_description.clone() };

    // C13 experimental: record the pending upload privately.
    // This is not yet a commit protocol.
    if let Err(e) = crate::c13_transaction::record_pending(
        std::path::Path::new(&uucp_dir),
        &id,
    ) {
        return Ok(HttpResponse::InternalServerError().json(
            serde_json::json!({"error": format!("C13 journal failed: {}", e)})
        ));
    }

    let cid_file = format!("{}/{}.cid", uucp_dir, local_cid);




    // C13 interim: keep staged data private until index write succeeds.
    // Remaining risk: publish failure may leave an orphan index entry.
    // Test-only fault injection follows the publication error path.
    #[cfg(test)]
    let publish_result = if C13_FORCE_PUBLISH_FAILURE.swap(
        false,
        std::sync::atomic::Ordering::SeqCst,
    ) {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "C13 injected publication failure",
        ))
    } else {
        staged.publish_noclobber(std::path::Path::new(&uucp))
    };

    #[cfg(not(test))]
    let publish_result = staged.publish_noclobber(std::path::Path::new(&uucp));

    if let Err(e) = publish_result {
        error!("[upload] publication failed after index append: {}", e);
        return Ok(HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": format!("publish failed: {}", e)})));
    }

    // C13 experimental journal completion.
    // Readers are not yet gated by this state.
    if let Err(e) = crate::c13_transaction::record_committed(
        std::path::Path::new(&uucp_dir),
        &id,
    ) {
        error!("[upload] journal completion failed: {}", e);
        return Ok(HttpResponse::InternalServerError().json(
            serde_json::json!({"error": format!("C13 commit failed: {}", e)})
        ));
    }

    crate::handlers::publish_cid_alias("upload", &cid_file, &id);


    // Verificare strictă TOCTOU (C13) - ne asigurăm că fișierul a fost creat
    let is_valid_alias = std::fs::read_to_string(&cid_file)
        .map(|content| content.trim() == id)
        .unwrap_or(false);

    if !is_valid_alias {
        error!("[upload] failed to publish CID alias (invalid content or missing): {}", cid_file);
        let _ = fs::remove_file(&uucp);
        return Ok(HttpResponse::InternalServerError().json(serde_json::json!({"error": "failed to publish CID alias"})));
    }
    if let Err(e) = write_index_entry(&uucp_dir, &id, &final_title, Some(&final_description), vec!["upload".to_string(), ext.to_string()], &local_cid, &witness, &filename, file_data.len(), ipfs_cid.clone(), None, None) {
        error!("[upload] index failed; discarding private staged file: {}", uucp);
        return Ok(HttpResponse::InternalServerError().json(serde_json::json!({"error": format!("index failed: {}", e)})));
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "id": id, "filename": filename, "title": final_title, "description": final_description,
        "cid": local_cid, "ipfs_cid": ipfs_cid, "witness": witness, "mime": mime_str,
        "size": file_data.len(), "url": format!("/paste/{}", id),
    })))
}


#[cfg(test)]
mod c13_http_regression {
    use super::*;
    use actix_web::{test, web, App};

    #[actix_web::test]
    async fn publication_failure_must_not_append_index() {
        let spool = tempfile::tempdir().unwrap();
        std::env::set_var("UUCP_SPOOL", spool.path());

        let app = test::init_service(
            App::new().route("/upload", web::post().to(upload_file))
        ).await;

        C13_FORCE_PUBLISH_FAILURE.store(
            true,
            std::sync::atomic::Ordering::SeqCst,
        );

        let request = test::TestRequest::post()
            .uri("/upload")
            .insert_header(("Content-Type", "application/json"))
            .set_payload(
                r#"{"content":"C13_FORCED_PUBLISH_FAILURE","title":"C13 failure probe"}"#
            )
            .to_request();

        let response = test::call_service(&app, request).await;
        assert_eq!(
            response.status(),
            actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
        );

        let index_path = spool.path().join("index.jsonl");
        let records = std::fs::read_to_string(&index_path)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();

        assert_eq!(
            records, 0,
            "C13 ORPHAN INDEX: failed publication left an index record"
        );

        std::env::remove_var("UUCP_SPOOL");
    }

    #[actix_web::test]
    async fn failed_index_must_not_leave_public_upload() {
        let spool = tempfile::tempdir().unwrap();
        std::fs::create_dir(spool.path().join("index.jsonl")).unwrap();

        // Process-global environment: run this test in isolation.
        std::env::set_var("UUCP_SPOOL", spool.path());

        let app = test::init_service(
            App::new().route("/upload", web::post().to(upload_file))
        ).await;

        let request = test::TestRequest::post()
            .uri("/upload")
            .insert_header(("Content-Type", "application/json"))
            .set_payload(r#"{"content":"C13_HTTP_INDEX_FAILURE_PROBE","title":"C13 probe"}"#)
            .to_request();

        let response = test::call_service(&app, request).await;
        assert_eq!(response.status(), actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);

        let public_files: Vec<_> = std::fs::read_dir(spool.path()).unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .collect();

        assert!(
            public_files.is_empty(),
            "C13 FAILURE: index failed but public upload file remains: {:?}",
            public_files.iter().map(|e| e.path()).collect::<Vec<_>>()
        );

        std::env::remove_var("UUCP_SPOOL");
    }
}
