use std::io;
use std::path::{Path, PathBuf};

pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read(path)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::read(path.as_ref())
    }
}

pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let bytes = read(path)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::write(path, bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::write(path.as_ref(), bytes.as_ref())
    }
}

pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::create_dir_all(path)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::create_dir_all(path.as_ref());
        Ok(())
    }
}

pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::remove_file(path)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::remove_file(path.as_ref())
    }
}

pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::rename(from, to)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::rename(from.as_ref(), to.as_ref())
    }
}

pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::remove_dir_all(path)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::remove_dir_all(path.as_ref())
    }
}

pub fn exists(path: impl AsRef<Path>) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        path.as_ref().exists()
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::exists(path.as_ref())
    }
}

pub fn list(path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(path)? {
            paths.push(entry?.path());
        }
        Ok(paths)
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::list(path.as_ref())
    }
}

pub fn sync() {
    #[cfg(target_arch = "wasm32")]
    wasm::sync();
}

pub fn take_error() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::take_error()
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn hydrate() -> Result<(), String> {
    wasm::hydrate().await
}

#[cfg(target_arch = "wasm32")]
pub fn note_error(error: String) {
    wasm::note_error(error);
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::cell::{Cell, RefCell};
    use std::collections::{BTreeMap, BTreeSet};
    use std::io;
    use std::path::{Component, Path, PathBuf};
    use std::sync::Mutex;

    use wasm_bindgen::prelude::*;

    struct Store {
        files: BTreeMap<String, Vec<u8>>,
        dirs: BTreeSet<String>,
    }

    static STORE: Mutex<Store> = Mutex::new(Store {
        files: BTreeMap::new(),
        dirs: BTreeSet::new(),
    });

    thread_local! {
        static READY: Cell<bool> = const { Cell::new(false) };
        static DIRTY: Cell<bool> = const { Cell::new(false) };
        static WRITING: Cell<bool> = const { Cell::new(false) };
        static ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
        static DB: RefCell<Option<web_sys::IdbDatabase>> = const { RefCell::new(None) };
    }

    fn store() -> std::sync::MutexGuard<'static, Store> {
        STORE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn note_error(error: String) {
        ERROR.with(|slot| *slot.borrow_mut() = Some(error));
    }

    pub fn take_error() -> Option<String> {
        ERROR.with(|slot| slot.borrow_mut().take())
    }

    fn key(path: &Path) -> String {
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    parts.pop();
                }
                Component::RootDir => {}
                Component::Prefix(prefix) => {
                    parts.push(prefix.as_os_str().to_string_lossy().replace('\\', ""));
                }
                Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            }
        }
        parts.join("/")
    }

    fn missing(path: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing {}", path.display()),
        )
    }

    fn library_root() -> String {
        key(&crate::library::library_directory())
    }

    fn in_library(key: &str) -> bool {
        let root = library_root();
        key == root || key.starts_with(&format!("{root}/"))
    }

    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        store()
            .files
            .get(&key(path))
            .cloned()
            .ok_or_else(|| missing(path))
    }

    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let file_key = key(path);
        let mut parents = PathBuf::new();
        if let Some(parent) = path.parent() {
            parents = parent.to_path_buf();
        }
        if parents.as_os_str().is_empty() {
            remember_dirs(path);
        } else {
            create_dir_all(&parents);
        }
        let mut store = store();
        store.files.insert(file_key, bytes.to_vec());
        drop(store);
        Ok(())
    }

    fn remember_dirs(path: &Path) {
        let mut built = String::new();
        for component in path.components() {
            let Component::Normal(part) = component else {
                continue;
            };
            if !built.is_empty() {
                built.push('/');
            }
            built.push_str(&part.to_string_lossy());
            store().dirs.insert(built.clone());
        }
    }

    pub fn create_dir_all(path: &Path) {
        remember_dirs(path);
    }

    pub fn remove_file(path: &Path) -> io::Result<()> {
        let file_key = key(path);
        let mut store = store();
        if store.files.remove(&file_key).is_none() {
            return Err(missing(path));
        }
        Ok(())
    }

    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let bytes = read(from)?;
        write(to, &bytes)?;
        let _ = remove_file(from);
        Ok(())
    }

    pub fn remove_dir_all(path: &Path) -> io::Result<()> {
        let dir_key = key(path);
        if !exists(path) {
            return Err(missing(path));
        }
        let prefix = format!("{dir_key}/");
        let mut store = store();
        store
            .files
            .retain(|key, _| key != &dir_key && !key.starts_with(&prefix));
        store
            .dirs
            .retain(|key| key != &dir_key && !key.starts_with(&prefix));
        Ok(())
    }

    pub fn exists(path: &Path) -> bool {
        let file_key = key(path);
        let prefix = format!("{file_key}/");
        let store = store();
        store.files.contains_key(&file_key)
            || store.dirs.contains(&file_key)
            || store.files.keys().any(|key| key.starts_with(&prefix))
            || store.dirs.iter().any(|key| key.starts_with(&prefix))
    }

    pub fn list(path: &Path) -> io::Result<Vec<PathBuf>> {
        if !exists(path) {
            return Err(missing(path));
        }
        let dir_key = key(path);
        let prefix = format!("{dir_key}/");
        let store = store();
        let mut names = BTreeSet::new();
        for key in store
            .files
            .keys()
            .chain(store.dirs.iter())
            .filter(|key| key.starts_with(&prefix))
        {
            let rest = &key[prefix.len()..];
            let Some(name) = rest.split('/').next() else {
                continue;
            };
            if !name.is_empty() {
                names.insert(format!("{prefix}{name}"));
            }
        }
        Ok(names.into_iter().map(PathBuf::from).collect())
    }

    pub fn sync() {
        if !READY.with(Cell::get) {
            return;
        }
        DIRTY.with(|dirty| dirty.set(true));
        if WRITING.with(Cell::get) {
            return;
        }
        WRITING.with(|writing| writing.set(true));
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let dirty = DIRTY.with(|dirty| {
                    let value = dirty.get();
                    dirty.set(false);
                    value
                });
                if !dirty {
                    break;
                }
                let files = library_files();
                if let Err(error) = save_library(&files).await {
                    note_error(error);
                    break;
                }
            }
            WRITING.with(|writing| writing.set(false));
            if DIRTY.with(Cell::get) {
                sync();
            }
        });
    }

    fn library_files() -> Vec<(String, Vec<u8>)> {
        store()
            .files
            .iter()
            .filter(|(key, _)| in_library(key))
            .map(|(key, bytes)| (key.clone(), bytes.clone()))
            .collect()
    }

    pub async fn hydrate() -> Result<(), String> {
        let files = load_library().await?;
        for (file_key, bytes) in files {
            let path = PathBuf::from(&file_key);
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                create_dir_all(parent);
            }
            store().files.insert(file_key, bytes);
        }
        READY.with(|ready| ready.set(true));
        Ok(())
    }

    async fn database() -> Result<web_sys::IdbDatabase, String> {
        if let Some(db) = DB.with(|slot| slot.borrow().clone()) {
            return Ok(db);
        }
        let db = open_database().await?;
        DB.with(|slot| *slot.borrow_mut() = Some(db.clone()));
        Ok(db)
    }

    async fn open_database() -> Result<web_sys::IdbDatabase, String> {
        let window = web_sys::window().ok_or_else(|| "Could not open the library".to_string())?;
        let factory = window
            .indexed_db()
            .map_err(js_message)?
            .ok_or_else(|| "Could not open the library".to_string())?;
        let request = factory
            .open_with_u32("pudduscroll", 1)
            .map_err(js_message)?;
        let upgrade = request.clone();
        let on_upgrade = Closure::once(move |_event: web_sys::IdbVersionChangeEvent| {
            let Ok(value) = upgrade.result() else {
                return;
            };
            let Ok(db) = value.dyn_into::<web_sys::IdbDatabase>() else {
                return;
            };
            if !db.object_store_names().contains("files") {
                let _ = db.create_object_store("files");
            }
        });
        request.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));
        on_upgrade.forget();
        let value = await_request(request.into()).await?;
        value
            .dyn_into::<web_sys::IdbDatabase>()
            .map_err(|_| "Could not open the library".to_string())
    }

    async fn load_library() -> Result<Vec<(String, Vec<u8>)>, String> {
        let db = database().await?;
        let transaction = db
            .transaction_with_str_and_mode("files", web_sys::IdbTransactionMode::Readonly)
            .map_err(js_message)?;
        let store = transaction.object_store("files").map_err(js_message)?;
        let keys_request = store.get_all_keys().map_err(js_message)?;
        let values_request = store.get_all().map_err(js_message)?;
        let keys = js_sys::Array::from(&await_request(keys_request).await?);
        let values = js_sys::Array::from(&await_request(values_request).await?);
        let mut files = Vec::new();
        for (key, value) in keys.iter().zip(values.iter()) {
            let Some(key) = key.as_string() else {
                continue;
            };
            let Ok(bytes) = value.dyn_into::<js_sys::Uint8Array>() else {
                continue;
            };
            files.push((key, bytes.to_vec()));
        }
        Ok(files)
    }

    async fn save_library(files: &[(String, Vec<u8>)]) -> Result<(), String> {
        let db = database().await?;
        let transaction = db
            .transaction_with_str_and_mode("files", web_sys::IdbTransactionMode::Readwrite)
            .map_err(js_message)?;
        let store = transaction.object_store("files").map_err(js_message)?;
        let clear = store.clear().map_err(js_message)?;
        let mut writes = Vec::new();
        for (key, bytes) in files {
            if !in_library(key) {
                continue;
            }
            let data = js_sys::Uint8Array::from(bytes.as_slice());
            writes.push(
                store
                    .put_with_key(data.unchecked_ref(), &JsValue::from_str(key))
                    .map_err(js_message)?,
            );
        }
        let saved = await_transaction(transaction).await;
        drop(clear);
        drop(writes);
        saved
    }

    async fn await_request(request: web_sys::IdbRequest) -> Result<JsValue, String> {
        let mut succeeded = Some(request.clone());
        let mut failed = Some(request.clone());
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let Some(succeeded) = succeeded.take() else {
                return;
            };
            let Some(failed) = failed.take() else {
                return;
            };
            let on_success = Closure::once(move || {
                let value = succeeded.result().unwrap_or(JsValue::NULL);
                let _ = resolve.call1(&JsValue::UNDEFINED, &value);
            });
            let on_error = Closure::once(move || {
                let message = match failed.error() {
                    Ok(Some(error)) => error.message(),
                    _ => "Could not store the library".to_string(),
                };
                let _ = reject.call1(&JsValue::UNDEFINED, &JsValue::from_str(&message));
            });
            request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
            request.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            on_success.forget();
            on_error.forget();
        });
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(js_message)
    }

    async fn await_transaction(transaction: web_sys::IdbTransaction) -> Result<(), String> {
        let mut failed = Some(transaction.clone());
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let Some(failed) = failed.take() else {
                return;
            };
            let on_complete = Closure::once(move || {
                let _ = resolve.call0(&JsValue::UNDEFINED);
            });
            let on_error = Closure::once(move || {
                let message = failed
                    .error()
                    .map(|error| error.message())
                    .unwrap_or_else(|| "Could not store the library".to_string());
                let _ = reject.call1(&JsValue::UNDEFINED, &JsValue::from_str(&message));
            });
            transaction.set_oncomplete(Some(on_complete.as_ref().unchecked_ref()));
            transaction.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            on_complete.forget();
            on_error.forget();
        });
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map(|_| ())
            .map_err(js_message)
    }

    fn js_message(value: JsValue) -> String {
        value
            .as_string()
            .or_else(|| {
                value
                    .dyn_ref::<web_sys::DomException>()
                    .map(web_sys::DomException::message)
            })
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "Could not store the library".to_string())
    }
}
