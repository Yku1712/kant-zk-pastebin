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
    let filename = format!("{}_{}.{}", ts, save_slug, ext);
    let id = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(&filename).to_string();
    let uucp = format!("{}/{}", uucp_dir, filename);

    // Stage upload in the spool filesystem and publish without overwriting.
    let mut staged = match tempfile::NamedTempFile::new_in(&uucp_dir) {
        Ok(f) => f,
        Err(e) => return Ok(HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": format!("staging failed: {}", e)}))),
    };
    if let Err(e) = std::io::Write::write_all(&mut staged, &file_data) {
        return Ok(HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": format!("write failed: {}", e)})));
    }
    let published = match staged.persist_noclobber(&uucp) {
        Ok(f) => f,
        Err(e) => return Ok(HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": format!("publish failed: {}", e.error)}))),
    };

    let mut hasher = Sha256::new();
    hasher.update(&file_data);
    let hash = hasher.finalize();
    let local_cid = format!("bafk{}", hex::encode(&hash[..16]));
    let witness = hex::encode(&hash);
    let ipfs_cid = ipfs::ipfs_add_bytes(&file_data);

    let final_title = save_title.clone();
    let final_description = if user_description.is_empty() { file_description(&orig_name, &mime_str, file_data.len(), &file_data) } else { user_description.clone() };

    let cid_file = format!("{}/{}.cid", uucp_dir, local_cid);

    // 1. Publish alias first
    crate::handlers::publish_cid_alias("upload", &cid_file, &id);

    // 2. Validate alias
    let is_valid_alias = std::fs::read_to_string(&cid_file)
        .map(|content| content.trim() == id)
        .unwrap_or(false);
    if !is_valid_alias {
        error!("[upload] failed to publish CID alias: {}", cid_file);
        let _ = fs::remove_file(&uucp);
        return Ok(HttpResponse::InternalServerError().json(serde_json::json!({"error": "failed to publish CID alias"})));
    }

    // 3. Write to index
    if let Err(e) = write_index_entry(&uucp_dir, &id, &final_title, Some(&final_description), vec!["upload".to_string(), ext.to_string()], &local_cid, &witness, &filename, file_data.len(), ipfs_cid.clone(), None, None) {
        error!("[upload] index failed; rolling back spool file and alias: {}", uucp);
        let _ = fs::remove_file(&uucp);
        let _ = fs::remove_file(&cid_file);
        return Ok(HttpResponse::InternalServerError().json(serde_json::json!({"error": format!("index failed: {}", e)})));
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "id": id, "filename": filename, "title": final_title, "description": final_description,
        "cid": local_cid, "ipfs_cid": ipfs_cid, "witness": witness, "mime": mime_str,
        "size": file_data.len(), "url": format!("/paste/{}", id),
    })))
}
