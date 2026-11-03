//! `cvblob://localhost/<handle>/<blob>?mime=<type>[&as=wav]` serves
//! attachment bytes from memory, with HTTP Range support for audio/video
//! seeking. `as=wav` decodes Ogg Opus voice notes for WebViews that cannot
//! play them. Responses are marked `no-store` so the WebView does not
//! cache them.

use std::io::Read;
use std::sync::Arc;

use percent_encoding::percent_decode_str;
use tauri::http::{Request, Response, StatusCode, header};

use crate::state::AppState;

pub const SCHEME: &str = "cvblob";

#[derive(Debug, PartialEq, Eq)]
pub struct BlobRequest {
    pub handle: u32,
    pub blob: String,
    pub mime: Option<String>,
    /// Decode Ogg Opus to WAV.
    pub as_wav: bool,
}

/// Parses `/<handle>/<percent-encoded blob ref>` and the query.
pub fn parse_path(path: &str, query: Option<&str>) -> Option<BlobRequest> {
    let rest = path.strip_prefix('/')?;
    let (handle, blob) = rest.split_once('/')?;
    let handle = handle.parse().ok()?;
    let blob = percent_decode_str(blob).decode_utf8().ok()?.into_owned();
    let params: Vec<&str> = query.map(|q| q.split('&').collect()).unwrap_or_default();
    let mime = params
        .iter()
        .find_map(|kv| kv.strip_prefix("mime="))
        .and_then(|m| percent_decode_str(m).decode_utf8().ok())
        .map(|m| m.into_owned())
        .filter(|m| m.chars().all(|c| c.is_ascii_graphic()));
    Some(BlobRequest {
        handle,
        blob,
        mime,
        as_wav: params.contains(&"as=wav"),
    })
}

/// Parses a single `bytes=start-end` range for a body of `len` bytes.
pub fn parse_range(header: &str, len: usize) -> Option<(usize, usize)> {
    let spec = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start, end) = spec.split_once('-')?;
    let (start, end) = match (start.parse::<usize>().ok(), end.parse::<usize>().ok()) {
        (Some(s), Some(e)) => (s, e.min(len.checked_sub(1)?)),
        (Some(s), None) => (s, len.checked_sub(1)?),
        (None, Some(suffix)) => (len.saturating_sub(suffix), len.checked_sub(1)?),
        (None, None) => return None,
    };
    (start <= end && end < len).then_some((start, end))
}

fn error(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .body(Vec::new())
        .expect("valid response")
}

pub fn handle(state: &AppState, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let uri = request.uri();
    let Some(BlobRequest {
        handle,
        blob,
        mut mime,
        as_wav,
    }) = parse_path(uri.path(), uri.query())
    else {
        return error(StatusCode::BAD_REQUEST);
    };
    let key = format!("{handle}/{blob}");
    let cached = state.blobs.lock().expect("blob lock").get(&key);
    let data = match cached {
        Some(data) => data,
        None => {
            let archives = state.archives.lock().expect("state lock");
            let Some(open) = archives.get(&handle) else {
                return error(StatusCode::NOT_FOUND);
            };
            let mut bytes = Vec::new();
            let read = open
                .archive
                .open_blob(&cv_model::BlobRef::new(blob))
                .map_err(|e| e.to_string())
                .and_then(|mut r| r.read_to_end(&mut bytes).map_err(|e| e.to_string()));
            if let Err(e) = read {
                log::warn!("app: media could not be opened: {e}");
                return error(StatusCode::NOT_FOUND);
            }
            let data = Arc::new(bytes);
            state
                .blobs
                .lock()
                .expect("blob lock")
                .put(key.clone(), data.clone());
            data
        }
    };

    let data = if as_wav {
        let wav_key = format!("{key}#wav");
        let cached = state.blobs.lock().expect("blob lock").get(&wav_key);
        let wav = match cached {
            Some(wav) => wav,
            None => match cv_media::opus_to_wav(&data) {
                Ok(wav) => {
                    let wav = Arc::new(wav);
                    state
                        .blobs
                        .lock()
                        .expect("blob lock")
                        .put(wav_key, wav.clone());
                    wav
                }
                Err(e) => {
                    log::warn!("app: audio could not be converted: {e}");
                    return error(StatusCode::UNSUPPORTED_MEDIA_TYPE);
                }
            },
        };
        mime = Some("audio/wav".into());
        wav
    } else {
        data
    };

    let mut response = Response::builder()
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            header::CONTENT_TYPE,
            mime.unwrap_or_else(|| "application/octet-stream".into()),
        );
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|r| parse_range(r, data.len()));
    let body = match range {
        Some((start, end)) => {
            response = response.status(StatusCode::PARTIAL_CONTENT).header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{}", data.len()),
            );
            data[start..=end].to_vec()
        }
        None => data.to_vec(),
    };
    response.body(body).expect("valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_blob_paths() {
        assert_eq!(
            parse_path("/3/signal%3Aabc", Some("mime=image%2Fjpeg")),
            Some(BlobRequest {
                handle: 3,
                blob: "signal:abc".into(),
                mime: Some("image/jpeg".into()),
                as_wav: false,
            })
        );
        let r = parse_path(
            "/1/media%2F000001-0-a%20b.opus",
            Some("mime=audio%2Fogg&as=wav"),
        )
        .unwrap();
        assert_eq!(
            (r.handle, r.blob.as_str(), r.as_wav),
            (1, "media/000001-0-a b.opus", true)
        );
        assert_eq!(parse_path("/x/abc", None), None);
        assert_eq!(
            parse_path("/1/a", Some("mime=bad%0Atype")).unwrap().mime,
            None
        );
    }

    #[test]
    fn parses_ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=0-5000", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }
}
