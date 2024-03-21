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
use crate::device::ihda_types::{AmpCapabilitiesInfo, AudioFunctionGroupCapabilitiesInfo, AudioWidgetCapabilitiesInfo, Codec, CommandBuilder, ConnectionListLengthInfo, ControllerRegisterSet, FunctionGroupNode, FunctionGroupTypeInfo, GPIOCountInfo, NodeAddress, PinCapabilitiesInfo, ProcessingCapabilitiesInfo, ResponseParser, RevisionIdInfo, RootNode, SampleSizeRateCAPsInfo, StreamFormatsInfo, SubordinateNodeCountInfo, SupportedPowerStatesInfo, VendorIdInfo, WidgetInfo, WidgetNode, WidgetType};
use crate::device::ihda_types::Parameter::{AudioFunctionGroupCapabilities, AudioWidgetCapabilities, ConnectionListLength, FunctionGroupType, GPIOCount, InputAmpCapabilities, OutputAmpCapabilities, PinCapabilities, ProcessingCapabilities, RevisionId, SampleSizeRateCAPs, StreamFormats, SubordinateNodeCount, SupportedPowerStates, VendorId};
use crate::device::pit::Timer;
use crate::interrupt::interrupt_dispatcher::InterruptVector;
use crate::memory::{MemorySpace, PAGE_SIZE};

