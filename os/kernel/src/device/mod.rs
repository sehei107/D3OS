pub mod apic;
pub mod pit;
pub mod ps2;
pub mod qemu_cfg;
pub mod speaker;
#[macro_use]
pub mod terminal;
pub mod lfb_terminal;
pub mod serial;
pub mod pci;
pub mod ihda_driver;
mod ihda_controller;
mod ihda_codec;
mod ihda_pci;
