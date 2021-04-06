#![feature(asm)]
use pressurize::counters;

// cargo +nightly build --example counters --features nightly
fn main() {
    // to be run in debug and nightly to be able to use ASM instructions in counter and 
    // not to optimize the loop away
    let counter =counters::Counter::by_name("instructions-minus-irqs:u").unwrap();
    for i in  0..99999999 {

    }
    println!("Instructions executed: {}", counter.since_start());
    ()
}