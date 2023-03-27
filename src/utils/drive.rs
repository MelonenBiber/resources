use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use regex::Regex;
use std::{collections::HashMap, path::PathBuf};

static DRIVE_REGEX: OnceCell<Regex> = OnceCell::new();

const SYS_STAT_FIELDS: [&str; 17] = [
    "read_ios",
    "read_merges",
    "read_sectors",
    "read_ticks",
    "write_ios",
    "write_merges",
    "write_sectors",
    "write_ticks",
    "in_flight",
    "io_ticks",
    "time_in_queue",
    "discard_ios",
    "discard_merges",
    "discard_sectors",
    "discard_ticks",
    "flush_ios",
    "flush_ticks",
];

/// Returns the parsed contents of the stat file
/// of the `dev`'s sysfs folder
///
/// # Errors
///
/// Will return `Err` if the are errors during
/// reading or parsing
pub async fn sys_stat(dev: &str) -> Result<HashMap<&'static str, usize>> {
    let stat = async_std::fs::read_to_string(PathBuf::from(format!("/sys/block/{dev}/stat")))
        .await
        .with_context(|| format!("unable to read /sys/block/{dev}/stat"))?;
    // TODO: maybe generate this regex automatically from `SYS_STAT_FIELDS`?
    let captures = DRIVE_REGEX
        .get_or_init(|| Regex::new(r" *(?P<read_ios>[0-9]*) *(?P<read_merges>[0-9]*) *(?P<read_sectors>[0-9]*) *(?P<read_ticks>[0-9]*) *(?P<write_ios>[0-9]*) *(?P<write_merges>[0-9]*) *(?P<write_sectors>[0-9]*) *(?P<write_ticks>[0-9]*) *(?P<in_flight>[0-9]*) *(?P<io_ticks>[0-9]*) *(?P<time_in_queue>[0-9]*) *(?P<discard_ios>[0-9]*) *(?P<discard_merges>[0-9]*) *(?P<discard_sectors>[0-9]*) *(?P<discard_ticks>[0-9]*) *(?P<flush_ios>[0-9]*) *(?P<flush_ticks>[0-9]*)").unwrap())
        .captures(&stat)
        .ok_or_else(|| anyhow!("unable to parse /sys/block/{dev}/stat"))?;
    let mut hash_map = HashMap::new();
    for field in SYS_STAT_FIELDS {
        hash_map.insert(
            field,
            captures
                .name(field)
                .ok_or_else(|| anyhow!("unable to get {field} from /sys/block/{dev}/stat"))?
                .as_str()
                .parse()?,
        );
    }
    Ok(hash_map)
}

/// Returns the sector size of the given device
///
/// # Errors
///
/// Will return `Err` if the are errors during
/// reading or parsing
pub async fn get_sector_size(dev: &str) -> Result<usize> {
    async_std::fs::read_to_string(PathBuf::from(format!(
        "/sys/block/{dev}/queue/hw_sector_size"
    )))
    .await?
    .parse()
    .with_context(|| "unable to parse hw_sector_size")
}
