use std::ffi::OsString;

use ignore::WalkBuilder;

pub const THREADS_ENV: &str = "DIRRAKE_THREADS";

pub fn configure(builder: &mut WalkBuilder, max_depth: Option<usize>) {
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .max_depth(max_depth);

    if let Some(threads) = configured_threads() {
        builder.threads(threads);
    }
}

pub fn configured_threads() -> Option<usize> {
    parse_thread_count(std::env::var_os(THREADS_ENV))
}

fn parse_thread_count(value: Option<OsString>) -> Option<usize> {
    value
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|threads| *threads > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_override_accepts_only_positive_integers() {
        assert_eq!(parse_thread_count(Some(OsString::from("1"))), Some(1));
        assert_eq!(parse_thread_count(Some(OsString::from("8"))), Some(8));
        assert_eq!(parse_thread_count(Some(OsString::from("0"))), None);
        assert_eq!(parse_thread_count(Some(OsString::from("bad"))), None);
        assert_eq!(parse_thread_count(None), None);
    }
}
