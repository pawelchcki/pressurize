use std::sync::Arc;
use std::{io::Write, thread, time};
use std::{
    fs::OpenOptions,
    sync::atomic::{AtomicBool, Ordering},
};

fn write_and_keep_latency(latency: u32, running: Arc<AtomicBool>) -> anyhow::Result<()> {
    let mut file = OpenOptions::new()
        .read(false)
        .write(true)
        .open("/dev/cpu_dma_latency")?;

    file.write(&latency.to_ne_bytes())?;

    while running.load(Ordering::SeqCst) {
        thread::sleep(time::Duration::from_secs(1));
    }

    drop(file);
    Ok(())
}

fn setup(running: Arc<AtomicBool>) {
    ctrlc::set_handler(move || {
        running.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");
}

fn main() {
    let running = Arc::new(AtomicBool::new(true));
    setup(running.clone());
    write_and_keep_latency(0, running).unwrap();
}
