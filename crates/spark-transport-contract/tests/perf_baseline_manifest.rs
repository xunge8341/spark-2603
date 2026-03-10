use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn parse_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        if name.trim() == key {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn assert_valid_baseline(path: &str) {
    let full = repo_root().join(path);
    let content = fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", full.display()));

    let max_syscalls_per_kib: f64 = parse_value(&content, "max_syscalls_per_kib")
        .unwrap_or_else(|| panic!("missing max_syscalls_per_kib in {}", full.display()))
        .parse()
        .unwrap_or_else(|e| panic!("invalid max_syscalls_per_kib in {}: {e}", full.display()));
    let min_writev_share: f64 = parse_value(&content, "min_writev_share")
        .unwrap_or_else(|| panic!("missing min_writev_share in {}", full.display()))
        .parse()
        .unwrap_or_else(|e| panic!("invalid min_writev_share in {}: {e}", full.display()));
    let profile =
        parse_value(&content, "profile").unwrap_or_else(|| panic!("missing profile in {}", full.display()));

    assert!(
        max_syscalls_per_kib > 0.0,
        "max_syscalls_per_kib must be > 0 in {}",
        full.display()
    );
    assert!(
        min_writev_share >= 0.0,
        "min_writev_share must be >= 0 in {}",
        full.display()
    );
    assert!(
        min_writev_share <= 1.0,
        "min_writev_share must be <= 1 in {}",
        full.display()
    );
    assert!(
        !profile.trim().is_empty(),
        "profile must not be empty in {}",
        full.display()
    );
}

#[test]
fn perf_baseline_files_are_present_and_parseable() {
    assert_valid_baseline("perf/baselines/default.toml");
    assert_valid_baseline("perf/baselines/windows.toml");
    assert_valid_baseline("perf/baselines/unix.toml");
}
