//! Test tool to render a LinkerScript to `stdout`
//!
//! This might not represent a linker script that can be used on a
//! device! But, it may help with visually inspecting the output.

use imxrt_rt_gen::*;
use std::io;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ls = LinkerScript::<u32>::new();
    let flash = ls.region(FLASH, 0x0, 512).unwrap();
    let ram = ls.region(RAM, 0x20000000, 128).unwrap();
    ls.stack(ram.clone()).unwrap();
    ls.heap(ram.clone()).unwrap();
    ls.boot_config(512, "fcb", flash.clone()).unwrap();
    ls.vector_table(flash.clone(), Some(ram.clone())).unwrap();
    ls.text(flash.clone(), Some(ram.clone())).unwrap();
    ls.data(false, flash.clone(), Some(ram.clone())).unwrap();
    ls.rodata(false, flash.clone(), None).unwrap();
    ls.bss(false, flash.clone(), Some(ram.clone())).unwrap();
    ls.write(&mut io::stdout().lock())
        .map_err(|err| Box::new(err) as _)
}
