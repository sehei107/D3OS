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
use crate::device::ihda_types::{AmpCapabilitiesInfo, AudioFunctionGroupCapabilitiesInfo, AudioWidgetCapabilitiesInfo, Codec, ConfigDefDefaultDevice, ConfigDefPortConnectivity, ConfigurationDefaultInfo, ConnectionListEntryInfo, ConnectionListLengthInfo, ConnectionSelectInfo, ControllerRegisterSet, FunctionGroupNode, FunctionGroupTypeInfo, GPIOCountInfo, NodeAddress, PinCapabilitiesInfo, ProcessingCapabilitiesInfo, RegisterInterface, RevisionIdInfo, RootNode, SampleSizeRateCAPsInfo, SupportedStreamFormatsInfo, SubordinateNodeCountInfo, SupportedPowerStatesInfo, VendorIdInfo, WidgetInfoContainer, WidgetNode, WidgetType, Parameter, StreamFormatInfo};
use crate::device::ihda_types::Command::{GetConfigurationDefault, GetConnectionListEntry, GetConnectionSelect, GetParameter, GetStreamFormat};
use crate::device::ihda_types::Parameter::{AudioFunctionGroupCapabilities, AudioWidgetCapabilities, ConnectionListLength, FunctionGroupType, GPIOCount, InputAmpCapabilities, OutputAmpCapabilities, PinCapabilities, ProcessingCapabilities, RevisionId, SampleSizeRateCAPs, SupportedStreamFormats, SubordinateNodeCount, SupportedPowerStates, VendorId};
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
        let register_interface = IHDA::connect_controller();

        info!("Initializing IHDA sound card");
        IHDA::reset_controller(register_interface.crs());
        info!("IHDA Controller reset complete");

        IHDA::setup_ihda_config_space(register_interface.crs());
        info!("IHDA configuration space set up");


        IHDA::init_corb(register_interface.crs());
        IHDA::init_rirb(register_interface.crs());
        IHDA::start_corb(register_interface.crs());
        IHDA::start_rirb(register_interface.crs());

        info!("CORB and RIRB set up and running");

        // interview sound card
        let codecs = IHDA::scan_for_available_codecs(&register_interface);
        debug!("[{}] codec{} found", codecs.len(), if codecs.len() == 1 { "" } else { "s" });

        // debug!("AFG Subordinate Node Count: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().subordinate_node_count());
        // debug!("AFG Function Group Type: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().function_group_type());
        // debug!("AFG Audio Function Group Capabilities: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().audio_function_group_caps());
        // debug!("AFG Sample Size, Rate CAPs: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().sample_size_rate_caps());
        // debug!("AFG Stream Formats: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().stream_formats());
        // debug!("AFG Input Amp Capabilities: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().input_amp_caps());
        // debug!("AFG Output Amp Capabilities: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().output_amp_caps());
        // debug!("AFG Supported Power States: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().supported_power_states());
        // debug!("AFG Supported GPIO Count: {:?}", codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().gpio_count());

        // wait a bit to have tim to read each print

        debug!("VENDOR ID: {:?}", codecs.get(0).unwrap().root_node().vendor_id());
        debug!("REVISION ID: {:?}", codecs.get(0).unwrap().root_node().revision_id());

        debug!("Find all widgets in first audio function group:");
        for widget in codecs.get(0).unwrap().root_node().function_group_nodes().get(0).unwrap().widgets().iter() {
            // debug!("{:?} found, id: {:?}, channel count: {:?}", widget.audio_widget_capabilities().widget_type(), widget.address().node_id(), widget.max_number_of_channels());

            match widget.audio_widget_capabilities().widget_type() {
                WidgetType::AudioOutput => {
                    debug!("audio output converter widget {:?}:", widget.address());
                    debug!("channel count {:?}:", widget.max_number_of_channels());
                    debug!("stream format {:?}:", StreamFormatInfo::try_from(register_interface.send_command(widget.address(), &GetStreamFormat)).unwrap());

                }
                WidgetType::AudioInput => {}
                WidgetType::AudioMixer => {
                    // if (*widget.address().node_id() == 12) | (*widget.address().node_id() == 13) {
                    //     let config_defaults = ConfigurationDefaultInfo::try_from(register_interface.send_command(widget.address(), &GetConfigurationDefault)).unwrap();
                    //
                    //     debug!("mixer widget {:?}:", widget.address());
                    //     debug!("channel count {:?}:", widget.max_number_of_channels());
                    //     debug!("connection list length: {:?}", ConnectionListLengthInfo::try_from(register_interface.send_command(widget.address(), &GetParameter(ConnectionListLength))).unwrap());
                    //     debug!("first connection list entries: {:?}", ConnectionListEntryInfo::try_from(register_interface.send_command(widget.address(), &GetConnectionListEntry { offset: 0 })).unwrap());
                    //     debug!("connection select: {:?}", ConnectionSelectInfo::try_from(register_interface.send_command(widget.address(), &GetConnectionSelect)).unwrap());
                    //
                    //     debug!("port connectivity: {:?}, default device: {:?}, default association: {:?}, sequence: {:?}",
                    //             config_defaults.port_connectivity(), config_defaults.default_device(), config_defaults.default_association(), config_defaults.sequence());
                    // }

                }
                WidgetType::AudioSelector => {}
                WidgetType::PinComplex => {
                    // let config_defaults = ConfigurationDefaultInfo::try_from(register_interface.send_command(widget.address(), &GetConfigurationDefault)).unwrap();
                    // match config_defaults.port_connectivity() {
                    //     ConfigDefPortConnectivity::Jack => {
                    //         if *widget.address().node_id() == 27 {
                    //             debug!("pin widget {:?}:", widget.address());
                    //             debug!("EAPD capable: {:?}", PinCapabilitiesInfo::try_from(register_interface.send_command(widget.address(), &GetParameter(PinCapabilities))).unwrap().eapd_capable());
                    //             debug!("channel count {:?}:", widget.max_number_of_channels());
                    //             debug!("connection list length: {:?}:",
                    //                 match widget.widget_info() {
                    //                     WidgetInfoContainer::PinComplex(_,_,_,cll,_,_) => { cll },
                    //                     _ => { panic!() },
                    //                 });
                    //             debug!("first connection list entries: {:?}", ConnectionListEntryInfo::try_from(register_interface.send_command(widget.address(), &GetConnectionListEntry { offset: 0 })).unwrap());
                    //             debug!("connection select: {:?}", ConnectionSelectInfo::try_from(register_interface.send_command(widget.address(), &GetConnectionSelect)).unwrap());
                    //
                    //             debug!("port connectivity: {:?}, default device: {:?}, default association: {:?}, sequence: {:?}",
                    //             config_defaults.port_connectivity(), config_defaults.default_device(), config_defaults.default_association(), config_defaults.sequence());
                    //         }
                    //     }
                    //     _ => {}
                    // }
                }
                WidgetType::PowerWidget => {}
                WidgetType::VolumeKnobWidget => {}
                WidgetType::BeepGeneratorWidget => {}
                WidgetType::VendorDefinedAudioWidget => {}
            }
            Timer::wait(2000);
        }

        // wait ten minutes, so you can read the previous prints on real hardware where you can't set breakpoints with a debugger
        Timer::wait(600000);

        IHDA {}
    }

    fn connect_controller() -> RegisterInterface {
        let pci = pci_bus();

        // find ihda devices
        let ihda_devices = pci.search_by_class(PCI_MULTIMEDIA_DEVICE, PCI_IHDA_DEVICE);
        debug!("[{}] IHDA device{} found", ihda_devices.len(), if ihda_devices.len() == 1 { "" } else { "s" });


        if ihda_devices.len() > 0 {
            let device;
            // temporarily hard coded
            if ihda_devices.len() == 1 {
                // QEMU setup
                device = ihda_devices[0];
            } else {
                // university testing device setup
                device = ihda_devices[1];
            }

            let bar0 = device.bar(0, pci.config_space()).unwrap();

            let mmio_base_address: u64;
            let mmio_size: u64;

            match bar0 {
                Bar::Memory32 { address, size, .. } => {
                    mmio_base_address = address as u64;
                    mmio_size = size as u64;
                }
                Bar::Memory64 { address, size, prefetchable: _ } => {
                    mmio_base_address = address;
                    mmio_size = size;
                }
                Bar::Io { .. } => {
                    panic!("Driver doesn't support port i/o! ")
                }
            }

            // set BME bit in command register of PCI configuration space
            device.update_command(pci.config_space(), |command| {
                command.bitor(CommandRegister::BUS_MASTER_ENABLE)
            });

            // set Memory Space bit in command register of PCI configuration space (so that hardware can respond to memory space access)
            device.update_command(pci.config_space(), |command| {
                command.bitor(CommandRegister::MEMORY_ENABLE)
            });

            // setup MMIO space (currently one-to-one mapping from physical address space to virtual address space of kernel)
            let pages = mmio_size as usize / PAGE_SIZE;
            let mmio_page = Page::from_start_address(VirtAddr::new(mmio_base_address)).expect("IHDA MMIO address is not page aligned!");
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

            return RegisterInterface::new(mmio_base_address);
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

    fn init_corb(crs: &ControllerRegisterSet) {
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

        // the following call leads to panic in QEMU because of timeout, but it seems to work on real hardware without a reset...
        // IHDA::reset_corb(crs);
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

    fn init_rirb(crs: &ControllerRegisterSet) {
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
    fn scan_for_available_codecs(register_interface: &RegisterInterface) -> Vec<Codec> {
        let mut codecs: Vec<Codec> = Vec::new();
        for index in 0..MAX_AMOUNT_OF_CODECS {
            if register_interface.crs().wakests().assert_bit(index) {
                let root_node_addr = NodeAddress::new(index, 0x0);

                let vendor_id_info = VendorIdInfo::try_from(register_interface.send_command(&root_node_addr, &GetParameter(VendorId))).unwrap();
                let revision_id_info = RevisionIdInfo::try_from(register_interface.send_command(&root_node_addr, &GetParameter(RevisionId))).unwrap();
                let subordinate_node_count_info = SubordinateNodeCountInfo::try_from(register_interface.send_command(&root_node_addr, &GetParameter(SubordinateNodeCount))).unwrap();

                let function_group_nodes = IHDA::scan_codec_for_available_function_groups(register_interface, &root_node_addr, &subordinate_node_count_info);

                let root_node = RootNode::new(index, vendor_id_info, revision_id_info, subordinate_node_count_info, function_group_nodes);
                codecs.push(Codec::new(index, root_node));
            }
        }
        codecs
    }

    fn scan_codec_for_available_function_groups(
        register_interface: &RegisterInterface,
        root_node_addr: &NodeAddress,
        snci: &SubordinateNodeCountInfo
    ) -> Vec<FunctionGroupNode> {
        let mut fg_nodes: Vec<FunctionGroupNode> = Vec::new();
        let codec_address = *root_node_addr.codec_address();

        for node_id in *snci.starting_node_number()..(*snci.starting_node_number() + *snci.total_number_of_nodes()) {
            let fg_address = NodeAddress::new(codec_address, node_id);

            let subordinate_node_count_info = SubordinateNodeCountInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(SubordinateNodeCount))).unwrap();
            let function_group_type_info = FunctionGroupTypeInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(FunctionGroupType))).unwrap();
            let afg_caps = AudioFunctionGroupCapabilitiesInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(AudioFunctionGroupCapabilities))).unwrap();
            let sample_size_rate_caps = SampleSizeRateCAPsInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(SampleSizeRateCAPs))).unwrap();
            let supported_stream_formats = SupportedStreamFormatsInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(SupportedStreamFormats))).unwrap();
            let input_amp_caps = AmpCapabilitiesInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(InputAmpCapabilities))).unwrap();
            let output_amp_caps = AmpCapabilitiesInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(OutputAmpCapabilities))).unwrap();
            let supported_power_states = SupportedPowerStatesInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(SupportedPowerStates))).unwrap();
            let gpio_count = GPIOCountInfo::try_from(register_interface.send_command(&fg_address, &GetParameter(GPIOCount))).unwrap();

            let widgets = IHDA::scan_function_group_for_available_widgets(register_interface, &fg_address, &subordinate_node_count_info);

            fg_nodes.push(FunctionGroupNode::new(
                fg_address,
                subordinate_node_count_info,
                function_group_type_info,
                afg_caps,
                sample_size_rate_caps,
                supported_stream_formats,
                input_amp_caps,
                output_amp_caps,
                supported_power_states,
                gpio_count,
                widgets));
        }
        fg_nodes
    }

    fn scan_function_group_for_available_widgets(
        register_interface: &RegisterInterface,
        fg_addr: &NodeAddress,
        snci: &SubordinateNodeCountInfo
    ) -> Vec<WidgetNode> {
        let mut widgets: Vec<WidgetNode> = Vec::new();
        let codec_address = *fg_addr.codec_address();

        for node_id in *snci.starting_node_number()..(*snci.starting_node_number() + *snci.total_number_of_nodes()) {
            let widget_address = NodeAddress::new(codec_address, node_id);
            let widget_info: WidgetInfoContainer;
            let audio_widget_capabilities_info = AudioWidgetCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(AudioWidgetCapabilities))).unwrap();

            match audio_widget_capabilities_info.widget_type() {
                WidgetType::AudioOutput => {
                    let ssrc_info = SampleSizeRateCAPsInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(SampleSizeRateCAPs))).unwrap();
                    let sf_info = SupportedStreamFormatsInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(SupportedStreamFormats))).unwrap();
                    let output_amp_caps = AmpCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(OutputAmpCapabilities))).unwrap();
                    let supported_power_states = SupportedPowerStatesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(ProcessingCapabilities))).unwrap();

                    widget_info = WidgetInfoContainer::AudioOutputConverter(ssrc_info, sf_info, output_amp_caps, supported_power_states, processing_capabilities);
                }
                WidgetType::AudioInput => {
                    let ssrc_info = SampleSizeRateCAPsInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(SampleSizeRateCAPs))).unwrap();
                    let sf_info = SupportedStreamFormatsInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(SupportedStreamFormats))).unwrap();
                    let input_amp_caps = AmpCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(InputAmpCapabilities))).unwrap();
                    let connection_list_length = ConnectionListLengthInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(ConnectionListLength))).unwrap();
                    let supported_power_states = SupportedPowerStatesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(ProcessingCapabilities))).unwrap();

                    widget_info = WidgetInfoContainer::AudioInputConverter(ssrc_info, sf_info, input_amp_caps, connection_list_length, supported_power_states, processing_capabilities);
                }
                WidgetType::AudioMixer => {
                    widget_info = WidgetInfoContainer::Mixer;
                }
                WidgetType::AudioSelector => {
                    widget_info = WidgetInfoContainer::Selector;
                }

                WidgetType::PinComplex => {
                    let pin_caps = PinCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(PinCapabilities))).unwrap();
                    let input_amp_caps = AmpCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(InputAmpCapabilities))).unwrap();
                    let output_amp_caps = AmpCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(OutputAmpCapabilities))).unwrap();
                    let connection_list_length = ConnectionListLengthInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(ConnectionListLength))).unwrap();
                    let supported_power_states = SupportedPowerStatesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesInfo::try_from(register_interface.send_command(&widget_address, &GetParameter(ProcessingCapabilities))).unwrap();

                    widget_info = WidgetInfoContainer::PinComplex(pin_caps, input_amp_caps, output_amp_caps, connection_list_length, supported_power_states, processing_capabilities);
                }
                WidgetType::PowerWidget => {
                    widget_info = WidgetInfoContainer::Power;
                }
                WidgetType::VolumeKnobWidget => {
                    widget_info = WidgetInfoContainer::VolumeKnob;
                }
                WidgetType::BeepGeneratorWidget => {
                    widget_info = WidgetInfoContainer::BeepGenerator;
                }
                WidgetType::VendorDefinedAudioWidget => {
                    widget_info = WidgetInfoContainer::VendorDefined;
                }
            }

            widgets.push(WidgetNode::new(widget_address, audio_widget_capabilities_info, widget_info));
        }
        widgets
    }
}
