//! What a configuration name may be, and which backend sections name one
//! physical queue.
//!
//! Both checks run in [`SchedulerConfig::validate`](super::SchedulerConfig::validate),
//! before any backend is opened.

use std::path::{Component, Path};

use super::backend::BackendConfig;
use super::error::ConfigError;

/// Longest pool or backend name, in bytes.
pub const MAX_NAME_LEN: usize = 64;

/// The dotted path of the section called `name` under `section`: `pools.p`,
/// or `pools[""]` when the name could not stand in a dotted path.
pub fn section_path(section: &str, name: &str) -> String {
    if is_valid_name(name) {
        format!("{section}.{name}")
    } else {
        format!("{section}[{name:?}]")
    }
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Reject a pool or backend name that is empty, longer than [`MAX_NAME_LEN`]
/// or holds anything but ASCII letters, digits, `-` and `_`.
///
/// Names label logs and hook events and appear in the dotted paths of errors,
/// where a `.` would make a path ambiguous.
pub fn validate_name(section: &str, name: &str) -> Result<(), ConfigError> {
    if is_valid_name(name) {
        return Ok(());
    }
    let singular = section.trim_end_matches('s');
    Err(ConfigError::invalid(
        section_path(section, name),
        format!(
            "a {singular} name must be 1 to {MAX_NAME_LEN} characters of ASCII letters, digits, \
             `-` and `_`"
        ),
    ))
}

/// The physical queue a backend section opens, as a comparable key, and the
/// field of the section that names it. `None` for a backend with no queue.
///
/// The key is lexical. It joins spellings of one location that differ only in
/// form: `./a/../q.redb` and `q.redb`; `postgresql://` and `postgres://`, a
/// host in upper case, an explicit default port, a trailing `/`, other
/// credentials (a database is one database whichever role opens it), and the
/// order of query parameters. It cannot see through a symlink, a relative
/// versus an absolute path, a case-insensitive file system, a host name versus
/// its IP address, or two hosts in front of one database; opening such a pair
/// is not caught here (redb refuses to open one file twice, and two Postgres
/// handles on one queue share its leases).
pub fn physical_queue(backend: &BackendConfig) -> Option<(String, &'static str)> {
    match backend {
        BackendConfig::Redb(cfg) => Some((format!("redb\0{}", normalize_path(&cfg.path)), "path")),
        BackendConfig::Postgres(cfg) => Some((
            format!("postgres\0{}\0{}", normalize_url(&cfg.url), cfg.queue_name),
            "queue_name",
        )),
        BackendConfig::Pgmq(cfg) => Some((
            format!("pgmq\0{}\0{}", normalize_url(&cfg.url), cfg.queue_name),
            "queue_name",
        )),
        BackendConfig::Iggy(_) => None,
    }
}

/// Collapse `.`, empty components and `dir/..` pairs without touching the file
/// system.
fn normalize_path(path: &Path) -> String {
    let mut prefix = String::new();
    let mut absolute = false;
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => prefix = p.as_os_str().to_string_lossy().into_owned(),
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.last().is_some_and(|last| last != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..".to_owned());
                }
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
        }
    }
    let root = if absolute { "/" } else { "" };
    format!("{prefix}{root}{}", parts.join("/"))
}

/// `scheme://[userinfo@]host[:port][/database][?query]` reduced to the parts
/// that pick a database. See [`physical_queue`] for what it does not unify.
fn normalize_url(url: &str) -> String {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "postgresql" => "postgres".to_owned(),
        other => other.to_owned(),
    };
    // The userinfo ends at the last `@`: a password may hold `/` and `?`.
    let tail = rest.rsplit_once('@').map_or(rest, |(_, tail)| tail);
    let (location, query) = tail.split_once('?').unwrap_or((tail, ""));
    let (hostport, database) = location.split_once('/').unwrap_or((location, ""));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, port)
        }
        _ => (hostport, "5432"),
    };
    let mut parameters: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| {
            let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
            !key.eq_ignore_ascii_case("user") && !key.to_ascii_lowercase().contains("password")
        })
        .collect();
    parameters.sort_unstable();
    format!(
        "{scheme}://{}:{port}/{}?{}",
        host.to_ascii_lowercase(),
        database.trim_end_matches('/'),
        parameters.join("&")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_that_differ_only_in_form_have_one_key() {
        let key = normalize_url("postgres://app:pw@db.example:5432/app");
        for same in [
            "postgresql://app:pw@db.example:5432/app",
            "postgres://other:x@DB.example/app",
            "postgres://app@db.example:5432/app/",
            "postgres://db.example/app?user=app&password=pw",
            "postgres://app:p?w@db.example/app",
        ] {
            assert_eq!(normalize_url(same), key, "{same}");
        }
        for different in [
            "postgres://app:pw@db.example:5433/app",
            "postgres://app:pw@db2.example:5432/app",
            "postgres://app:pw@db.example:5432/other",
            "postgres://app:pw@db.example:5432/app?options=-csearch_path%3Dx",
        ] {
            assert_ne!(normalize_url(different), key, "{different}");
        }
    }

    #[test]
    fn paths_that_differ_only_in_form_have_one_key() {
        let key = normalize_path(Path::new("data/q.redb"));
        for same in ["./data/q.redb", "data//q.redb", "data/x/../q.redb"] {
            assert_eq!(normalize_path(Path::new(same)), key, "{same}");
        }
        assert_ne!(
            normalize_path(Path::new("../q.redb")),
            normalize_path(Path::new("q.redb"))
        );
        assert_eq!(normalize_path(Path::new("/../q.redb")), "/q.redb");
    }
}
