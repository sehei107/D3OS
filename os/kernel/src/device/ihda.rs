#![allow(dead_code)]

use alloc::boxed::Box;
use core::ops::BitOr;
use log::{debug, info};
use pci_types::{Bar, BaseClass, CommandRegister, EndpointHeader, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::interrupt::interrupt_handler::InterruptHandler;
use crate::{apic, interrupt_dispatcher, pci_bus, process_manager};
use crate::device::ihda_controller::{Controller};
use crate::device::ihda_codec::{BitsPerSample, StreamFormat, StreamType};
use crate::device::pci::PciBus;
use crate::device::qemu_cfg;
use crate::interrupt::interrupt_dispatcher::InterruptVector;
use crate::memory::{MemorySpace, PAGE_SIZE};

pub struct IHDA;

unsafe impl Sync for IHDA {}
unsafe impl Send for IHDA {}

#[derive(Default)]
struct IHDAInterruptHandler;

impl InterruptHandler for IHDAInterruptHandler {
    fn trigger(&mut self) {
        debug!("INTERRUPT!!!");
    }
}

impl IHDA {
    pub fn new() -> Self {
        let pci_bus = pci_bus();
        let ihda_device = Self::find_ihda_device(pci_bus);

        Self::configure_pci(pci_bus, ihda_device);
        let controller = Self::map_mmio_space(pci_bus, ihda_device);
        Self::connect_interrupt_line(pci_bus, ihda_device);


        controller.reset();
        info!("IHDA Controller reset complete");

        // the following function call is irrelevant when not using interrupts
        // register_interface.setup_ihda_config_space();
        info!("IHDA configuration space set up");

        controller.init_dma_position_buffer();
        info!("DMA position buffer set up and running");

        // interview sound card
        let codecs = controller.scan_for_available_codecs();
        debug!("[{}] codec{} found", codecs.len(), if codecs.len() == 1 { "" } else { "s" });

        controller.init_corb();
        controller.init_rirb();
        controller.start_corb();
        controller.start_rirb();

        info!("CORB and RIRB set up and running");

        let stream_format = StreamFormat::new(2, BitsPerSample::Sixteen, 1, 1, 48000, StreamType::PCM);
        let stream_id = 1;
        let stream = &controller.allocate_output_stream(0, stream_format, 2, 128, stream_id);


        // the virtual sound card in QEMU and the physical sound card on the testing device both only had one codec, so the codec at index 0 gets auto-selected at the moment
        let codec = codecs.get(0).unwrap();
        controller.configure_codec_for_default_stereo_output(codec, stream);

        Self {}
    }

    fn find_ihda_device(pci_bus: &PciBus) -> &EndpointHeader {
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
                info!("WARNING: device selection currently hard coded!");
                ihda_devices[1]
            }
        } else {
            panic!("No IHDA device found!");
        }
    }

    fn configure_pci(pci_bus: &PciBus, ihda_device: &EndpointHeader) {
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

    fn map_mmio_space(pci_bus: &PciBus, ihda_device: &EndpointHeader) -> Controller {
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

        return Controller::new(mmio_base_address);
    }

    fn connect_interrupt_line(pci_bus: &PciBus, ihda_device: &EndpointHeader) {
        const X86_CPU_EXCEPTION_OFFSET: u8 = 32;

        let (_, interrupt_line) = ihda_device.interrupt(pci_bus.config_space());
        let interrupt_vector = InterruptVector::try_from(X86_CPU_EXCEPTION_OFFSET + interrupt_line).unwrap();
        interrupt_dispatcher().assign(interrupt_vector, Box::new(IHDAInterruptHandler::default()));
        apic().allow(interrupt_vector);
        info!("Connected driver to interrupt line {} (plus X86_CPU_EXCEPTION_OFFSET of 32)", interrupt_line);
        /*
        The sound card on the testing device uses interrupt line 3, so that CPU_EXCEPTION_OFFSET + interrupt_line = 35.
        A fake interrupt via the call of "unsafe { asm!("int 35"); }" will now result in a call of IHDAInterruptHandler's trigger() function.
        */
    }
}

// ########## debugging sandbox ##########
// let connection_list_entries_mixer11 = ConnectionListEntryResponse::try_from(register_interface.send_command(&GetConnectionListEntry(NodeAddress::new(0, 11), GetConnectionListEntryPayload::new(0)))).unwrap();
// debug!("connection list entries mixer widget: {:?}", connection_list_entries_mixer11);

// debug!("----------------------------------------------------------------------------------");
// sd_registers1.sdctl().dump();
// sd_registers1.sdsts().dump();
// sd_registers1.sdlpib().dump();
// sd_registers1.sdcbl().dump();
// sd_registers1.sdlvi().dump();
// sd_registers1.sdfifow().dump();
// sd_registers1.sdfifod().dump();
// sd_registers1.sdfmt().dump();
// sd_registers1.sdbdpl().dump();
// sd_registers1.sdbdpu().dump();
// debug!("----------------------------------------------------------------------------------");


// Timer::wait(2000);
// debug!("dma_position_in_buffer of stream descriptor [1]: {:#x}", register_interface.stream_descriptor_position_in_current_buffer(1));
// Timer::wait(2000);
// debug!("dma_position_in_buffer of stream descriptor [1]: {:#x}", register_interface.stream_descriptor_position_in_current_buffer(1));
// Timer::wait(2000);
// debug!("dma_position_in_buffer of stream descriptor [1]: {:#x}", register_interface.stream_descriptor_position_in_current_buffer(1));
// Timer::wait(2000);
// debug!("dma_position_in_buffer of stream descriptor [1]: {:#x}", register_interface.stream_descriptor_position_in_current_buffer(1));