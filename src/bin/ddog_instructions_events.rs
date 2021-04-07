use bcc::perf_event::{Event, HardwareEvent};

use bcc::PerfEvent;
use bcc::BPF;

use time::{Instant};

use core::sync::atomic::{AtomicBool, Ordering};
use dogstatsd::{Client, Options};
use std::sync::Arc;
use std::{collections::HashMap, hash::Hash};
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

impl Key {
    fn as_tags(&self) -> [String; 3] {
        [
            format!("pid:{}", self.pid),
            format!("name:{}", self.name),
            format!("cpu:{}", self.cpu),
        ]
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
struct Key {
    cpu: i32,
    pid: i32,
    name: String,
}

fn setup_ebpf() -> anyhow::Result<BPF> {
    let code = include_str!("instructions_events.c").to_string();
    let mut bpf = BPF::new(&code)?;
    PerfEvent::new()
        .handler("on_instructions")
        .event(Event::Hardware(HardwareEvent::Instructions))
        .sample_period(Some(DEFAULT_SAMPLE_PERIOD))
        .attach(&mut bpf)?;
    Ok(bpf)
}

fn ddog_client() -> anyhow::Result<Client> {
    let default_options = Options::default();
    Ok(Client::new(default_options)?)
}

struct LruValue {
    instructions: u64,
    accessed: Option<Instant>,
}

impl Default for LruValue {
    fn default() -> Self {
        Self {
            instructions: 0,
            accessed: None,
        }
    }
}

fn do_main(runnable: Arc<AtomicBool>) -> anyhow::Result<()> {
    let bpf = setup_ebpf()?;
    let client = ddog_client()?;

    let mut change: HashMap<Key, LruValue> = HashMap::new();
    let _start_time = Instant::now();

    while runnable.load(Ordering::SeqCst) {
        thread::sleep(time::Duration::new(1, 0));

        // Count misses
        let mut instruction_count = bpf.table("instruction_count")?;
        let instruction_map = to_map(&mut instruction_count);

        for (key, value) in instruction_map.iter() {
            let prev_value = match change.get_mut(&key) {
                Some(val) => val,
                None => {
                    change.insert(key.clone(), LruValue::default());
                    change.get_mut(&key).expect("failed to set LRU storage")
                }
            };

            let diff = (*value as i64)
                .checked_sub(prev_value.instructions as i64)
                .unwrap_or(0);
            if diff > 0 {
                client
                    .count("pawel.instructions.v0", diff, &key.as_tags())
                    .unwrap();
                prev_value.instructions = *value;
            } else if diff < 0 {
                println!(
                    "{:<-8} {:<-8} {:<-16} {:<-6}",
                    key.pid, key.cpu, key.name, value,
                );
            }
        }

        let now = Instant::now();
        change.retain(|_k, v| {
            let passed = v.accessed.and_then(|accessed| now.checked_duration_since(accessed)); 

            match passed {
                Some(passed) => { passed.as_secs() < 3600 }
                None => { false }
            }
        });
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
