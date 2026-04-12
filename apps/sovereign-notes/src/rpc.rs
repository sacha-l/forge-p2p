use std::path::PathBuf;
use std::sync::OnceLock;

use crate::store::NoteStore;

/// Global data directory for the RPC handler.
/// Set once at startup before the node is built.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Initialize the global data directory for RPC handlers.
pub fn init_data_dir(path: PathBuf) {
    DATA_DIR
        .set(path)
        .expect("RPC data dir already initialized");
}

/// RPC request prefix for fetching a note by ID.
const FETCH_PREFIX: &[u8] = b"FETCH:";

/// Build an RPC request payload to fetch a note by ID.
#[allow(dead_code)]
pub fn make_fetch_request(note_id: &str) -> Vec<Vec<u8>> {
    let mut key = FETCH_PREFIX.to_vec();
    key.extend_from_slice(note_id.as_bytes());
    vec![key]
}

/// RPC handler function. Receives a request and returns a response.
/// This is registered with `CoreBuilder::with_rpc()`.
///
/// Protocol:
/// - Request: `["FETCH:<note_id>"]`
/// - Response success: `["OK", "<note_json>"]`
/// - Response error: `["ERR", "<message>"]`
pub fn handle_rpc(request: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let Some(first) = request.first() else {
        return vec![b"ERR".to_vec(), b"empty request".to_vec()];
    };

    if let Some(note_id_bytes) = first.strip_prefix(FETCH_PREFIX) {
        let note_id = String::from_utf8_lossy(note_id_bytes);
        let Some(data_dir) = DATA_DIR.get() else {
            return vec![b"ERR".to_vec(), b"server not initialized".to_vec()];
        };
        match NoteStore::new(data_dir) {
            Ok(store) => match store.get(&note_id) {
                Ok(note) => match serde_json::to_string(&note) {
                    Ok(json) => vec![b"OK".to_vec(), json.into_bytes()],
                    Err(e) => vec![b"ERR".to_vec(), format!("serialize: {e}").into_bytes()],
                },
                Err(e) => vec![b"ERR".to_vec(), format!("not found: {e}").into_bytes()],
            },
            Err(e) => vec![b"ERR".to_vec(), format!("store: {e}").into_bytes()],
        }
    } else {
        vec![b"ERR".to_vec(), b"unknown command".to_vec()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fetch_existing_note() {
        let dir = TempDir::new().unwrap();
        init_data_dir(dir.path().to_path_buf());

        let store = NoteStore::new(dir.path()).unwrap();
        let note = store.create("Test Note").unwrap();

        let request = make_fetch_request(&note.id);
        let response = handle_rpc(request);
        assert_eq!(response[0], b"OK");

        let fetched: crate::store::Note = serde_json::from_slice(&response[1]).unwrap();
        assert_eq!(fetched.id, note.id);
        assert_eq!(fetched.title, "Test Note");
    }

    #[test]
    fn fetch_nonexistent_note() {
        // DATA_DIR already set from the previous test in this process
        let request = make_fetch_request("nonexistent-id");
        let response = handle_rpc(request);
        assert_eq!(response[0], b"ERR");
    }

    #[test]
    fn empty_request() {
        let response = handle_rpc(vec![]);
        assert_eq!(response[0], b"ERR");
    }
}
