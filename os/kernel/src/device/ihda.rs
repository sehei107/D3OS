#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::arch::asm;
use core::ops::BitOr;
use log::{debug, info};
use pci_types::{Bar, BaseClass, CommandRegister, EndpointHeader, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::interrupt::interrupt_handler::InterruptHandler;
use crate::{apic, interrupt_dispatcher, pci_bus, process_manager};
use crate::device::ihda_codec::{AmpCapabilitiesResponse, AudioFunctionGroupCapabilitiesResponse, AudioWidgetCapabilitiesResponse, ConfigDefDefaultDevice, ConfigDefPortConnectivity, ConfigurationDefaultResponse, ConnectionListEntryResponse, ConnectionListLengthResponse, FunctionGroupTypeResponse, GPIOCountResponse, PinCapabilitiesResponse, ProcessingCapabilitiesResponse, RevisionIdResponse, SampleSizeRateCAPsResponse, SupportedStreamFormatsResponse, SubordinateNodeCountResponse, SupportedPowerStatesResponse, VendorIdResponse, WidgetType, PinWidgetControlResponse, VoltageReferenceSignalLevel, GetConnectionListEntryPayload, SetAmplifierGainMuteSide, SetAmplifierGainMuteType, SetPinWidgetControlPayload, SetAmplifierGainMutePayload, SetChannelStreamIdPayload, SetStreamFormatPayload, BitsPerSample, StreamType};
use crate::device::ihda_codec::Command::{GetConfigurationDefault, GetConnectionListEntry, GetParameter, GetPinWidgetControl, SetAmplifierGainMute, SetChannelStreamId, SetPinWidgetControl, SetStreamFormat};
use crate::device::ihda_codec::Parameter::{AudioFunctionGroupCapabilities, AudioWidgetCapabilities, ConnectionListLength, FunctionGroupType, GPIOCount, InputAmpCapabilities, OutputAmpCapabilities, PinCapabilities, ProcessingCapabilities, RevisionId, SampleSizeRateCAPs, SubordinateNodeCount, SupportedPowerStates, SupportedStreamFormats, VendorId};
use crate::device::ihda_controller::{ControllerRegisterInterface, BufferDescriptorList, alloc_no_cache_dma_memory, CyclicBuffer, StreamDescriptorRegisters, Stream};
use crate::device::ihda_codec::{Codec, FunctionGroupNode, NodeAddress, RootNode, WidgetInfoContainer, WidgetNode};
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

        register_interface.init_corb();
        register_interface.init_rirb();
        register_interface.start_corb();
        register_interface.start_rirb();

        info!("CORB and RIRB set up and running");

        // interview sound card
        let codecs = IHDA::scan_for_available_codecs(&register_interface);

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

    // check the bitmask from bits 0 to 14 of the WAKESTS (in the specification also called STATESTS) indicating available codecs
    // then find all function group nodes and widgets associated with a codec
    fn scan_for_available_codecs(register_interface: &ControllerRegisterInterface) -> Vec<Codec> {
        let mut codecs: Vec<Codec> = Vec::new();
        for index in 0..MAX_AMOUNT_OF_CODECS {
            // _TODO_: create proper API method in RegisterInterface
            if register_interface.wakests().assert_bit(index) {
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
        register_interface: &ControllerRegisterInterface,
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
        register_interface: &ControllerRegisterInterface,
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

    fn prepare_default_stereo_output(register_interface: &ControllerRegisterInterface, codec: &Codec) {
        let widgets = codec.root_node().function_group_nodes().get(0).unwrap().widgets();
        let line_out_pin_widgets_connected_to_jack = Self::find_line_out_pin_widgets_connected_to_jack(widgets);
        let default_output = *line_out_pin_widgets_connected_to_jack.get(0).unwrap();

        Self::default_stereo_setup(default_output, register_interface);

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

    fn configure_codec(pin_widget: &WidgetNode, connection_list_entry: usize, register_interface: &ControllerRegisterInterface, stream_format: SetStreamFormatPayload, stream_id: u8, channel: u8) {
        // ########## configure codec ##########

        // set gain/mute for pin widget (observation: pin widget owns input and output amp; for both, gain stays at 0, no matter what value gets set, but mute reacts to set commands)
        register_interface.send_command(&SetAmplifierGainMute(pin_widget.address().clone(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Both, SetAmplifierGainMuteSide::Both, 0, false, 100)));

        // activate input and output for pin widget
        let pin_widget_control = PinWidgetControlResponse::try_from(register_interface.send_command(&GetPinWidgetControl(pin_widget.address().clone()))).unwrap();
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

        let connection_list_entries_pin = ConnectionListEntryResponse::try_from(register_interface.send_command(&GetConnectionListEntry(pin_widget.address().clone(), GetConnectionListEntryPayload::new(0)))).unwrap();
        // debug!("connection list entries pin widget: {:?}", connection_list_entries_pin);


        let mixer_widget = if connection_list_entry == 0 {
            NodeAddress::new(0, *connection_list_entries_pin.connection_list_entry_at_offset_index())
        } else {
            NodeAddress::new(0, *connection_list_entries_pin.connection_list_entry_at_offset_index_plus_one())
        };


        // set gain/mute for mixer widget (observation: mixer widget only owns input amp; gain stays at 0, no matter what value gets set, but mute reacts to set commands)
        register_interface.send_command(&SetAmplifierGainMute(mixer_widget.clone(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Input, SetAmplifierGainMuteSide::Both, 0, false, 60)));

        let connection_list_entries_mixer1 = ConnectionListEntryResponse::try_from(register_interface.send_command(&GetConnectionListEntry(mixer_widget.clone(), GetConnectionListEntryPayload::new(0)))).unwrap();
        let audio_out_widget = NodeAddress::new(0, *connection_list_entries_mixer1.connection_list_entry_at_offset_index());

        // set gain/mute for audio output converter widget (observation: audio output converter widget only owns output amp; mute stays false, no matter what value gets set, but gain reacts to set commands)
        // careful: the gain register is only 7 bits long (bits [6:0]), so the max gain value is 127; writing higher numbers into the u8 for gain will overwrite the mute bit at position 7
        // default gain value is 87
        register_interface.send_command(&SetAmplifierGainMute(audio_out_widget.clone(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Both, SetAmplifierGainMuteSide::Both, 0, false, 40)));

        // set stream id
        register_interface.send_command(&SetChannelStreamId(audio_out_widget.clone(), SetChannelStreamIdPayload::new(channel, stream_id)));

        // set stream format
        register_interface.send_command(&SetStreamFormat(audio_out_widget.clone(), stream_format.clone()));
    }

    fn default_stereo_setup(pin_widget: &WidgetNode, register_interface: &ControllerRegisterInterface) {
        // ########## determine appropriate stream parameters ##########
        let stream_format = SetStreamFormatPayload::new(2, BitsPerSample::Sixteen, 1, 1, 48000, StreamType::PCM);

        // default stereo, 48kHz, 24 Bit stream format can be read from audio output converter widget (which gets declared further below)
        // let stream_format = SetStreamFormatPayload::from_response(StreamFormatResponse::try_from(register_interface.send_command(&GetStreamFormat(audio_out_widget.clone()))).unwrap());

        let stream_id = 1;
        let stream = Stream::new(register_interface.output_stream_descriptors().get(0).unwrap(), stream_format.clone(), 2, 2048, stream_id);
        Self::configure_codec(pin_widget, 0, register_interface, stream_format.clone(), stream_id, 0);

        // ########## set up DMA position buffer (not necessary, only for debugging) ##########

        let dmapib_frame_range = alloc_no_cache_dma_memory(1);

        register_interface.set_dma_position_buffer_address(dmapib_frame_range.start);
        register_interface.enable_dma_position_buffer();

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
