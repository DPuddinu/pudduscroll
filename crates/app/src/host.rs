use std::path::PathBuf;

#[derive(Debug)]
pub enum PickedFile {
    #[cfg_attr(
        target_arch = "wasm32",
        allow(dead_code, reason = "desktop dialogs return a filesystem path")
    )]
    Path(PathBuf),
    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "the browser file picker returns the file bytes")
    )]
    Bytes { name: String, bytes: Vec<u8> },
}

pub fn spawn(work: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(work);
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(async move { work() });
    }
}

pub fn boot(done: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(done);
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(error) = crate::disk::hydrate().await {
                crate::disk::note_error(error);
            }
            done();
        });
    }
}

pub fn pick_file(
    title: &str,
    filter: &str,
    extensions: &'static [&str],
    done: impl FnOnce(Option<PickedFile>) + Send + 'static,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let picked = rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter, extensions)
            .pick_file()
            .map(PickedFile::Path);
        done(picked);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let title = title.to_string();
        let filter = filter.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            let handle = rfd::AsyncFileDialog::new()
                .set_title(&title)
                .add_filter(&filter, extensions)
                .pick_file()
                .await;
            let picked = match handle {
                Some(handle) => {
                    let name = handle.file_name();
                    let bytes = handle.read().await;
                    Some(PickedFile::Bytes { name, bytes })
                }
                None => None,
            };
            done(picked);
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_path(
    title: &str,
    filter: &str,
    extensions: &[&str],
    file_name: &str,
) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter(filter, extensions)
        .set_file_name(file_name)
        .save_file()
}

pub fn fetch(url: String, done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(move || done(crate::library::download_image(&url)));
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(async move {
            done(fetch_bytes(&url).await);
        });
    }
}

#[cfg(target_arch = "wasm32")]
pub fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or_else(|| "Could not download the file".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "Could not download the file".to_string())?;
    let data = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&data);
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/octet-stream");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|_| "Could not download the file".to_string())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "Could not download the file".to_string())?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "Could not download the file".to_string())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "Could not download the file".to_string())?;
    anchor.set_href(&url);
    anchor.set_download(name);
    anchor.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or_else(|| "Could not download the photo".to_string())?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|_| "Could not download the photo".to_string())?
        .dyn_into::<web_sys::Response>()
        .map_err(|_| "Could not download the photo".to_string())?;
    if !response.ok() {
        return Err("Could not download the photo".to_string());
    }
    let final_url = response.url();
    if crate::library::direct_image_url(&final_url).is_err() {
        return Err("Could not download the photo".to_string());
    }
    if let Some(length) = response
        .headers()
        .get("Content-Length")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u64>().ok())
        && length > crate::library::MAX_LINK_BYTES
    {
        return Err("Photo is larger than 32 MB".to_string());
    }
    let buffer = wasm_bindgen_futures::JsFuture::from(
        response
            .array_buffer()
            .map_err(|_| "Could not download the photo".to_string())?,
    )
    .await
    .map_err(|_| "Could not download the photo".to_string())?;
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
    if bytes.len() as u64 > crate::library::MAX_LINK_BYTES {
        return Err("Photo is larger than 32 MB".to_string());
    }
    if bytes.is_empty() {
        return Err("Could not download the photo".to_string());
    }
    Ok(bytes)
}
