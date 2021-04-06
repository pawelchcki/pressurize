use bcc::perf_event::{Event, HardwareEvent};
use bcc::BccError;
use bcc::PerfEvent;
use bcc::BPF;
use clap::{App, Arg};

use core::sync::atomic::{AtomicBool, Ordering};
use dogstatsd::{Client, Options};
use std::collections::HashMap;
use std::sync::Arc;
use std::{mem, ptr, str, thread, time};

// Summarize cache reference and cache misses
//
// Based on https://github.com/iovisor/bcc/blob/master/tools/llcstat.py

const DEFAULT_SAMPLE_PERIOD: u64 = 100; // Events (Aka every 100 events)
const DEFAULT_DURATION: u64 = 10; // Seconds

#[repr(C)]
struct key_t {
    cpu: i32,
    pid: i32,
    name: [u8; 16],
}

impl Into<Key> for key_t {
    fn into(self) -> Key {
        Key {
            cpu: self.cpu,
            pid: self.pid,
            name: str::from_utf8(&self.name).unwrap_or("").to_string(),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Debug)]
struct Key {
    cpu: i32,
    pid: i32,
    name: String,
}

fn do_main(runnable: Arc<AtomicBool>) -> Result<(), BccError> {
    let matches = App::new("cpudist")
        .arg(
            Arg::with_name("sample_period")
                .long("sample_period")
                .short("c")
                .help("Sample one in this many number of cache reference / miss events")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("duration")
                .long("duration")
                .short("d")
                .help("Duration in seconds to run")
                .takes_value(true),
        )
        .get_matches();

    let sample_period: u64 = matches
        .value_of("sample_period")
        .map(|v| v.parse().expect("Invalid sample period"))
        .unwrap_or(DEFAULT_SAMPLE_PERIOD);

    let duration: u64 = matches
        .value_of("duration")
        .map(|v| v.parse().expect("Invalid duration"))
        .unwrap_or(DEFAULT_DURATION);

    let code = include_str!("llcstat.c").to_string();
    let mut bpf = BPF::new(&code)?;
    PerfEvent::new()
        .handler("on_cache_miss")
        .event(Event::Hardware(HardwareEvent::Instructions))
        .sample_period(Some(sample_period))
        .attach(&mut bpf)?;
    PerfEvent::new()
        .handler("on_cache_ref")
        .event(Event::Hardware(HardwareEvent::CacheReferences))
        .sample_period(Some(sample_period))
        .attach(&mut bpf)?;

    println!("Running for {} seconds", duration);

    let mut elapsed = 0;

    let default_options = Options::default();
    let client = Client::new(default_options).unwrap();

    let tags = &["env:production"];

    // Increment a counter
    client.incr("my_counter", tags).unwrap();

    while runnable.load(Ordering::SeqCst) {
        thread::sleep(time::Duration::new(1, 0));

        // Count misses
        let mut miss_table = bpf.table("miss_count")?;
        let miss_map = to_map(&mut miss_table);
        let mut change = HashMap::new();
        // let x  = Data {cpu: 1, pid: 2, name: "blah".to_string()};
        // change.insert(Data{.cpu = 1, .pid = 2, .name = "blah"},1 )

        for (key, value) in miss_map.iter() {
            if !change.contains_key(key) {
                change.insert(key, 0u64);
            }

            let prev_value = change.get(key).unwrap_or(&0);
            let diff = value.wrapping_sub(*prev_value) as i64;
            if diff > 0 {
                let tags = &[
                    format!("pid:{}", key.pid),
                    format!("name:{}", key.name),
                    format!("cpu:{}", key.cpu),
                ];
                client
                    .count("pawel.instructions.v0", diff, tags)
                    .unwrap();
                change.insert(key, *value);
            } else if diff < 0 {
                println!(
                    "{:<-8} {:<-8} {:<-16} {:<-6}",
                    key.pid, key.cpu, key.name, value,
                );
            }
        }
    }

    Ok(())
}

fn to_map(table: &mut bcc::table::Table) -> HashMap<Key, u64> {
    let mut map = HashMap::new();

    for entry in table.iter() {
        let key = parse_struct(&entry.key);
        let value = parse_u64(entry.value);
        map.insert(key.into(), value);
    }

    map
}

fn parse_u64(x: Vec<u8>) -> u64 {
    let mut v = [0_u8; 8];
    for i in 0..8 {
        v[i] = *x.get(i).unwrap_or(&0);
    }

    unsafe { mem::transmute(v) }
}

fn parse_struct(x: &[u8]) -> key_t {
    unsafe { ptr::read_unaligned(x.as_ptr() as *const key_t) }
}

fn main() {
    let runnable = Arc::new(AtomicBool::new(true));
    let r = runnable.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Failed to set handler for SIGINT / SIGTERM");

    if let Err(x) = do_main(runnable) {
        eprintln!("Error: {}", x);
        std::process::exit(1);
    }
}
