//! Opt-in OA-15 recording against a fresh, marked test-app-owned store only.
//!
//! With no fixture environment variable, no trace is written and the incumbent
//! app-data location is used. A malformed explicit fixture refuses startup;
//! it never falls back to a user's real application store.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const ROOT_ENV: &str = "PARKING_LOT_DESKTOP_ASSURANCE_ROOT";
const MARKER: &str = "oa15-desktop-assurance-v1";

#[derive(Default)]
pub struct Assurance {
    root: Option<PathBuf>,
    session: String,
    trace: Option<Mutex<File>>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}

fn now_ms() -> io::Result<u128> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(io::Error::other)
}

fn regular_file(path: &Path) -> io::Result<()> {
    if !path.symlink_metadata()?.file_type().is_file() {
        return Err(invalid("OA-15 fixture file must be regular, not a symlink"));
    }
    Ok(())
}

impl Assurance {
    pub fn from_environment() -> io::Result<Self> {
        let Some(value) = std::env::var_os(ROOT_ENV) else {
            return Ok(Self::default());
        };
        let root = PathBuf::from(value);
        if !root.is_absolute() || root.canonicalize()? != root {
            return Err(invalid("OA-15 root must be an existing absolute canonical path"));
        }
        let name = root.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if !name.starts_with("parking-lot-oa15-")
            || root.symlink_metadata()?.file_type().is_symlink()
        {
            return Err(invalid("OA-15 root must be a fixture-owned directory"));
        }
        let marker_path = root.join("owner.txt");
        regular_file(&marker_path)?;
        let marker = std::fs::read_to_string(marker_path)?;
        let lines: Vec<_> = marker.lines().collect();
        if lines.len() != 3
            || lines[0] != MARKER
            || lines[1].len() != 32
            || lines[2].len() != 40
            || !lines[1..].iter().all(|value| {
                value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        {
            return Err(invalid("OA-15 fixture ownership marker is invalid"));
        }
        let store_dir = root.join("store");
        if store_dir.canonicalize()? != store_dir || !store_dir.is_dir() {
            return Err(invalid("OA-15 store directory must not redirect outside its root"));
        }
        let store = store_dir.join("parking-lot.redb");
        if store.symlink_metadata().is_ok() {
            regular_file(&store)?;
        }
        let trace_path = root.join("app-trace.tsv");
        if trace_path.symlink_metadata().is_ok() {
            regular_file(&trace_path)?;
        }
        let trace = OpenOptions::new()
            .create(true)
            .append(true)
            .open(trace_path)?;
        let assurance = Self {
            root: Some(root),
            session: format!("{}-{}", std::process::id(), now_ms()?),
            trace: Some(Mutex::new(trace)),
        };
        assurance.record("launch", "main", None, store.to_string_lossy().as_bytes())?;
        Ok(assurance)
    }

    pub fn store_path(&self) -> Option<PathBuf> {
        self.root
            .as_ref()
            .map(|root| root.join("store/parking-lot.redb"))
    }

    /// Record only in explicit fixture mode. One synced line keeps trace errors
    /// visible to the command; a lost receipt cannot silently certify an action.
    pub fn record(
        &self,
        kind: &str,
        window: &str,
        task: Option<u64>,
        data: &[u8],
    ) -> io::Result<()> {
        let Some(trace) = &self.trace else {
            return Ok(());
        };
        if window != "main" {
            return Err(invalid("OA-15 commands must originate from the main webview"));
        }
        let mut file = trace
            .lock()
            .map_err(|_| io::Error::other("OA-15 trace lock poisoned"))?;
        let task = task.map_or_else(|| "-".to_owned(), |id| id.to_string());
        let line = format!(
            "{}\t{}\t{}\t{kind}\t{window}\t{task}\t{}\n",
            now_ms()?,
            self.session,
            std::process::id(),
            hex(data)
        );
        file.write_all(line.as_bytes())?;
        file.sync_all()
    }
}
