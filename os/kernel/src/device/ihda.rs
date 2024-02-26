use core::ops::BitOr;
use log::debug;
use pci_types::{Bar, BaseClass, CommandRegister, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::pci_bus;
use crate::memory::{MemorySpace, PAGE_SIZE};
use crate::process::process::current_process;

const PCI_MULTIMEDIA_DEVICE:  BaseClass = 4;
const PCI_IHDA_DEVICE:  SubClass = 3;

pub struct IHDA {
    mmio_address: u32,
}

impl IHDA {
    pub fn new() -> Self {
        let pci = pci_bus();

        // find ihda devices
        let ihda_devices = pci.search_by_class(PCI_MULTIMEDIA_DEVICE, PCI_IHDA_DEVICE);

        if ihda_devices.len() > 0 {
            // first found ihda device gets picked for initialisation under the assumption that there is exactly one ihda sound card available
            let device = ihda_devices[0];
            let bar0 = device.bar(0, pci.config_space()).unwrap();

            match bar0 {
                Bar::Memory32 { address, size, .. } => {
                    // set BME bit in command register of PCI configuration space
                    device.update_command(pci.config_space(), |command| {
                        command.bitor(CommandRegister::BUS_MASTER_ENABLE)
                    });

                    // setup MMIO space (currently one-to-one mapping from physical address space to virtual address space of kernel)
                    let pages = size as usize / PAGE_SIZE;
                    let mmio_page = Page::from_start_address(VirtAddr::new(address as u64)).expect("IHDA MMIO address is not page aligned!");
                    let address_space = current_process().address_space();
                    address_space.map(PageRange { start: mmio_page, end: mmio_page + pages as u64 }, MemorySpace::Kernel, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE);

                    // set controller reset bit (CRST)
                    let gctl = (address + 0x08) as *mut u32;

                    unsafe {
                        gctl.write(gctl.read() | 0x00000001);
                    }
                    
                    // some temporary reading examples of IHDA registers

                    let minor_ptr = (address + 2) as *const u8;
                    let major_ptr = (address + 3) as *const u8;

                    unsafe {
                        debug!("IHDA Version: {}.{}", major_ptr.read(), minor_ptr.read());
                    }

                    let wall_clock_counter = (address + 0x30) as *const u32;
                    
                    unsafe {
                        debug!("Wall Clock Time: {:#x}", wall_clock_counter.read());
                        debug!("Wall Clock Time: {:#x}", wall_clock_counter.read());
                        debug!("Wall Clock Time: {:#x}", wall_clock_counter.read());
                    }

                    /* potential ways to write to a buffer (don't compile yet)

                    let wav = include_bytes!("test.wav");
                    let phys_addr = current_process().address_space().translate(VirtAddr::new(wav.as_ptr() as u64)).unwrap();

                    let audio_buffer = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80];
                    let phys_addr = current_process().address_space().translate(VirtAddr::new(audio_buffer.as_ptr() as u64)).unwrap();

                    */

                    return Self {
                        mmio_address: address,
                    }
                },

                _ => { panic!("Invalid BAR! IHDA always uses Memory32") },
            }
        }

        panic!("No IHDA device found!")
    }
}
