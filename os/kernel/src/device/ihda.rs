#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ops::BitOr;
use log::{debug, info};
use pci_types::{Bar, BaseClass, CommandRegister, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::interrupt::interrupt_handler::InterruptHandler;
use crate::{apic, interrupt_dispatcher, memory, pci_bus, process_manager, timer};
use crate::device::ihda_types::{Codec, CommandBuilder, ControllerRegisterSet, FunctionGroupNode, NodeAddress, ResponseParser, RootNode, SubordinateNodeCountInfo, WidgetNode};
use crate::device::ihda_types::Parameter::{AudioWidgetCapabilities, FunctionGroupType, RevisionId, SubordinateNodeCount, VendorId};
use crate::device::pit::Timer;
use crate::interrupt::interrupt_dispatcher::InterruptVector;
use crate::memory::{MemorySpace, PAGE_SIZE};

const PCI_MULTIMEDIA_DEVICE:  BaseClass = 4;
const PCI_IHDA_DEVICE:  SubClass = 3;
const MAX_AMOUNT_OF_CODECS: u8 = 15;

pub struct IHDA {
    crs: ControllerRegisterSet,
}

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
        let pci = pci_bus();

        // find ihda devices
        let ihda_devices = pci.search_by_class(PCI_MULTIMEDIA_DEVICE, PCI_IHDA_DEVICE);

        if ihda_devices.len() > 0 {
            // first found ihda device gets picked for initialisation under the assumption that there is exactly one ihda sound card available
            let device = ihda_devices[0];
            let bar0 = device.bar(0, pci.config_space()).unwrap();

            match bar0 {
                Bar::Memory32 { address, size, .. } => {
                    let crs = ControllerRegisterSet::new(address);

                    // set BME bit in command register of PCI configuration space
                    device.update_command(pci.config_space(), |command| {
                        command.bitor(CommandRegister::BUS_MASTER_ENABLE)
                    });

                    // set Memory Space bit in command register of PCI configuration space (so that hardware can respond to memory space access)
                    device.update_command(pci.config_space(), |command| {
                        command.bitor(CommandRegister::MEMORY_ENABLE)
                    });

                    // setup MMIO space (currently one-to-one mapping from physical address space to virtual address space of kernel)
                    let pages = size as usize / PAGE_SIZE;
                    let mmio_page = Page::from_start_address(VirtAddr::new(address as u64)).expect("IHDA MMIO address is not page aligned!");
                    let address_space = process_manager().read().kernel_process().unwrap().address_space();
                    address_space.map(PageRange { start: mmio_page, end: mmio_page + pages as u64 }, MemorySpace::Kernel, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE);

                    // setup interrupt line
                    const CPU_EXCEPTION_OFFSET: u8 = 32;
                    let (_, interrupt_line) = device.interrupt(pci.config_space());
                    let interrupt_vector = InterruptVector::try_from(CPU_EXCEPTION_OFFSET + interrupt_line).unwrap();
                    interrupt_dispatcher().assign(interrupt_vector, Box::new(IHDAInterruptHandler::default()));
                    apic().allow(interrupt_vector);
                    // A fake interrupt via the call of "unsafe { asm!("int 43"); }" from the crate core::arch::asm
                    // will now result in a call of IHDAInterruptHandler's "trigger"-function.

                    return Self {
                        crs,
                    }
                },

                _ => { panic!("Invalid BAR! IHDA always uses Memory32") },
            }
        }

        panic!("No IHDA device found!")
    }

    pub fn init(&self) {
        info!("Initializing IHDA sound card");
        self.reset_controller();
        info!("IHDA Controller reset complete");

        self.setup_ihda_config_space();
        info!("IHDA configuration space set up");


        self.setup_corb();
        self.setup_rirb();
        self.start_corb();
        self.start_rirb();

        info!("CORB and RIRB set up and running");

        // interview sound card
        let codecs = self.scan_for_available_codecs();

        debug!("Find all widgets in first audio function group:");
        for widget in codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().widgets().iter() {
            debug!("widget found: {:?}", widget.audio_widget_capabilities().widget_type());
        }



        // wait two minutes, so you can read the previous prints on real hardware where you can't set breakpoints with a debugger
        Timer::wait(120000);

    }

    fn reset_controller(&self) {
        unsafe {
            // set controller reset bit (CRST)
            self.crs.gctl().set_bit(0);
            let start_timer = timer().read().systime_ms();
            // value for CRST_TIMEOUT arbitrarily chosen
            const CRST_TIMEOUT: usize = 100;
            while !self.crs.gctl().assert_bit(0) {
                if timer().read().systime_ms() > start_timer + CRST_TIMEOUT {
                    panic!("IHDA controller reset timed out")
                }
            }

            // according to IHDA specification (section 4.3 Codec Discovery), the system should at least wait .521 ms after reading CRST as 1, so that the codecs have time to self-initialize
            Timer::wait(1);
        }
    }

    fn setup_ihda_config_space(&self) {
        // set Accept Unsolicited Response Enable (UNSOL) bit
        unsafe {
            self.crs.gctl().set_bit(8);
        }

        // set global interrupt enable (GIE) and controller interrupt enable (CIE) bits
        unsafe {
            self.crs.intctl().set_bit(30);
            self.crs.intctl().set_bit(31);
        }

        // enable wake events and interrupts for all SDIN (actually, only one bit needs to be set, but this works for now...)
        unsafe {
            self.crs.wakeen().set_all_bits();
        }
    }

    fn setup_corb(&self) {
        // disable CORB DMA engine (CORBRUN) and CORB memory error interrupt (CMEIE)
        unsafe {
            self.crs.corbctl().clear_all_bits();
        }

        // verify that CORB size is 1KB (IHDA specification, section 3.3.24: "There is no requirement to support more than one CORB Size.")
        let corbsize;
        unsafe {
            corbsize = self.crs.corbsize().read() & 0b11;
        }
        assert_eq!(corbsize, 0b10);

        // setup MMIO space for Command Outbound Ring Buffer – CORB
        let corb_frame_range = memory::physical::alloc(1);
        match corb_frame_range {
            PhysFrameRange { start, end: _ } => {
                let start_address = start.start_address().as_u64();
                let lbase = (start_address & 0xFFFFFFFF) as u32;
                let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;
                unsafe {
                    self.crs.corblbase().write(lbase);
                    self.crs.corbubase().write(ubase);
                }
            }
        }
    }

    fn reset_corb(&self) {
        // clear CORBWP
        unsafe {
            self.crs.corbwp().clear_all_bits();
        }

        //reset CORBRP
        unsafe {
            self.crs.corbrp().set_bit(15);
            let start_timer = timer().read().systime_ms();
            // value for CORBRPRST_TIMEOUT arbitrarily chosen
            const CORBRPRST_TIMEOUT: usize = 10000;
            while self.crs.corbrp().read() != 0x0 {
                if timer().read().systime_ms() > start_timer + CORBRPRST_TIMEOUT {
                    panic!("CORB read pointer reset timed out")
                }
            }
            // on my testing device with a physical IHDA sound card, the CORBRP reset doesn't work like described in the specification (section 3.3.21)
            // actually you are supposed to do something like this:

            // while !self.crs.corbrp.assert_bit(15) {
            //     if timer().read().systime_ms() > start_timer + CORBRPRST_TIMEOUT {
            //         panic!("CORB read pointer reset timed out")
            //     }
            // }
            // self.crs.corbrp.clear_all_bits();
            // while self.crs.corbrp.assert_bit(15) {
            //     if timer().read().systime_ms() > start_timer + CORBRPRST_TIMEOUT {
            //         panic!("CORB read pointer clear timed out")
            //     }
            // }

            // but the physical sound card never writes a 1 back to the CORBRPRST bit so that the code always panicked with "CORB read pointer reset timed out"
            // on the other hand, setting the CORBRPRST bit successfully set the CORBRP register back to 0
            // this is why the code now just checks if the register contains the value 0 after the reset
            // it is still to figure out if the controller really clears "any residual pre-fetched commands in the CORB hardware buffer within the controller" (section 3.3.21)
        }
    }

    fn setup_rirb(&self) {
        // disable RIRB response overrun interrupt control (RIRBOIC), RIRB DMA engine (RIRBDMAEN) and RIRB response interrupt control (RINTCTL)
        unsafe {
            self.crs.rirbctl().clear_all_bits();
        }

        // setup MMIO space for Response Inbound Ring Buffer – RIRB
        let rirb_frame_range = memory::physical::alloc(1);
        match rirb_frame_range {
            PhysFrameRange { start, end: _ } => {
                let start_address = start.start_address().as_u64();
                let lbase = (start_address & 0xFFFFFFFF) as u32;
                let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;
                unsafe {
                    self.crs.rirblbase().write(lbase);
                    self.crs.rirbubase().write(ubase);
                }
            }
        }

        // clear first CORB-entry (might not be necessary)
        unsafe {
            (self.crs.corblbase().read() as *mut u32).write(0x0);
        }

        // reset RIRBWP
        unsafe {
            self.crs.rirbwp().set_bit(15);
        }
    }


    fn start_corb(&self) {
        unsafe {
            // set CORBRUN and CMEIE bits
            self.crs.corbctl().set_bit(0);
            self.crs.corbctl().set_bit(1);

        }
    }
    fn start_rirb(&self) {
        unsafe {
            // set RIRBOIC, RIRBDMAEN  und RINTCTL bits
            self.crs.rirbctl().set_bit(0);
            self.crs.rirbctl().set_bit(1);
            self.crs.rirbctl().set_bit(2);
        }
    }

    // check the bitmask from bits 0 to 14 of the WAKESTS (in the specification also called STATESTS) indicating available codecs
    // then find all function group nodes and widgets associated with a codec
    fn scan_for_available_codecs(&self) -> Vec<Codec> {
        let mut codecs: Vec<Codec> = Vec::new();
        for index in 0..MAX_AMOUNT_OF_CODECS {
            unsafe {
                if self.crs.wakests().assert_bit(index) {
                    let root_node_addr = NodeAddress::new(index, 0x0);
                    let mut response;

                    let vendor_id = CommandBuilder::get_parameter(&root_node_addr, VendorId);
                    response = self.crs.immediate_command(vendor_id);
                    let vendor_id_info = ResponseParser::get_parameter_vendor_id(response);

                    let revision_id = CommandBuilder::get_parameter(&root_node_addr, RevisionId);
                    response = self.crs.immediate_command(revision_id);
                    let revision_id_info = ResponseParser::get_parameter_revision_id(response);

                    let subordinate_node_count = CommandBuilder::get_parameter(&root_node_addr, SubordinateNodeCount);
                    response = self.crs.immediate_command(subordinate_node_count);
                    let subordinate_node_count_info = ResponseParser::get_parameter_subordinate_node_count(response);

                    let function_group_nodes = self.scan_codec_for_available_function_groups(&root_node_addr, &subordinate_node_count_info);

                    let root_node = RootNode::new(index, vendor_id_info, revision_id_info, subordinate_node_count_info, function_group_nodes);
                    codecs.push(Codec::new(index, root_node));
                }
            }
        }
        codecs
    }

    fn scan_codec_for_available_function_groups(
        &self,
        root_node_addr: &NodeAddress,
        snci: &SubordinateNodeCountInfo
    ) -> Vec<FunctionGroupNode> {
        let mut fg_nodes: Vec<FunctionGroupNode> = Vec::new();
        let codec_address = *root_node_addr.codec_address();
        let mut response;

        for node_id in *snci.starting_node_number()..(*snci.starting_node_number() + *snci.total_number_of_nodes()) {
            let fg_address = NodeAddress::new(codec_address, node_id);

            unsafe {
                let subordinate_node_count = CommandBuilder::get_parameter(&fg_address, SubordinateNodeCount);
                response = self.crs.immediate_command(subordinate_node_count);
                let subordinate_node_count_info = ResponseParser::get_parameter_subordinate_node_count(response);

                let function_group_type = CommandBuilder::get_parameter(&fg_address, FunctionGroupType);
                response = self.crs.immediate_command(function_group_type);
                let function_group_type_info = ResponseParser::get_parameter_function_group_type(response);

                let widgets = self.scan_function_group_for_available_widgets(&fg_address, &subordinate_node_count_info);

                fg_nodes.push(FunctionGroupNode::new(fg_address, subordinate_node_count_info, function_group_type_info, widgets));
            }
        }
        fg_nodes
    }

    fn scan_function_group_for_available_widgets(
        &self,
        fg_addr: &NodeAddress,
        snci: &SubordinateNodeCountInfo
    ) -> Vec<WidgetNode> {
        let mut widgets: Vec<WidgetNode> = Vec::new();
        let codec_address = *fg_addr.codec_address();
        let mut response;

        for node_id in *snci.starting_node_number()..(*snci.starting_node_number() + *snci.total_number_of_nodes()) {
            let widget_address = NodeAddress::new(codec_address, node_id);

            unsafe {
                let audio_widget_capabilites = CommandBuilder::get_parameter(&widget_address, AudioWidgetCapabilities);
                response = self.crs.immediate_command(audio_widget_capabilites);
                let audio_widget_capabilities_info = ResponseParser::get_parameter_audio_widget_capabilities(response);
                widgets.push(WidgetNode::new(widget_address, audio_widget_capabilities_info));
            }
        }
        widgets
    }

    // IHDA Commands

    fn subordinate_node_count(&self, node_address: &NodeAddress) -> SubordinateNodeCountInfo {
        let command = CommandBuilder::get_parameter(node_address, SubordinateNodeCount);
        let response;
        unsafe {
            response = self.crs.immediate_command(command);
        }
        ResponseParser::get_parameter_subordinate_node_count(response)
    }
}
