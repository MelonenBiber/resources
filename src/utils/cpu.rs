use anyhow::{anyhow, bail, Context, Result};
use nparse::KVStrToJson;
use once_cell::sync::OnceCell;
use regex::bytes::Regex;
use serde_json::Value;
use std::process::Command;

static PROC_STAT_REGEX: OnceCell<Regex> = OnceCell::new();

#[derive(Debug, Clone, Default)]
pub struct CPUInfo {
    pub vendor_id: Option<String>,
    pub model_name: Option<String>,
    pub architecture: Option<String>,
    pub logical_cpus: Option<usize>,
    pub physical_cpus: Option<usize>,
    pub sockets: Option<usize>,
    pub virtualization: Option<String>,
    pub max_speed: Option<f32>,
}

fn lscpu() -> Result<Value> {
    String::from_utf8(
        Command::new("lscpu")
            .env("LC_ALL", "C")
            .output()
            .with_context(|| "unable to run lscpu, is util-linux installed?")?
            .stdout,
    )
    .with_context(|| "unable to parse lscpu output to UTF-8")?
    .kv_str_to_json()
    .map_err(|x| anyhow!("{}", x))
}

/// Returns a `CPUInfo` struct populated with values gathered from `lscpu`.
///
/// # Errors
///
/// Will return `Err` if the are problems during reading or parsing
/// of the `lscpu` command
pub fn cpu_info() -> Result<CPUInfo> {
    let lscpu_output = lscpu()?;

    let vendor_id = lscpu_output["Vendor ID"]
        .as_str()
        .map(std::string::ToString::to_string);
    let model_name = lscpu_output["Model name"]
        .as_str()
        .map(std::string::ToString::to_string);
    let architecture = lscpu_output["Architecture"]
        .as_str()
        .map(std::string::ToString::to_string);
    let logical_cpus = lscpu_output["CPU(s)"]
        .as_str()
        .and_then(|x| x.parse::<usize>().ok());
    let sockets = lscpu_output["Socket(s)"]
        .as_str()
        .and_then(|x| x.parse::<usize>().ok());
    let physical_cpus = lscpu_output["Core(s) per socket"]
        .as_str()
        .and_then(|x| x.parse::<usize>().ok().map(|y| y * sockets.unwrap_or(1)));
    let virtualization = lscpu_output["Virtualization"]
        .as_str()
        .map(std::string::ToString::to_string);
    let max_speed = lscpu_output["CPU max MHz"]
        .as_str()
        .and_then(|x| x.parse::<f32>().ok())
        .map(|y| y * 1_000_000.0);

    Ok(CPUInfo {
        vendor_id,
        model_name,
        architecture,
        logical_cpus,
        physical_cpus,
        sockets,
        virtualization,
        max_speed,
    })
}

/// Returns the frequency of the given CPU `core`
///
/// # Errors
///
/// Will return `Err` if the are problems during reading or parsing
/// of the corresponding file in sysfs
pub fn get_cpu_freq(core: usize) -> Result<u64> {
    std::fs::read_to_string(format!(
        "/sys/devices/system/cpu/cpu{core}/cpufreq/scaling_cur_freq"
    ))
    .with_context(|| format!("unable to read scaling_cur_freq for core {core}"))?
    .replace('\n', "")
    .parse::<u64>()
    .with_context(|| "can't parse scaling_cur_freq to usize")
    .map(|x| x * 1000)
}

fn parse_proc_stat_line(line: &[u8]) -> Result<(u64, u64)> {
    let captures = PROC_STAT_REGEX
        .get_or_init(|| Regex::new(r"cpu[0-9]* *(?P<user>[0-9]*) *(?P<nice>[0-9]*) *(?P<system>[0-9]*) *(?P<idle>[0-9]*) *(?P<iowait>[0-9]*) *(?P<irq>[0-9]*) *(?P<softirq>[0-9]*) *(?P<steal>[0-9]*) *(?P<guest>[0-9]*) *(?P<guest_nice>[0-9]*)").unwrap())
        .captures(line)
        .ok_or_else(|| anyhow!("using regex to parse /proc/stat failed"))?;
    let idle_time = captures
        .name("idle")
        .and_then(|x| String::from_utf8_lossy(x.as_bytes()).parse::<u64>().ok())
        .ok_or_else(|| anyhow!("unable to get idle time"))?
        + captures
            .name("iowait")
            .and_then(|x| String::from_utf8_lossy(x.as_bytes()).parse::<u64>().ok())
            .ok_or_else(|| anyhow!("unable to get iowait time"))?;
    let sum = captures
        .iter()
        .skip(1)
        .flat_map(|cap| {
            cap.and_then(|x| String::from_utf8_lossy(x.as_bytes()).parse::<u64>().ok())
                .ok_or_else(|| anyhow!("unable to sum CPU times from /proc/stat"))
        })
        .sum();
    Ok((idle_time, sum))
}

async fn get_proc_stat(core: Option<usize>) -> Result<String> {
    // the combined stats are in line 0, the other cores are in the following lines,
    // since our `core` argument starts with 0, we must add 1 to it if it's not `None`.
    let selected_line_number = core.map_or(0, |x| x + 1);
    let proc_stat_raw = async_std::fs::read_to_string("/proc/stat")
        .await
        .with_context(|| "unable to read /proc/stat")?;
    let mut proc_stat = proc_stat_raw.split('\n').collect::<Vec<&str>>();
    proc_stat.retain(|x| x.starts_with("cpu"));
    // return an `Error` if `core` is greater than the number of cores
    if selected_line_number > proc_stat.len() {
        bail!("`core` argument greater than amount of cores")
    }
    Ok(proc_stat[selected_line_number].to_string())
}

/// Returns the CPU usage of either all cores combined (if supplied argument is `None`),
/// or of a specific thread (taken from the supplied argument starting at 0)
/// Please keep in mind that this is the total CPU time since boot, you have to do delta
/// calculations yourself. The tuple's layout is: `(idle_time, total_time)`
///
/// # Errors
///
/// Will return `Err` if the are problems during reading or parsing
/// of /proc/stat
pub async fn get_cpu_usage(core: Option<usize>) -> Result<(u64, u64)> {
    parse_proc_stat_line(get_proc_stat(core).await?.as_bytes())
}
