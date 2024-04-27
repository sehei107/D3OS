#![allow(dead_code)]

use core::ops::BitOr;
use log::{info};
use pci_types::{Bar, BaseClass, CommandRegister, EndpointHeader, InterruptLine, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::process_manager;
use crate::device::pci::PciBus;
use crate::device::qemu_cfg;
use crate::memory::{MemorySpace, PAGE_SIZE};

pub fn find_ihda_device(pci_bus: &PciBus) -> &EndpointHeader {
    const PCI_MULTIMEDIA_DEVICE:  BaseClass = 4;
    const PCI_IHDA_DEVICE:  SubClass = 3;

    // find ihda devices
    let ihda_devices = pci_bus.search_by_class(PCI_MULTIMEDIA_DEVICE, PCI_IHDA_DEVICE);
    // let ihda_devices = pci.search_by_ids(0x1022, 0x1457);
    info!("[{}] IHDA device{} found", ihda_devices.len(), if ihda_devices.len() == 1 { "" } else { "s" });

    if ihda_devices.len() > 0 {
        /*
        The device selection is currently hard coded in order to work in the two used development environments:
        1.: in QEMU, the IHDA sound card is the device at index 0
        2.: on the testing device with real hardware, it is at index 1 as the graphics card's sound card is at index 0
        The graphics card's sound card gets ignored completely by the driver as the driver in its current state
        doesn't support digital input/output formats.
        A user, who wants to use the integrated sound card as well as to play sound over HDMI/Displayport via the graphics card,
        would need to initiate two IHDA devices instead of one (after implementing support for digital input/output formats).

        A universal device selection algorithm would require a better overview over existing vendors and devices.
        The hda_intel.c from the IHDA linux driver for example gets this overview through more than 300 lines of hard coded
        vendor id / device id combinations, so that the driver can explicitly filter devices by these ids.
        As this complexity can not be handled within the context of a bachelor thesis,
        the device selection stays hard coded for now and probably needs to be adjusted when booting on a different machine.
        */
        if qemu_cfg::is_available() {
            ihda_devices[0]
        } else {
            for device in ihda_devices {
                match device.header().id(pci_bus.config_space()) {
                    (vendor_id, device_id) => {
                        if vendor_id == 0x8086 && device_id == 0x8c20 {
                            return device;
                        }
                    }
                }
            }
            panic!("None of the found IHDA devices is supported by the driver.")
        }
    } else {
        panic!("No IHDA device found!");
    }
}

pub fn configure_pci(pci_bus: &PciBus, ihda_device: &EndpointHeader) {
    // set Bus Master bit in command register of PCI configuration space (so that sound card can behave as a bus master)
    ihda_device.update_command(pci_bus.config_space(), |command| {
        command.bitor(CommandRegister::BUS_MASTER_ENABLE)
    });

    // set Memory Space bit in command register of PCI configuration space (so that sound card can respond to memory space accesses)
    ihda_device.update_command(pci_bus.config_space(), |command| {
        command.bitor(CommandRegister::MEMORY_ENABLE)
    });
    info!("Set Bus Master bit and Memory Space bit in PCI configuration space");
}

pub fn map_mmio_space(pci_bus: &PciBus, ihda_device: &EndpointHeader) -> VirtAddr {
    // IHDA-MMIO address is always placed in bar 0 of the device's PCI configuration space
    let bar0 = ihda_device.bar(0, pci_bus.config_space()).unwrap();

    let mmio_base_address: u64;
    let mmio_size: u64;

    match bar0 {
        Bar::Memory32 { address, size, prefetchable: _ } => {
            mmio_base_address = address as u64;
            mmio_size = size as u64;
        }
        Bar::Memory64 { address, size, prefetchable: _ } => {
            mmio_base_address = address;
            mmio_size = size;
        }
        Bar::Io { .. } => {
            panic!("This arm should never be reached as IHDA never uses I/O space bars")
        }
    }

    // set up MMIO space (in current state of D3OS one-to-one mapping from physical address space to virtual address space of kernel)
    let pages = mmio_size / (PAGE_SIZE as u64);
    let mmio_page = Page::from_start_address(VirtAddr::new(mmio_base_address)).expect("IHDA MMIO address is not page aligned!");
    let address_space = process_manager().read().kernel_process().unwrap().address_space();
    address_space.map(
        PageRange { start: mmio_page, end: mmio_page + pages },
        MemorySpace::Kernel,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE
    );
    info!("Mapped MMIO registers to address {:#x}", mmio_base_address);

    VirtAddr::new(mmio_base_address)
}

pub fn get_interrupt_line(pci_bus: &PciBus, ihda_device: &EndpointHeader) -> InterruptLine {
    let (_, interrupt_line) = ihda_device.interrupt(pci_bus.config_space());
    interrupt_line
}
