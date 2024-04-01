#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::arch::asm;
use core::ops::BitOr;
use log::{debug, info};
use pci_types::{Bar, BaseClass, CommandRegister, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::interrupt::interrupt_handler::InterruptHandler;
use crate::{apic, interrupt_dispatcher, memory, pci_bus, process_manager, timer};
use crate::device::ihda_node_communication::{AmpCapabilitiesResponse, AudioFunctionGroupCapabilitiesResponse, AudioWidgetCapabilitiesResponse, ConfigDefDefaultDevice, ConfigDefPortConnectivity, ConfigurationDefaultResponse, ConnectionListEntryResponse, ConnectionListLengthResponse, FunctionGroupTypeResponse, GPIOCountResponse, PinCapabilitiesResponse, ProcessingCapabilitiesResponse, RevisionIdResponse, SampleSizeRateCAPsResponse, SupportedStreamFormatsResponse, SubordinateNodeCountResponse, SupportedPowerStatesResponse, VendorIdResponse, WidgetType, StreamFormatResponse, ChannelStreamIdResponse, PinWidgetControlResponse, VoltageReferenceSignalLevel, GetConnectionListEntryPayload, SetAmplifierGainMuteSide, SetAmplifierGainMuteType, SetPinWidgetControlPayload, SetAmplifierGainMutePayload, SetChannelStreamIdPayload, SetStreamFormatPayload};
use crate::device::ihda_node_communication::Command::{GetChannelStreamId, GetConfigurationDefault, GetConnectionListEntry, GetParameter, GetPinWidgetControl, GetStreamFormat, SetAmplifierGainMute, SetChannelStreamId, SetPinWidgetControl};
use crate::device::ihda_node_communication::Parameter::{AudioFunctionGroupCapabilities, AudioWidgetCapabilities, ConnectionListLength, FunctionGroupType, GPIOCount, InputAmpCapabilities, OutputAmpCapabilities, PinCapabilities, ProcessingCapabilities, RevisionId, SampleSizeRateCAPs, SubordinateNodeCount, SupportedPowerStates, SupportedStreamFormats, VendorId};
use crate::device::ihda_types::{Codec, ControllerRegisterSet, FunctionGroupNode, NodeAddress, RegisterInterface, RootNode, WidgetInfoContainer, WidgetNode, BufferDescriptorList, BufferDescriptorListEntry};
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

        IHDA::prepare_default_stereo_output(&register_interface, &codecs.get(0).unwrap());

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
                    debug!("stream format {:?}:", StreamFormatResponse::try_from(register_interface.send_command(&GetStreamFormat(widget.address().clone()))).unwrap());

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

                let vendor_id_info = VendorIdResponse::try_from(register_interface.send_command(&GetParameter(root_node_addr.clone(), VendorId))).unwrap();
                let revision_id_info = RevisionIdResponse::try_from(register_interface.send_command(&GetParameter(root_node_addr.clone(), RevisionId))).unwrap();
                let subordinate_node_count_info = SubordinateNodeCountResponse::try_from(register_interface.send_command(&GetParameter(root_node_addr.clone(), SubordinateNodeCount))).unwrap();

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
        snci: &SubordinateNodeCountResponse
    ) -> Vec<FunctionGroupNode> {
        let mut fg_nodes: Vec<FunctionGroupNode> = Vec::new();
        let codec_address = *root_node_addr.codec_address();

        for node_id in *snci.starting_node_number()..(*snci.starting_node_number() + *snci.total_number_of_nodes()) {
            let fg_address = NodeAddress::new(codec_address, node_id);

            let subordinate_node_count_info = SubordinateNodeCountResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), SubordinateNodeCount))).unwrap();
            let function_group_type_info = FunctionGroupTypeResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), FunctionGroupType))).unwrap();
            let afg_caps = AudioFunctionGroupCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), AudioFunctionGroupCapabilities))).unwrap();
            let sample_size_rate_caps = SampleSizeRateCAPsResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), SampleSizeRateCAPs))).unwrap();
            let supported_stream_formats = SupportedStreamFormatsResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), SupportedStreamFormats))).unwrap();
            let input_amp_caps = AmpCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), InputAmpCapabilities))).unwrap();
            let output_amp_caps = AmpCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), OutputAmpCapabilities))).unwrap();
            let supported_power_states = SupportedPowerStatesResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), SupportedPowerStates))).unwrap();
            let gpio_count = GPIOCountResponse::try_from(register_interface.send_command(&GetParameter(fg_address.clone(), GPIOCount))).unwrap();

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
        snci: &SubordinateNodeCountResponse
    ) -> Vec<WidgetNode> {
        let mut widgets: Vec<WidgetNode> = Vec::new();
        let codec_address = *fg_addr.codec_address();

        for node_id in *snci.starting_node_number()..(*snci.starting_node_number() + *snci.total_number_of_nodes()) {
            let widget_address = NodeAddress::new(codec_address, node_id);
            let widget_info: WidgetInfoContainer;
            let audio_widget_capabilities_info = AudioWidgetCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), AudioWidgetCapabilities))).unwrap();

            match audio_widget_capabilities_info.widget_type() {
                WidgetType::AudioOutput => {
                    let ssrc_info = SampleSizeRateCAPsResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), SampleSizeRateCAPs))).unwrap();
                    let sf_info = SupportedStreamFormatsResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), SupportedStreamFormats))).unwrap();
                    let output_amp_caps = AmpCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), OutputAmpCapabilities))).unwrap();
                    let supported_power_states = SupportedPowerStatesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), ProcessingCapabilities))).unwrap();

                    widget_info = WidgetInfoContainer::AudioOutputConverter(ssrc_info, sf_info, output_amp_caps, supported_power_states, processing_capabilities);
                }
                WidgetType::AudioInput => {
                    let ssrc_info = SampleSizeRateCAPsResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), SampleSizeRateCAPs))).unwrap();
                    let sf_info = SupportedStreamFormatsResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), SupportedStreamFormats))).unwrap();
                    let input_amp_caps = AmpCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), InputAmpCapabilities))).unwrap();
                    let connection_list_length = ConnectionListLengthResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), ConnectionListLength))).unwrap();
                    let supported_power_states = SupportedPowerStatesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), ProcessingCapabilities))).unwrap();

                    widget_info = WidgetInfoContainer::AudioInputConverter(ssrc_info, sf_info, input_amp_caps, connection_list_length, supported_power_states, processing_capabilities);
                }
                WidgetType::AudioMixer => {
                    widget_info = WidgetInfoContainer::Mixer;
                }
                WidgetType::AudioSelector => {
                    widget_info = WidgetInfoContainer::Selector;
                }

                WidgetType::PinComplex => {
                    let pin_caps = PinCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), PinCapabilities))).unwrap();
                    let input_amp_caps = AmpCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), InputAmpCapabilities))).unwrap();
                    let output_amp_caps = AmpCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), OutputAmpCapabilities))).unwrap();
                    let connection_list_length = ConnectionListLengthResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), ConnectionListLength))).unwrap();
                    let supported_power_states = SupportedPowerStatesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesResponse::try_from(register_interface.send_command(&GetParameter(widget_address.clone(), ProcessingCapabilities))).unwrap();
                    let configuration_default = ConfigurationDefaultResponse::try_from(register_interface.send_command(&GetConfigurationDefault(widget_address.clone()))).unwrap();
                    let first_connection_list_entries = ConnectionListEntryResponse::try_from(register_interface.send_command(&GetConnectionListEntry(widget_address.clone(), GetConnectionListEntryPayload::new(0)))).unwrap();

                    widget_info = WidgetInfoContainer::PinComplex(pin_caps, input_amp_caps, output_amp_caps, connection_list_length, supported_power_states, processing_capabilities, configuration_default, first_connection_list_entries);
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

    fn prepare_default_stereo_output(register_interface: &RegisterInterface, codec: &Codec) {
        let widgets = codec.root_node().function_group_nodes().get(0).unwrap().widgets();
        let line_out_pin_widgets_connected_to_jack = Self::find_line_out_pin_widgets_connected_to_jack(widgets);
        let default_output = *line_out_pin_widgets_connected_to_jack.get(0).unwrap();

        Self::default_stereo_setup(default_output, register_interface, codec);

    }

    fn find_line_out_pin_widgets_connected_to_jack(widgets: &Vec<WidgetNode>) -> Vec<&WidgetNode> {
        let mut pin_widgets_connected_to_jack = Vec::new();
        for widget in widgets.iter() {
            match widget.audio_widget_capabilities().widget_type() {
                WidgetType::PinComplex => {
                    let config_defaults = match widget.widget_info() {
                        WidgetInfoContainer::PinComplex(_, _, _, _, _, _, config_default, _) => {
                            config_default
                        }
                        _ => {
                            panic!("This arm should never be reached!")
                        }
                    };
                    match config_defaults.port_connectivity() {
                        ConfigDefPortConnectivity::Jack | ConfigDefPortConnectivity::JackAndInternalDevice => {
                            match config_defaults.default_device() {
                                ConfigDefDefaultDevice::LineOut => {
                                    pin_widgets_connected_to_jack.push(widget);
                                }
                                _ => {},
                            }
                        }
                        _ => {},
                    }
                }
                _ => {},
            }
        }

        pin_widgets_connected_to_jack
    }

    fn default_stereo_setup(pin_widget: &WidgetNode, register_interface: &RegisterInterface, codec: &Codec) {

        // set gain/mute for pin widget (observation: pin widget owns input and output amp; for both, gain stays at 0, no matter what value gets set, but mute reacts to set commands)
        debug!("pin widget: {:?}", pin_widget.address());
        register_interface.send_command(&SetAmplifierGainMute(pin_widget.address().clone(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Both, SetAmplifierGainMuteSide::Both, 0, false, 100)));
        // debug!("input amp_gain_mute left after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(pin_widget.address(), &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Input, side: GetAmplifierGainMuteSide::Left, index: 0 })).unwrap());
        // debug!("input amp_gain_mute right after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(pin_widget.address(), &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Input, side: GetAmplifierGainMuteSide::Right, index: 0 })).unwrap());
        // debug!("output amp_gain_mute left after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(pin_widget.address(), &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Left, index: 0 })).unwrap());
        // debug!("output amp_gain_mute right after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(pin_widget.address(), &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Right, index: 0 })).unwrap());

        // activate input and output for pin widget
        let pin_widget_control = PinWidgetControlResponse::try_from(register_interface.send_command(&GetPinWidgetControl(pin_widget.address().clone()))).unwrap();
        // debug!("pin widget control before: {:?}", pin_widget_control);
        /* after the following command, plugging headphones in and out the jack should make an audible noise */
        register_interface.send_command(&SetPinWidgetControl(pin_widget.address().clone(), SetPinWidgetControlPayload::new(
            match pin_widget_control.voltage_reference_enable() {
                VoltageReferenceSignalLevel::HiZ => VoltageReferenceSignalLevel::HiZ,
                VoltageReferenceSignalLevel::FiftyPercent => VoltageReferenceSignalLevel::FiftyPercent,
                VoltageReferenceSignalLevel::Ground0V => VoltageReferenceSignalLevel::Ground0V,
                VoltageReferenceSignalLevel::EightyPercent => VoltageReferenceSignalLevel::EightyPercent,
                VoltageReferenceSignalLevel::HundredPercent => VoltageReferenceSignalLevel::HundredPercent,
            },
            true,
            true,
            *pin_widget_control.h_phn_enable()
        )));
        // debug!("pin widget control after: {:?}", PinWidgetControlInfo::try_from(register_interface.send_command(&pin_widget.address(), &GetPinWidgetControl)).unwrap());

        // let eapd = EAPDBTLEnableInfo::try_from(register_interface.send_command(&pin_widget.address(), &GetEAPDBTLEnable)).unwrap();
        // debug!("eapd before: {:?}", eapd);
        // register_interface.send_command(&pin_widget.address(), &SetEAPDBTLEnable(EAPDBTLEnableInfo::new(0b111)));
        // debug!("eapd after: {:?}", EAPDBTLEnableInfo::try_from(register_interface.send_command(&pin_widget.address(), &GetEAPDBTLEnable)).unwrap());
        // Timer::wait(10000);

        let connection_list_entries = ConnectionListEntryResponse::try_from(register_interface.send_command(&GetConnectionListEntry(pin_widget.address().clone(), GetConnectionListEntryPayload::new(0)))).unwrap();
        let mixer_widget = NodeAddress::new(0, *connection_list_entries.connection_list_entry_at_offset_index());

        // set gain/mute for mixer widget (observation: mixer widget only owns input amp; gain stays at 0, no matter what value gets set, but mute reacts to set commands)
        debug!("mixer widget: {:?}", mixer_widget);
        // debug!("input amp_gain_mute left before: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&mixer_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Input, side: GetAmplifierGainMuteSide::Left, index: 0 })).unwrap());
        // debug!("input amp_gain_mute right before: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&mixer_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Input, side: GetAmplifierGainMuteSide::Right, index: 0 })).unwrap());
        // debug!("output amp_gain_mute left before: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&mixer_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Left, index: 0 })).unwrap());
        // debug!("output amp_gain_mute right before: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&mixer_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Right, index: 0 })).unwrap());
        register_interface.send_command(&SetAmplifierGainMute(mixer_widget.clone(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Input, SetAmplifierGainMuteSide::Both, 0, false, 100)));
        // debug!("input amp_gain_mute left after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&mixer_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Input, side: GetAmplifierGainMuteSide::Left, index: 0 })).unwrap());
        // debug!("input amp_gain_mute right after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&mixer_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Input, side: GetAmplifierGainMuteSide::Right, index: 0 })).unwrap());
        // debug!("output amp_gain_mute left after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&mixer_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Left, index: 0 })).unwrap());
        // debug!("output amp_gain_mute right after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&mixer_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Right, index: 0 })).unwrap());


        let connection_list_entries = ConnectionListEntryResponse::try_from(register_interface.send_command(&GetConnectionListEntry(mixer_widget.clone(), GetConnectionListEntryPayload::new(0)))).unwrap();
        let audio_out_widget = NodeAddress::new(0, *connection_list_entries.connection_list_entry_at_offset_index());

        // set gain/mute for audio output converter widget (observation: audio output converter widget only owns output amp; mute stays false, no matter what value gets set, but gain reacts to set commands)
        // careful: the gain register is only 7 bits long (bits [6:0]), so the max gain value is 127; writing higher numbers into the u8 for gain will overwrite the mute bit at position 7
        // default gain value is 87
        debug!("audio out widget: {:?}", audio_out_widget);
        // debug!("output amp_gain_mute left before: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&audio_out_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Left, index: 0 })).unwrap());
        // debug!("output amp_gain_mute right before: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&audio_out_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Right, index: 0 })).unwrap());
        register_interface.send_command(&SetAmplifierGainMute(audio_out_widget.clone(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Both, SetAmplifierGainMuteSide::Both, 0, false, 127)));
        // debug!("output amp_gain_mute left after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&audio_out_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Left, index: 0 })).unwrap());
        // debug!("output amp_gain_mute right after: {:?}", AmplifierGainMuteInfo::try_from(register_interface.send_command(&audio_out_widget, &GetAmplifierGainMute { amp_type: GetAmplifierGainMuteType::Output, side: GetAmplifierGainMuteSide::Right, index: 0 })).unwrap());

        // set stream id to 1
        debug!("channel stream id before: {:?}", ChannelStreamIdResponse::try_from(register_interface.send_command(&GetChannelStreamId(audio_out_widget.clone()))).unwrap());
        register_interface.send_command(&SetChannelStreamId(audio_out_widget.clone(), SetChannelStreamIdPayload::new(0, 1)));
        debug!("channel stream id after: {:?}", ChannelStreamIdResponse::try_from(register_interface.send_command(&GetChannelStreamId(audio_out_widget.clone()))).unwrap());

        // set stream descriptor
        let sd_registers = register_interface.crs().output_stream_descriptors().get(0).unwrap();

        debug!("----------------------------------------------------------------------------------");
        debug!("sdctl: {:#x}", sd_registers.sdctl().read());
        debug!("sdsts: {:#x}", sd_registers.sdsts().read());
        debug!("sdlpib: {:#x}", sd_registers.sdlpib().read());
        debug!("sdcbl: {:#x}", sd_registers.sdcbl().read());
        debug!("sdlvi: {:#x}", sd_registers.sdlvi().read());
        debug!("sdfifod: {:#x}", sd_registers.sdfifod().read());
        debug!("sdfmt: {:#x}", sd_registers.sdfmt().read());
        debug!("sdbdpl: {:#x}", sd_registers.sdbdpl().read());
        debug!("sdbdpu: {:#x}", sd_registers.sdbdpu().read());

        // stop stream in case it is running
        sd_registers.sdctl().clear_bit(1);

        // reset stream
        // sd_registers.sdctl().set_bit(0);
        // sd_registers.sdctl().write(sd_registers.sdctl().read() & 0xFF1F_FFFD);

        // set stream number
        sd_registers.sdctl().write(sd_registers.sdctl().read() | 0x10_0000);

        // setup MMIO space for buffer descriptor list
        // hard coded 8*4096 for 256 entries with 128 bits each
        let bdl_frame_range = memory::physical::alloc(1);

        debug!("bdl_base_address: {}", bdl_frame_range.start.start_address().as_u64());

        match bdl_frame_range {
            PhysFrameRange { start, end: _ } => {
                let start_address = start.start_address().as_u64();
                let lbase = (start_address & 0xFFFFFFFF) as u32;
                let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;
                sd_registers.sdbdpl().write(lbase);
                sd_registers.sdbdpu().write(ubase);
            }
        }
        unsafe { asm!("wbinvd"); }
        debug!("wbinvd");

        let bdl = BufferDescriptorList::new(bdl_frame_range);

        debug!("buffer descriptor list: {:?}", bdl);

        let data_buffer0 = BufferDescriptorListEntry::new(memory::physical::alloc(1), true);
        let data_buffer1 = BufferDescriptorListEntry::new(memory::physical::alloc(1), true);

        bdl.set_entry(0, &data_buffer0);
        bdl.set_entry(1, &data_buffer1);
        unsafe { asm!("wbinvd"); }
        debug!("wbinvd");

        debug!("bdl entry 0: {:?}", bdl.get_entry(0));
        debug!("bdl entry 1: {:?}", bdl.get_entry(1));
        debug!("bdl entry 2: {:?}", bdl.get_entry(2));

        debug!("data_buffer0 address: {:?}", data_buffer0.address());
        debug!("data_buffer1 address: {:?}", data_buffer1.address());
        debug!("data_buffer0 address: {:?}", data_buffer0.length_in_bytes());
        debug!("data_buffer1 address: {:?}", data_buffer1.length_in_bytes());

        for index in 0..(data_buffer0.length_in_bytes() / 4) {
            data_buffer0.set_buffer_entry(index, 0b1111_1111_1111_1111_1111_1111_0000_0000);
            data_buffer1.set_buffer_entry(index, 0b1111_1111_0000_0000);
        }

        for index in 0..5 {
            debug!("data_buffer0 sample at index {}: {}", index, data_buffer0.get_buffer_entry(index));
            debug!("data_buffer1 sample at index {}: {}", index, data_buffer1.get_buffer_entry(index));
        }

        Timer::wait(20000);

        data_buffer0.get_buffer_entry(0);

        // set cyclic buffer length
        sd_registers.sdcbl().write(*data_buffer0.length_in_bytes() + *data_buffer1.length_in_bytes());
        sd_registers.sdlvi().write(1);

        // set stream format
        let stream_format = StreamFormatResponse::try_from(register_interface.send_command(&GetStreamFormat(audio_out_widget.clone()))).unwrap();
        sd_registers.sdfmt().write(SetStreamFormatPayload::from_response(stream_format).as_u16());

        // run
        sd_registers.sdctl().set_bit(1);

        debug!("----------------------------------------------------------------------------------");
        debug!("sdctl: {:#x}", sd_registers.sdctl().read());
        debug!("sdsts: {:#x}", sd_registers.sdsts().read());
        debug!("sdlpib: {:#x}", sd_registers.sdlpib().read());
        debug!("sdcbl: {:#x}", sd_registers.sdcbl().read());
        debug!("sdlvi: {:#x}", sd_registers.sdlvi().read());
        debug!("sdfifod: {:#x}", sd_registers.sdfifod().read());
        debug!("sdfmt: {:#x}", sd_registers.sdfmt().read());
        debug!("sdbdpl: {:#x}", sd_registers.sdbdpl().read());
        debug!("sdbdpu: {:#x}", sd_registers.sdbdpu().read());

        Timer::wait(60000);
    }

    fn allocate_data_buffer() -> PhysFrameRange {
        // let container_size_in_bits = match stream_format_info.bits_per_sample() {
        //     BitsPerSample::Eight => 8,
        //     BitsPerSample::Sixteen => 16,
        //     BitsPerSample::Twenty => 16,
        //     BitsPerSample::Twentyfour => 32,
        //     BitsPerSample::Thirtytwo => 32,
        // };
        // let block_size_in_bits = container_size_in_bits * stream_format_info.number_of_channels();
        // let packet_size_in_bits = block_size_in_bits * stream_format_info.sample_base_rate_multiple() / stream_format_info.sample_base_rate_divisor();


        memory::physical::alloc(1)
    }
}
