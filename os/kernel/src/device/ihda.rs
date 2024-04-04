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
use crate::{apic, interrupt_dispatcher, memory, pci_bus, process_manager};
use crate::device::ihda_node_communication::{AmpCapabilitiesResponse, AudioFunctionGroupCapabilitiesResponse, AudioWidgetCapabilitiesResponse, ConfigDefDefaultDevice, ConfigDefPortConnectivity, ConfigurationDefaultResponse, ConnectionListEntryResponse, ConnectionListLengthResponse, FunctionGroupTypeResponse, GPIOCountResponse, PinCapabilitiesResponse, ProcessingCapabilitiesResponse, RevisionIdResponse, SampleSizeRateCAPsResponse, SupportedStreamFormatsResponse, SubordinateNodeCountResponse, SupportedPowerStatesResponse, VendorIdResponse, WidgetType, StreamFormatResponse, ChannelStreamIdResponse, PinWidgetControlResponse, VoltageReferenceSignalLevel, GetConnectionListEntryPayload, SetAmplifierGainMuteSide, SetAmplifierGainMuteType, SetPinWidgetControlPayload, SetAmplifierGainMutePayload, SetChannelStreamIdPayload, SetStreamFormatPayload};
use crate::device::ihda_node_communication::Command::{GetChannelStreamId, GetConfigurationDefault, GetConnectionListEntry, GetParameter, GetPinWidgetControl, GetStreamFormat, SetAmplifierGainMute, SetChannelStreamId, SetPinWidgetControl};
use crate::device::ihda_node_communication::Parameter::{AudioFunctionGroupCapabilities, AudioWidgetCapabilities, ConnectionListLength, FunctionGroupType, GPIOCount, InputAmpCapabilities, OutputAmpCapabilities, PinCapabilities, ProcessingCapabilities, RevisionId, SampleSizeRateCAPs, SubordinateNodeCount, SupportedPowerStates, SupportedStreamFormats, VendorId};
use crate::device::ihda_types::{Codec, FunctionGroupNode, NodeAddress, RegisterInterface, RootNode, WidgetInfoContainer, WidgetNode, BufferDescriptorList, BufferDescriptorListEntry, AudioBuffer48kHz24BitStereo, SampleContainer};
use crate::device::ihda_types::BitDepth::BitDepth24Bit;
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
        let register_interface = Self::connect_controller();

        info!("Initializing IHDA sound card");
        register_interface.reset_controller();
        info!("IHDA Controller reset complete");

        register_interface.setup_ihda_config_space();
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

    // check the bitmask from bits 0 to 14 of the WAKESTS (in the specification also called STATESTS) indicating available codecs
    // then find all function group nodes and widgets associated with a codec
    fn scan_for_available_codecs(register_interface: &RegisterInterface) -> Vec<Codec> {
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

    fn default_stereo_setup(pin_widget: &WidgetNode, register_interface: &RegisterInterface) {
        // set gain/mute for pin widget (observation: pin widget owns input and output amp; for both, gain stays at 0, no matter what value gets set, but mute reacts to set commands)
        debug!("pin widget: {:?}", pin_widget.address());
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
        // debug!("pin widget control after: {:?}", PinWidgetControlInfo::try_from(register_interface.send_command(&pin_widget.address(), &GetPinWidgetControl)).unwrap());

        let connection_list_entries = ConnectionListEntryResponse::try_from(register_interface.send_command(&GetConnectionListEntry(pin_widget.address().clone(), GetConnectionListEntryPayload::new(0)))).unwrap();
        let mixer_widget = NodeAddress::new(0, *connection_list_entries.connection_list_entry_at_offset_index());

        // set gain/mute for mixer widget (observation: mixer widget only owns input amp; gain stays at 0, no matter what value gets set, but mute reacts to set commands)
        debug!("mixer widget: {:?}", mixer_widget);
        register_interface.send_command(&SetAmplifierGainMute(mixer_widget.clone(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Input, SetAmplifierGainMuteSide::Both, 0, false, 100)));


        let connection_list_entries = ConnectionListEntryResponse::try_from(register_interface.send_command(&GetConnectionListEntry(mixer_widget.clone(), GetConnectionListEntryPayload::new(0)))).unwrap();
        let audio_out_widget = NodeAddress::new(0, *connection_list_entries.connection_list_entry_at_offset_index());

        // set gain/mute for audio output converter widget (observation: audio output converter widget only owns output amp; mute stays false, no matter what value gets set, but gain reacts to set commands)
        // careful: the gain register is only 7 bits long (bits [6:0]), so the max gain value is 127; writing higher numbers into the u8 for gain will overwrite the mute bit at position 7
        // default gain value is 87
        debug!("audio out widget: {:?}", audio_out_widget);
        register_interface.send_command(&SetAmplifierGainMute(audio_out_widget.clone(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Both, SetAmplifierGainMuteSide::Both, 0, false, 127)));

        // set stream id to 1
        debug!("channel stream id before: {:?}", ChannelStreamIdResponse::try_from(register_interface.send_command(&GetChannelStreamId(audio_out_widget.clone()))).unwrap());
        register_interface.send_command(&SetChannelStreamId(audio_out_widget.clone(), SetChannelStreamIdPayload::new(0, 1)));
        debug!("channel stream id after: {:?}", ChannelStreamIdResponse::try_from(register_interface.send_command(&GetChannelStreamId(audio_out_widget.clone()))).unwrap());

        // set stream descriptor
        let sd_registers = register_interface.output_stream_descriptors().get(0).unwrap();

        sd_registers.reset_stream();

        sd_registers.set_stream_number(1);




        let audio_buffer = AudioBuffer48kHz24BitStereo::new(Self::alloc_no_cache_dma_memory(1));

        debug!("audiobuffer_base_address: {:#x}", audio_buffer.start_address());
        unsafe { debug!("audio_buffer first entry: {:#x}", (*audio_buffer.start_address() as *mut u32).read()) }
        unsafe { debug!("audio_buffer second entry: {:#x}", ((*audio_buffer.start_address() + 32) as *mut u32).read()) }
        debug!("sample0: {:#x}", audio_buffer.read_sample_from_buffer(0));
        debug!("sample1: {:#x}", audio_buffer.read_sample_from_buffer(1));
        audio_buffer.write_sample_to_buffer(SampleContainer::from(0b1111_1111_1111_1111_1111_1111, BitDepth24Bit), 1);
        debug!("sample0: {:#x}", audio_buffer.read_sample_from_buffer(0));
        debug!("sample1: {:#x}", audio_buffer.read_sample_from_buffer(1));
        unsafe { debug!("audio_buffer first entry: {:#x}", (*audio_buffer.start_address() as *mut u32).read()) }
        unsafe { debug!("audio_buffer second entry: {:#x}", ((*audio_buffer.start_address() + 32) as *mut u32).read()) }
        unsafe { debug!("audio_buffer first two entry: {:#x}", (*audio_buffer.start_address() as *mut u64).read()) }

        Timer::wait(200000);



        // setup MMIO space for buffer descriptor list
        // hard coded 8*4096 for 256 entries with 128 bits each
        let bdl_frame_range = Self::alloc_no_cache_dma_memory(1);

        debug!("bdl_base_address: {}", bdl_frame_range.start.start_address().as_u64());

        sd_registers.set_bdl_pointer_address(bdl_frame_range.start);

        let bdl = BufferDescriptorList::new(bdl_frame_range);


        debug!("buffer descriptor list: {:?}", bdl);

        for i in 0..255 {
            let frame_range = Self::alloc_no_cache_dma_memory(1);
            let data_buffer = BufferDescriptorListEntry::new(frame_range, false);
            bdl.set_entry(i, &data_buffer);
            for j in 0..(data_buffer.length_in_bytes() / 4) {
                if i%2 == 0 {
                    data_buffer.set_buffer_entry(j, 0b1111_1111_1111_1111_1111_1111_0000_0000);
                } else {
                    data_buffer.set_buffer_entry(j, 0b1111_1111_0000_0000);
                }
            }
        }

        let data_buffer0 = BufferDescriptorListEntry::new(memory::physical::alloc(1), false);
        // let data_buffer1 = BufferDescriptorListEntry::new(memory::physical::alloc(1), false);
        //
        // bdl.set_entry(0, &data_buffer0);
        // bdl.set_entry(1, &data_buffer1);

        debug!("bdl entry 0: {:?}", bdl.get_entry(0));
        debug!("bdl entry 1: {:?}", bdl.get_entry(1));
        debug!("bdl entry 2: {:?}", bdl.get_entry(2));

        // debug!("data_buffer0 address: {:?}", data_buffer0.address());
        // debug!("data_buffer1 address: {:?}", data_buffer1.address());
        // debug!("data_buffer0 address: {:?}", data_buffer0.length_in_bytes());
        // debug!("data_buffer1 address: {:?}", data_buffer1.length_in_bytes());



        // for index in 0..5 {
        //     debug!("data_buffer0 sample at index {}: {}", index, data_buffer0.get_buffer_entry(index));
        //     debug!("data_buffer1 sample at index {}: {}", index, data_buffer1.get_buffer_entry(index));
        // }

        data_buffer0.get_buffer_entry(0);

        // set cyclic buffer length
        sd_registers.set_cyclic_buffer_lenght(*data_buffer0.length_in_bytes() * 256);
        sd_registers.set_last_valid_index(255);

        // set stream format
        let stream_format = StreamFormatResponse::try_from(register_interface.send_command(&GetStreamFormat(audio_out_widget.clone()))).unwrap();
        sd_registers.set_stream_format(SetStreamFormatPayload::from_response(stream_format));

        let dmapib_frame_range = Self::alloc_no_cache_dma_memory(1);

        register_interface.set_dma_position_buffer_address(dmapib_frame_range.start);
        register_interface.enable_dma_position_buffer();

        Timer::wait(2000);

        for i in 0..1 {
            debug!("dma_position_in_buffer of stream descriptor [{}]: {:#x}", i, register_interface.stream_descriptor_position_in_current_buffer(i));
        }
        debug!("dma_position_in_buffer of stream descriptor [0] before run: {:#x}", register_interface.stream_descriptor_position_in_current_buffer(0));

        // debug!("run in one minute seconds!");
        // Timer::wait(60000);
        // run
        sd_registers.set_stream_run_bit();
        // immediately after this run command gets executed on my testing device, I can hear a Dirac impulse over the line out jack
        // this is the expected sound if the two buffers defined above only get played once each: _-
        // instead of looped indefinitely: _-_-_-_-_-_-_-...

        debug!("----------------------------------------------------------------------------------");
        // debug!("sdctl: {:#x}", sd_registers.sdctl().read());
        // debug!("sdsts: {:#x}", sd_registers.sdsts().read());
        // debug!("sdlpib: {:#x}", sd_registers.sdlpib().read());
        // debug!("sdcbl: {:#x}", sd_registers.sdcbl().read());
        // debug!("sdlvi: {:#x}", sd_registers.sdlvi().read());
        // debug!("sdfifod: {:#x}", sd_registers.sdfifod().read());
        // debug!("sdfmt: {:#x}", sd_registers.sdfmt().read());
        // debug!("sdbdpl: {:#x}", sd_registers.sdbdpl().read());
        // debug!("sdbdpu: {:#x}", sd_registers.sdbdpu().read());


        debug!("dma_position_in_buffer of stream descriptor [0] after run: {:#x}", register_interface.stream_descriptor_position_in_current_buffer(0));

        Timer::wait(600000);
    }

    fn alloc_no_cache_dma_memory(frame_count: usize) -> PhysFrameRange {
        let phys_frame_range = memory::physical::alloc(frame_count);

        let kernel_address_space = process_manager().read().kernel_process().unwrap().address_space();
        let start_page = Page::from_start_address(VirtAddr::new(phys_frame_range.start.start_address().as_u64())).unwrap();
        let end_page = Page::from_start_address(VirtAddr::new(phys_frame_range.end.start_address().as_u64())).unwrap();
        let phys_page_range = PageRange { start: start_page, end: end_page };
        kernel_address_space.set_flags(phys_page_range, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE);

        phys_frame_range
    }
}
