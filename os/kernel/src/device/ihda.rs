#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec;
use core::arch::asm;
use core::ops::BitOr;
use log::{debug, info};
use pci_types::{Bar, BaseClass, CommandRegister, EndpointHeader, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::interrupt::interrupt_handler::InterruptHandler;
use crate::{apic, interrupt_dispatcher, pci_bus, process_manager};
use crate::device::ihda_controller::{ControllerRegisterInterface, Stream};
use crate::device::ihda_codec::{BitsPerSample, Codec, SetStreamFormatPayload, StreamType, WidgetNode};
use crate::device::pci::PciBus;
use crate::device::pit::Timer;
use crate::device::qemu_cfg;
use crate::interrupt::interrupt_dispatcher::InterruptVector;
use crate::memory::{MemorySpace, PAGE_SIZE};

const MAX_AMOUNT_OF_CODECS: u8 = 15;

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
        let register_interface = Self::map_mmio_space(pci_bus, ihda_device);
        Self::connect_interrupt_line(pci_bus, ihda_device);


        register_interface.reset_controller();
        info!("IHDA Controller reset complete");

        // the following function call is irrelevant when not using interrupts
        // register_interface.setup_ihda_config_space();
        info!("IHDA configuration space set up");

        register_interface.init_dma_position_buffer();
        info!("DMA position buffer set up and running");

        register_interface.init_corb();
        register_interface.init_rirb();
        register_interface.start_corb();
        register_interface.start_rirb();

        info!("CORB and RIRB set up and running");

        // interview sound card
        let codecs = Codec::scan_for_available_codecs(&register_interface);

        IHDA::prepare_default_stereo_output(&register_interface, &codecs.get(0).unwrap());

        debug!("[{}] codec{} found", codecs.len(), if codecs.len() == 1 { "" } else { "s" });

        IHDA {}
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

    fn map_mmio_space(pci_bus: &PciBus, ihda_device: &EndpointHeader) -> ControllerRegisterInterface {
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

        return ControllerRegisterInterface::new(mmio_base_address);
    }

    fn connect_interrupt_line(pci_bus: &PciBus, ihda_device: &EndpointHeader) {
        const X86_CPU_EXCEPTION_OFFSET: u8 = 32;

        let (_, interrupt_line) = ihda_device.interrupt(pci_bus.config_space());
        let interrupt_vector = InterruptVector::try_from(X86_CPU_EXCEPTION_OFFSET + interrupt_line).unwrap();
        interrupt_dispatcher().assign(interrupt_vector, Box::new(IHDAInterruptHandler::default()));
        apic().allow(interrupt_vector);
        info!("Connected driver to interrupt line {} (plus CPU_EXCEPTION_OFFSET of 32)", interrupt_line);
        /*
        The sound card on the testing device uses interrupt line 3, so that CPU_EXCEPTION_OFFSET + interrupt_line = 35.
        A fake interrupt via the call of "unsafe { asm!("int 35"); }" will now result in a call of IHDAInterruptHandler's trigger() function.
        */
    }

    fn prepare_default_stereo_output(register_interface: &ControllerRegisterInterface, codec: &Codec) {
        let widgets = codec.root_node().function_group_nodes().get(0).unwrap().widgets();
        let line_out_pin_widgets_connected_to_jack = Codec::find_line_out_pin_widgets_connected_to_jack(widgets);
        let default_output = *line_out_pin_widgets_connected_to_jack.get(0).unwrap();

        Self::default_stereo_setup(default_output, register_interface);

    }

    fn default_stereo_setup(pin_widget: &WidgetNode, register_interface: &ControllerRegisterInterface) {
        // ########## determine appropriate stream parameters ##########
        let stream_format = SetStreamFormatPayload::new(2, BitsPerSample::Sixteen, 1, 1, 48000, StreamType::PCM);

        // default stereo, 48kHz, 24 Bit stream format can be read from audio output converter widget (which gets declared further below)
        // let stream_format = SetStreamFormatPayload::from_response(StreamFormatResponse::try_from(register_interface.send_command(&GetStreamFormat(audio_out_widget.clone()))).unwrap());

        let stream_id = 1;
        let stream = Stream::new(register_interface.output_stream_descriptors().get(0).unwrap(), stream_format.clone(), 2, 2048, stream_id);
        Codec::configure_codec(pin_widget, 0, register_interface, stream_format.clone(), stream_id, 0);

        // ########## write data to buffers ##########

        // let range = *stream.cyclic_buffer().length_in_bytes() / 2;
        //
        // for index in 0..range {
        //     unsafe {
        //         let address = *stream.cyclic_buffer().audio_buffers().get(0).unwrap().start_address() + (index as u64 * 2);
        //         if (index < 5) | (index == (range  - 1)) {
        //             let value = (address as *mut u16).read();
        //             debug!("address: {:#x}, value: {:#x}", address, value)
        //         }
        //         (address as *mut u16).write((index as u16 % 160) * 409);
        //         // (address as *mut u16).write(0);
        //         if (index < 5) | (index == (range - 1)) {
        //             let value = (address as *mut u16).read();
        //             debug!("address: {:#x}, value: {:#x}", address, value)
        //         }
        //     }
        // }


        let samples = vec![0u16; 500000];

        stream.write_data_to_buffer(0, &samples);
        stream.write_data_to_buffer(1, &samples);

        // without this flush, there is no sound coming out of the line out jack, although all DMA pages were allocated with the NO_CACHE flag...
        unsafe { asm!("wbinvd"); }


        // ########## start stream ##########

        debug!("run in one second!");
        Timer::wait(1000);
        stream.run();



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

        // register_interface.rirbwp().set_bit(15);
        // Timer::wait(1000);
        // unsafe { debug!("CORB entry 0: {:#x}", (register_interface.corb_address() as *mut u32).read()); }
        // unsafe { debug!("RIRB entry 0: {:#x}", (register_interface.rirb_address() as *mut u32).read()); }
        // unsafe { debug!("CORB entry 1: {:#x}", ((register_interface.corb_address() + 4) as *mut u32).read()); }
        // unsafe { debug!("RIRB entry 1: {:#x}", ((register_interface.rirb_address() + 4) as *mut u32).read()); }
        // debug!("CORBWP: {:#x}", register_interface.corbwp().read());
        // debug!("CORBRP: {:#x}", register_interface.corbrp().read());
        // debug!("RIRBWP: {:#x}", register_interface.rirbwp().read());
        //
        // unsafe { ((register_interface.corb_address() + 4) as *mut u32).write(GetParameter(NodeAddress::new(0, 0), VendorId).as_u32()); }
        // // unsafe { ((register_interface.corb_address() + 32) as *mut u32).write(GetParameter(audio_out_widget, OutputAmpCapabilities).as_u32()); }
        //
        // register_interface.corbwp().write(register_interface.corbwp().read() + 1);
        // Timer::wait(200);
        // unsafe { debug!("CORB entry 0: {:#x}", (register_interface.corb_address() as *mut u32).read()); }
        // unsafe { debug!("RIRB entry 0: {:#x}", (register_interface.rirb_address() as *mut u32).read()); }
        // unsafe { debug!("CORB entry 1: {:#x}", ((register_interface.corb_address() + 4) as *mut u32).read()); }
        // unsafe { debug!("RIRB entry 1: {:#x}", ((register_interface.rirb_address() + 4) as *mut u32).read()); }
        // debug!("CORBWP: {:#x}", register_interface.corbwp().read());
        // debug!("CORBRP: {:#x}", register_interface.corbrp().read());
        // debug!("RIRBWP: {:#x}", register_interface.rirbwp().read());
        // Timer::wait(200);
        //
        //
        // debug!("CORB address: {:#x}", register_interface.corb_address());
        // debug!("RIRB address: {:#x}", register_interface.rirb_address());




        Timer::wait(600000);
    }
}
