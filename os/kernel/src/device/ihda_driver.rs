#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::arch::asm;
use derive_getters::Getters;
use log::{debug, info};
use pci_types::InterruptLine;
use crate::interrupt::interrupt_handler::InterruptHandler;
use crate::{apic, interrupt_dispatcher, pci_bus};
use crate::device::ihda_controller::{Controller};
use crate::device::ihda_codec::{Codec, StreamFormat};
use crate::device::ihda_pci::{configure_pci, find_ihda_device, get_interrupt_line, map_mmio_space};
use crate::device::pit::Timer;
use crate::interrupt::interrupt_dispatcher::InterruptVector;

#[derive(Getters)]
pub struct IntelHDAudioDevice {
    pub controller: Controller,
    pub codecs: Vec<Codec>,
}

unsafe impl Sync for IntelHDAudioDevice {}
unsafe impl Send for IntelHDAudioDevice {}

#[derive(Default)]
struct IHDAInterruptHandler;

impl InterruptHandler for IHDAInterruptHandler {
    fn trigger(&mut self) {
        debug!("INTERRUPT!!!");
    }
}

impl IntelHDAudioDevice {
    pub fn new() -> Self {
        let pci_bus = pci_bus();

        let ihda_device = find_ihda_device(pci_bus);

        configure_pci(pci_bus, ihda_device);
        Self::connect_interrupt_line(get_interrupt_line(pci_bus, ihda_device));

        let controller = Controller::new(map_mmio_space(pci_bus, ihda_device));

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

        // Timer::wait(600000);

        Self {
            controller,
            codecs,
        }
    }

    pub fn demo(&self) {
        let stream_format = StreamFormat::stereo_48khz_16bit();
        let stream_id = 1;
        let stream = &self.controller.allocate_output_stream(0, stream_format, 2, 128, stream_id);


        // the virtual sound card in QEMU and the physical sound card on the testing device both only had one codec, so the codec at index 0 gets auto-selected at the moment
        let codec = self.codecs.get(0).unwrap();
        self.controller.configure_codec_for_line_out_playback(codec, stream);

        // ########## write data to buffers ##########

        let mut saw = Vec::new();
        for i in 0u32..32768 {
            let sample = (i%512 * 128) as u16;
            saw.push(sample);
        }

        stream.write_data_to_buffer(0, &saw);
        stream.write_data_to_buffer(1, &saw);

        // without this flush, there is no sound coming out of the line out jack, although all DMA pages were allocated with the NO_CACHE flag...
        unsafe { asm!("wbinvd"); }

        debug!("run in one second!");
        Timer::wait(1000);
        stream.run();
    }

    fn connect_interrupt_line(interrupt_line: InterruptLine) {
        const X86_CPU_EXCEPTION_OFFSET: u8 = 32;
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