const PCI_MULTIMEDIA_DEVICE:  BaseClass = 4;
const PCI_IHDA_DEVICE:  SubClass = 3;
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
        let crs = IHDA::connect_controller();

        info!("Initializing IHDA sound card");
        IHDA::reset_controller(&crs);
        info!("IHDA Controller reset complete");

        IHDA::setup_ihda_config_space(&crs);
        info!("IHDA configuration space set up");


        IHDA::setup_corb(&crs);
        IHDA::setup_rirb(&crs);
        IHDA::start_corb(&crs);
        IHDA::start_rirb(&crs);

        info!("CORB and RIRB set up and running");

        // interview sound card
        let codecs = IHDA::scan_for_available_codecs(&crs);

        debug!("AFG Subordinate Node Count: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().subordinate_node_count());
        debug!("AFG Function Group Type: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().function_group_type());
        debug!("AFG Audio Function Group Capabilities: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().audio_function_group_caps());
        debug!("AFG Sample Size, Rate CAPs: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().sample_size_rate_caps());
        debug!("AFG Stream Formats: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().stream_formats());
        debug!("AFG Input Amp Capabilities: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().input_amp_caps());
        debug!("AFG Output Amp Capabilities: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().output_amp_caps());
        debug!("AFG Supported Power States: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().supported_power_states());
        debug!("AFG Supported GPIO Count: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().gpio_count());

        // wait a bit to have tim to read each print
        Timer::wait(30000);

        debug!("Find all widgets in first audio function group:");
        for widget in codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().widgets().iter() {
            debug!("WIDGET FOUND: {:?}", widget);
            // wait a bit to have tim to read each print
            Timer::wait(30000);
        }

        // wait ten minutes, so you can read the previous prints on real hardware where you can't set breakpoints with a debugger
        Timer::wait(600000);

        IHDA {}
    }

    fn connect_controller() -> ControllerRegisterSet {
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

                    return crs;
                },
                _ => { panic!("Invalid BAR! IHDA always uses Memory32") },
            }
        }
        panic!("No IHDA device found!");
    }

    fn reset_controller(crs: &ControllerRegisterSet) {
        // set controller reset bit (CRST)
        crs.gctl().set_bit(0);
        let start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        const CRST_TIMEOUT: usize = 100;
        while !crs.gctl().assert_bit(0) {
            if timer().read().systime_ms() > start_timer + CRST_TIMEOUT {
                panic!("IHDA controller reset timed out")
            }
        }

        // according to IHDA specification (section 4.3 Codec Discovery), the system should at least wait .521 ms after reading CRST as 1, so that the codecs have time to self-initialize
        Timer::wait(1);
    }

    fn setup_ihda_config_space(crs: &ControllerRegisterSet) {
        // set Accept Unsolicited Response Enable (UNSOL) bit
        crs.gctl().set_bit(8);

        // set global interrupt enable (GIE) and controller interrupt enable (CIE) bits
        crs.intctl().set_bit(30);
        crs.intctl().set_bit(31);

        // enable wake events and interrupts for all SDIN (actually, only one bit needs to be set, but this works for now...)
        crs.wakeen().set_all_bits();
    }

    fn setup_corb(crs: &ControllerRegisterSet) {
        // disable CORB DMA engine (CORBRUN) and CORB memory error interrupt (CMEIE)
        crs.corbctl().clear_all_bits();

        // verify that CORB size is 1KB (IHDA specification, section 3.3.24: "There is no requirement to support more than one CORB Size.")
        let corbsize = crs.corbsize().read() & 0b11;

        assert_eq!(corbsize, 0b10);

        // setup MMIO space for Command Outbound Ring Buffer – CORB
        let corb_frame_range = memory::physical::alloc(1);
        match corb_frame_range {
            PhysFrameRange { start, end: _ } => {
                let start_address = start.start_address().as_u64();
                let lbase = (start_address & 0xFFFFFFFF) as u32;
                let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;

                crs.corblbase().write(lbase);
                crs.corbubase().write(ubase);
            }
        }
    }

    fn reset_corb(crs: &ControllerRegisterSet) {
        // clear CORBWP
        crs.corbwp().clear_all_bits();

        //reset CORBRP
        crs.corbrp().set_bit(15);
        let start_timer = timer().read().systime_ms();
        // value for CORBRPRST_TIMEOUT arbitrarily chosen
        const CORBRPRST_TIMEOUT: usize = 10000;
        while crs.corbrp().read() != 0x0 {
            if timer().read().systime_ms() > start_timer + CORBRPRST_TIMEOUT {
                panic!("CORB read pointer reset timed out")
            }
        }
        // on my testing device with a physical IHDA sound card, the CORBRP reset doesn't work like described in the specification (section 3.3.21)
        // actually you are supposed to read a 1 back from bit 15
        // but the physical sound card never wrote a 1 back to the CORBRPRST bit so that the code always panicked with "CORB read pointer reset timed out"
        // on the other hand, setting the CORBRPRST bit successfully set the CORBRP register back to 0
        // this is why the code now just checks if the register contains the value 0 after the reset
        // it is still to figure out if the controller really clears "any residual pre-fetched commands in the CORB hardware buffer within the controller" (section 3.3.21)
    }

    fn setup_rirb(crs: &ControllerRegisterSet) {
        // disable RIRB response overrun interrupt control (RIRBOIC), RIRB DMA engine (RIRBDMAEN) and RIRB response interrupt control (RINTCTL)
        crs.rirbctl().clear_all_bits();

        // setup MMIO space for Response Inbound Ring Buffer – RIRB
        let rirb_frame_range = memory::physical::alloc(1);
        match rirb_frame_range {
            PhysFrameRange { start, end: _ } => {
                let start_address = start.start_address().as_u64();
                let lbase = (start_address & 0xFFFFFFFF) as u32;
                let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;
                crs.rirblbase().write(lbase);
                crs.rirbubase().write(ubase);
            }
        }

        // reset RIRBWP
        crs.rirbwp().set_bit(15);
    }


    fn start_corb(crs: &ControllerRegisterSet) {
        // set CORBRUN and CMEIE bits
        crs.corbctl().set_bit(0);
        crs.corbctl().set_bit(1);
    }

    fn start_rirb(crs: &ControllerRegisterSet) {
        // set RIRBOIC, RIRBDMAEN  und RINTCTL bits
        crs.rirbctl().set_bit(0);
        crs.rirbctl().set_bit(1);
        crs.rirbctl().set_bit(2);
    }

    // check the bitmask from bits 0 to 14 of the WAKESTS (in the specification also called STATESTS) indicating available codecs
    // then find all function group nodes and widgets associated with a codec
    fn scan_for_available_codecs(crs: &ControllerRegisterSet) -> Vec<Codec> {
        let mut codecs: Vec<Codec> = Vec::new();
        for index in 0..MAX_AMOUNT_OF_CODECS {
            if crs.wakests().assert_bit(index) {
                let root_node_addr = NodeAddress::new(index, 0x0);
                let mut response;


                let vendor_id = CommandBuilder::get_parameter(&root_node_addr, VendorId);
                response = RegisterInterface::immediate_command(&crs, vendor_id);
                let vendor_id_info = VendorIdInfo::try_from(ResponseParser::get_parameter(VendorId, response)).unwrap();

                let revision_id = CommandBuilder::get_parameter(&root_node_addr, RevisionId);
                response = RegisterInterface::immediate_command(&crs, revision_id);
                let revision_id_info = RevisionIdInfo::try_from(ResponseParser::get_parameter(RevisionId, response)).unwrap();

                let subordinate_node_count = CommandBuilder::get_parameter(&root_node_addr, SubordinateNodeCount);
                response = RegisterInterface::immediate_command(&crs, subordinate_node_count);
                let subordinate_node_count_info = SubordinateNodeCountInfo::try_from(ResponseParser::get_parameter(SubordinateNodeCount, response)).unwrap();

                let function_group_nodes = IHDA::scan_codec_for_available_function_groups(crs, &root_node_addr, &subordinate_node_count_info);

                let root_node = RootNode::new(index, vendor_id_info, revision_id_info, subordinate_node_count_info, function_group_nodes);
                codecs.push(Codec::new(index, root_node));
            }
        }
        codecs
    }

    fn scan_codec_for_available_function_groups(
        crs: &ControllerRegisterSet,
        root_node_addr: &NodeAddress,
        snci: &SubordinateNodeCountInfo
    ) -> Vec<FunctionGroupNode> {
        let mut fg_nodes: Vec<FunctionGroupNode> = Vec::new();
        let codec_address = *root_node_addr.codec_address();
        let mut command;
        let mut response;

        for node_id in *snci.starting_node_number()..(*snci.starting_node_number() + *snci.total_number_of_nodes()) {
            let fg_address = NodeAddress::new(codec_address, node_id);


            command = CommandBuilder::get_parameter(&fg_address, SubordinateNodeCount);
            response = RegisterInterface::immediate_command(&crs, command);
            let subordinate_node_count_info = SubordinateNodeCountInfo::try_from(ResponseParser::get_parameter(SubordinateNodeCount, response)).unwrap();

            command = CommandBuilder::get_parameter(&fg_address, FunctionGroupType);
            response = RegisterInterface::immediate_command(&crs, command);
            let function_group_type_info = FunctionGroupTypeInfo::try_from(ResponseParser::get_parameter(FunctionGroupType, response)).unwrap();

            command = CommandBuilder::get_parameter(&fg_address, AudioFunctionGroupCapabilities);
            response = RegisterInterface::immediate_command(&crs, command);
            let afg_caps = AudioFunctionGroupCapabilitiesInfo::try_from(ResponseParser::get_parameter(AudioFunctionGroupCapabilities, response)).unwrap();

            command = CommandBuilder::get_parameter(&fg_address, SampleSizeRateCAPs);
            response = RegisterInterface::immediate_command(&crs, command);
            let sample_size_rate_caps = SampleSizeRateCAPsInfo::try_from(ResponseParser::get_parameter(SampleSizeRateCAPs, response)).unwrap();

            command = CommandBuilder::get_parameter(&fg_address, StreamFormats);
            response = RegisterInterface::immediate_command(&crs, command);
            let stream_formats = StreamFormatsInfo::try_from(ResponseParser::get_parameter(StreamFormats, response)).unwrap();

            command = CommandBuilder::get_parameter(&fg_address, InputAmpCapabilities);
            response = RegisterInterface::immediate_command(&crs, command);
            let input_amp_caps = AmpCapabilitiesInfo::try_from(ResponseParser::get_parameter(InputAmpCapabilities, response)).unwrap();

            command = CommandBuilder::get_parameter(&fg_address, OutputAmpCapabilities);
            response = RegisterInterface::immediate_command(&crs, command);
            let output_amp_caps = AmpCapabilitiesInfo::try_from(ResponseParser::get_parameter(OutputAmpCapabilities, response)).unwrap();

            command = CommandBuilder::get_parameter(&fg_address, SupportedPowerStates);
            response = RegisterInterface::immediate_command(&crs, command);
            let supported_power_states = SupportedPowerStatesInfo::try_from(ResponseParser::get_parameter(SupportedPowerStates, response)).unwrap();

            command = CommandBuilder::get_parameter(&fg_address, GPIOCount);
            response = RegisterInterface::immediate_command(&crs, command);
            let gpio_count = GPIOCountInfo::try_from(ResponseParser::get_parameter(GPIOCount, response)).unwrap();

            let widgets = IHDA::scan_function_group_for_available_widgets(crs, &fg_address, &subordinate_node_count_info);

            fg_nodes.push(FunctionGroupNode::new(
                fg_address,
                subordinate_node_count_info,
                function_group_type_info,
                afg_caps,
                sample_size_rate_caps,
                stream_formats,
                input_amp_caps,
                output_amp_caps,
                supported_power_states,
                gpio_count,
                widgets));
        }
        fg_nodes
    }

    fn scan_function_group_for_available_widgets(
        crs: &ControllerRegisterSet,
        fg_addr: &NodeAddress,
        snci: &SubordinateNodeCountInfo
    ) -> Vec<WidgetNode> {
        let mut widgets: Vec<WidgetNode> = Vec::new();
        let codec_address = *fg_addr.codec_address();
        let mut command;
        let mut response;

        for node_id in *snci.starting_node_number()..(*snci.starting_node_number() + *snci.total_number_of_nodes()) {
            let widget_address = NodeAddress::new(codec_address, node_id);
            let widget_info: WidgetInfo;

            command = CommandBuilder::get_parameter(&widget_address, AudioWidgetCapabilities);
            response = RegisterInterface::immediate_command(&crs, command);
            let audio_widget_capabilities_info = AudioWidgetCapabilitiesInfo::try_from(ResponseParser::get_parameter(AudioWidgetCapabilities, response)).unwrap();

            match audio_widget_capabilities_info.widget_type() {
                WidgetType::AudioOutput => {
                    command = CommandBuilder::get_parameter(&widget_address, SampleSizeRateCAPs);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let ssrc_info = SampleSizeRateCAPsInfo::try_from(ResponseParser::get_parameter(SampleSizeRateCAPs, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, StreamFormats);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let sf_info = StreamFormatsInfo::try_from(ResponseParser::get_parameter(StreamFormats, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, OutputAmpCapabilities);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let output_amp_caps = AmpCapabilitiesInfo::try_from(ResponseParser::get_parameter(OutputAmpCapabilities, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, SupportedPowerStates);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let supported_power_states = SupportedPowerStatesInfo::try_from(ResponseParser::get_parameter(SupportedPowerStates, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, ProcessingCapabilities);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let processing_capabilities = ProcessingCapabilitiesInfo::try_from(ResponseParser::get_parameter(ProcessingCapabilities, response)).unwrap();

                    widget_info = WidgetInfo::AudioOutputConverter(ssrc_info, sf_info, output_amp_caps, supported_power_states, processing_capabilities);
                }
                WidgetType::AudioInput => {
                    command = CommandBuilder::get_parameter(&widget_address, SampleSizeRateCAPs);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let ssrc_info = SampleSizeRateCAPsInfo::try_from(ResponseParser::get_parameter(SampleSizeRateCAPs, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, StreamFormats);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let sf_info = StreamFormatsInfo::try_from(ResponseParser::get_parameter(StreamFormats, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, InputAmpCapabilities);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let input_amp_caps = AmpCapabilitiesInfo::try_from(ResponseParser::get_parameter(InputAmpCapabilities, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, ConnectionListLength);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let connection_list_length = ConnectionListLengthInfo::try_from(ResponseParser::get_parameter(ConnectionListLength, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, SupportedPowerStates);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let supported_power_states = SupportedPowerStatesInfo::try_from(ResponseParser::get_parameter(SupportedPowerStates, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, ProcessingCapabilities);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let processing_capabilities = ProcessingCapabilitiesInfo::try_from(ResponseParser::get_parameter(ProcessingCapabilities, response)).unwrap();

                    widget_info = WidgetInfo::AudioInputConverter(ssrc_info, sf_info, input_amp_caps, connection_list_length, supported_power_states, processing_capabilities);
                }
                WidgetType::AudioMixer => {
                    widget_info = WidgetInfo::Mixer;
                }
                WidgetType::AudioSelector => {
                    widget_info = WidgetInfo::Selector;
                }

                WidgetType::PinComplex => {
                    command = CommandBuilder::get_parameter(&widget_address, PinCapabilities);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let pin_caps = PinCapabilitiesInfo::try_from(ResponseParser::get_parameter(PinCapabilities, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, InputAmpCapabilities);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let input_amp_caps = AmpCapabilitiesInfo::try_from(ResponseParser::get_parameter(InputAmpCapabilities, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, OutputAmpCapabilities);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let output_amp_caps = AmpCapabilitiesInfo::try_from(ResponseParser::get_parameter(OutputAmpCapabilities, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, ConnectionListLength);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let connection_list_length = ConnectionListLengthInfo::try_from(ResponseParser::get_parameter(ConnectionListLength, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, SupportedPowerStates);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let supported_power_states = SupportedPowerStatesInfo::try_from(ResponseParser::get_parameter(SupportedPowerStates, response)).unwrap();

                    command = CommandBuilder::get_parameter(&widget_address, ProcessingCapabilities);
                    response = RegisterInterface::immediate_command(&crs, command);
                    let processing_capabilities = ProcessingCapabilitiesInfo::try_from(ResponseParser::get_parameter(ProcessingCapabilities, response)).unwrap();

                    widget_info = WidgetInfo::PinComplex(pin_caps, input_amp_caps, output_amp_caps, connection_list_length, supported_power_states, processing_capabilities);
                }
                WidgetType::PowerWidget => {
                    widget_info = WidgetInfo::Power;
                }
                WidgetType::VolumeKnobWidget => {
                    widget_info = WidgetInfo::VolumeKnob;
                }
                WidgetType::BeepGeneratorWidget => {
                    widget_info = WidgetInfo::BeepGenerator;
                }
                WidgetType::VendorDefinedAudioWidget => {
                    widget_info = WidgetInfo::VendorDefined;
                }
            }

            widgets.push(WidgetNode::new(widget_address, audio_widget_capabilities_info, widget_info));
        }
        widgets
    }
}

struct RegisterInterface;

impl RegisterInterface {
    fn immediate_command(crs: &ControllerRegisterSet, command: u32) -> u32 {
        crs.icis().write(0b10);
        crs.icoi().write(command);
        crs.icis().write(0b1);
        let start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        const ICIS_TIMEOUT: usize = 100;
        while (crs.icis().read() & 0b10) != 0b10 {
            if timer().read().systime_ms() > start_timer + ICIS_TIMEOUT {
                panic!("IHDA immediate command timed out")
            }
        }
        crs.icii().read()
    }
}
