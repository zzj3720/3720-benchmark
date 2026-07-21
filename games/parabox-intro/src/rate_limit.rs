use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const API_INTERVAL: Duration = Duration::from_millis(500);

pub fn enforce(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("failed to open API rate state: {error}"))?;
    file.lock()
        .map_err(|error| format!("failed to lock API rate state: {error}"))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
        .as_millis();
    let mut saved = String::new();
    file.read_to_string(&mut saved)
        .map_err(|error| format!("failed to read API rate state: {error}"))?;
    if let Ok(previous) = saved.trim().parse::<u128>()
        && let Some(remaining) = remaining_millis(previous, now, API_INTERVAL.as_millis())
    {
        return Err(format!(
            "API rate limit: retry in {remaining} ms; send several directions in one \
             `move DIR...` batch"
        ));
    }

    file.rewind()
        .map_err(|error| format!("failed to rewind API rate state: {error}"))?;
    file.set_len(0)
        .map_err(|error| format!("failed to reset API rate state: {error}"))?;
    write!(file, "{now}").map_err(|error| format!("failed to write API rate state: {error}"))?;
    file.sync_data()
        .map_err(|error| format!("failed to persist API rate state: {error}"))
}

fn remaining_millis(previous: u128, now: u128, interval: u128) -> Option<u128> {
    let elapsed = now.saturating_sub(previous);
    if elapsed < interval {
        Some(interval - elapsed)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::remaining_millis;

    #[test]
    fn cooldown_uses_the_shared_half_second_window() {
        assert_eq!(remaining_millis(1_000, 1_000, 500), Some(500));
        assert_eq!(remaining_millis(1_000, 1_499, 500), Some(1));
        assert_eq!(remaining_millis(1_000, 1_500, 500), None);
        assert_eq!(remaining_millis(1_000, 2_000, 500), None);
    }

    #[test]
    fn backwards_clock_does_not_bypass_the_limit() {
        assert_eq!(remaining_millis(1_000, 999, 500), Some(500));
    }
}
