#![feature(asm)]
use pressurize::counters;
use std::process::Command;

// cargo +nightly build --example counters --features nightly
fn main() {
    // to be run in debug and nightly to be able to use ASM instructions in counter and
    // not to optimize the loop away
    let counter = counters::Counter::by_name("instructions-minus-irqs:u").unwrap();
    Command::new("bash")
        .arg("-c")
        .arg("for i in {1..99999}; do :; done")
        .output()
        .expect("Error running sample script");
    println!("Instructions executed: {}", counter.since_start());
    ()
}
