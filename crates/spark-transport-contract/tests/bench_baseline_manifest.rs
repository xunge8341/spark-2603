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

    let min_rps: f64 = parse_value(&content, "min_rps")
        .unwrap_or_else(|| panic!("missing min_rps in {}", full.display()))
        .parse()
        .unwrap_or_else(|e| panic!("invalid min_rps in {}: {e}", full.display()));
    let max_p99_us: f64 = parse_value(&content, "max_p99_us")
        .unwrap_or_else(|| panic!("missing max_p99_us in {}", full.display()))
        .parse()
        .unwrap_or_else(|e| panic!("invalid max_p99_us in {}: {e}", full.display()));
    let profile =
        parse_value(&content, "profile").unwrap_or_else(|| panic!("missing profile in {}", full.display()));

    assert!(min_rps > 0.0, "min_rps must be > 0 in {}", full.display());
    assert!(
        max_p99_us > 0.0,
        "max_p99_us must be > 0 in {}",
        full.display()
    );
    assert!(
        !profile.trim().is_empty(),
        "profile must not be empty in {}",
        full.display()
    );
}

#[test]
fn bench_baseline_files_are_present_and_parseable() {
    assert_valid_baseline("perf/baselines/bench_tcp_echo_default.toml");
    assert_valid_baseline("perf/baselines/bench_tcp_echo_windows.toml");
    assert_valid_baseline("perf/baselines/bench_tcp_echo_unix.toml");
}
