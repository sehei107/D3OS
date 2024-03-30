use alloc::vec::Vec;
use core::fmt::LowerHex;
use core::ops::BitAnd;
use log::{debug, info};
use num_traits::int::PrimInt;
use derive_getters::Getters;
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::page::PageRange;
use crate::device::ihda_node_infos::{AmpCapabilitiesResponse, AmplifierGainMuteResponse, AudioFunctionGroupCapabilitiesResponse, AudioWidgetCapabilitiesResponse, BitsPerSample, ChannelStreamIdResponse, ConfigurationDefaultResponse, ConnectionListEntryResponse, ConnectionListLengthResponse, ConnectionSelectResponse, EAPDBTLEnableResponse, FunctionGroupTypeResponse, GetAmplifierGainMuteSide, GetAmplifierGainMuteType, GPIOCountResponse, Response, PinCapabilitiesResponse, PinWidgetControlResponse, ProcessingCapabilitiesResponse, RevisionIdResponse, SampleSizeRateCAPsResponse, SetAmplifierGainMuteSide, SetAmplifierGainMuteType, StreamFormatResponse, StreamType, SubordinateNodeCountResponse, SupportedPowerStatesResponse, SupportedStreamFormatsResponse, VendorIdResponse, VoltageReferenceSignalLevel, VolumeKnobCapabilitiesResponse};
use crate::device::pit::Timer;
use crate::timer;

const MAX_AMOUNT_OF_CODECS: u8 = 15;
const IMMEDIATE_COMMAND_TIMEOUT_IN_MS: usize = 100;
const BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BITS: u8 = 128;
const MAX_AMOUNT_OF_BUFFER_DESCRIPTOR_LIST_ENTRIES: u16 = 256;
const MAX_AMOUNT_OF_AMPLIFIERS_IN_AMP_WIDGET: u8 = 16;
const MAX_AMPLIFIER_GAIN: u8 = u8::MAX;

// representation of an IHDA register
pub struct Register<T: LowerHex + PrimInt> {
    ptr: *mut T,
    name: &'static str,
}

// the LowerHex type bound is only necessary because of the dump function which displays T as a hex value
// the PrimeInt type bound is necessary because of the bit operations | and <<
impl<T: LowerHex + PrimInt> Register<T> {
    pub const fn new(ptr: *mut T, name: &'static str) -> Self {
        Self {
            ptr,
            name,
        }
    }
    pub fn read(&self) -> T {
        unsafe {
            self.ptr.read()
        }
    }
    pub fn write(&self, value: T) {
        unsafe {
            self.ptr.write(value);
        }
    }
    pub fn set_bit(&self, index: u8) {
        let bitmask: u32 = 0x1 << index;
        self.write(self.read() | T::from(bitmask).expect("As only u8, u16 and u32 are used as types for T, this should only fail if index is out of register range"));
    }
    pub fn clear_bit(&self, index: u8) {
        let bitmask: u32 = 0x1 << index;
        self.write(self.read() & !T::from(bitmask).expect("As only u8, u16 and u32 are used as types for T, this should only fail if index is out of register range"));
    }
    pub fn set_all_bits(&self) {
        self.write(!T::from(0).expect("As only u8, u16 and u32 are used as types for T, this should never fail"));
    }
    pub fn clear_all_bits(&self) {
        self.write(T::from(0).expect("As only u8, u16 and u32 are used as types for T, this should never fail"));
    }
    pub fn assert_bit(&self, index: u8) -> bool {
        let bitmask: u32 = 0x1 << index;
        (self.read() & T::from(bitmask).expect("As only u8, u16 and u32 are used as types for T, this should only fail if index is out of register range"))
            != T::from(0).expect("As only u8, u16 and u32 are used as types for T, this should never fail")
    }
    pub fn dump(&self) {
        debug!("Value read from register {}: {:#x}", self.name, self.read());
    }
}

// representation of a register set for each stream descriptor (starting at offset 0x80)
#[derive(Getters)]
pub struct StreamDescriptorRegisters {
    // careful: the sdctl register is only 3 bytes long, so that reading the register as an u32 also reads the sdsts register in the last byte
    // the last byte of the read value should therefore not be manipulated
    sdctl: Register<u32>,
    sdsts: Register<u8>,
    sdlpib: Register<u32>,
    sdcbl: Register<u32>,
    sdlvi: Register<u16>,
    sdfifod: Register<u16>,
    sdfmt: Register<u16>,
    sdbdpl: Register<u32>,
    sdbdpu: Register<u32>,
}

impl StreamDescriptorRegisters {
    pub fn new(sd_base_address: u64) -> Self {
        Self {
            sdctl: Register::new(sd_base_address as *mut u32, "SDCTL"),
            sdsts: Register::new((sd_base_address + 0x3) as *mut u8, "SDSTS"),
            sdlpib: Register::new((sd_base_address + 0x4) as *mut u32, "SDLPIB"),
            sdcbl: Register::new((sd_base_address + 0x8) as *mut u32, "SDCBL"),
            sdlvi: Register::new((sd_base_address + 0xC) as *mut u16, "SDLVI"),
            // bytes with offset 0x8E to 0x8F are reserved
            sdfifod: Register::new((sd_base_address + 0x10) as *mut u16, "SDFIFOD"),
            sdfmt: Register::new((sd_base_address + 0x12) as *mut u16, "SDFMT"),
            // bytes with offset 0x94 to 0x97 are reserved
            sdbdpl: Register::new((sd_base_address + 0x18) as *mut u32, "SDDPL"),
            sdbdpu: Register::new((sd_base_address + 0x1C) as *mut u32, "SDDPU"),
        }
    }
}

// representation of all IHDA registers
#[derive(Getters)]
pub struct ControllerRegisterSet {
    gcap: Register<u16>,
    vmin: Register<u8>,
    vmaj: Register<u8>,
    outpay: Register<u16>,
    inpay: Register<u16>,
    gctl: Register<u32>,
    wakeen: Register<u16>,
    wakests: Register<u16>,
    gsts: Register<u16>,
    outstrmpay: Register<u16>,
    instrmpay: Register<u16>,
    intctl: Register<u32>,
    intsts: Register<u32>,
    walclk: Register<u32>,
    ssync: Register<u32>,
    corblbase: Register<u32>,
    corbubase: Register<u32>,
    corbwp: Register<u16>,
    corbrp: Register<u16>,
    corbctl: Register<u8>,
    corbsts: Register<u8>,
    corbsize: Register<u8>,
    rirblbase: Register<u32>,
    rirbubase: Register<u32>,
    rirbwp: Register<u16>,
    rintcnt: Register<u16>,
    rirbctl: Register<u8>,
    rirbsts: Register<u8>,
    rirbsize: Register<u8>,
    icoi: Register<u32>,
    icii: Register<u32>,
    icis: Register<u16>,
    dpiblbase: Register<u32>,
    dpibubase: Register<u32>,

    input_stream_descriptors: Vec<StreamDescriptorRegisters>,
    output_stream_descriptors: Vec<StreamDescriptorRegisters>,
    bidirectional_stream_descriptors: Vec<StreamDescriptorRegisters>,

    // the aliases at high adresses are used to pass information to user level applications instead of the actual registers,
    // so that more sensible registers don't get accidentally passed, because they are on the same kernel page
    walclk_alias: Register<u32>,
    // sdlpiba_aliases: Vec<Register<u32>>,
}

impl ControllerRegisterSet {
    pub fn new(mmio_base_address: u64) -> Self {
        const SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES: u64 = 0x20;
        const OFFSET_OF_FIRST_SOUND_DESCRIPTOR: u64 = 0x80;
        // the following read addresses the Global Capacities (GCAP) register, which contains information on the amount of
        // input, output and bidirectional stream descriptors of a specific IHDA sound card (see section 3.3.2 of the specification)
        let gctl = unsafe { (mmio_base_address as *mut u16).read() as u64 };
        let input_stream_descriptor_amount = (gctl & 0x0F00) >> 8;
        let output_stream_descriptor_amount = (gctl & 0xF000) >> 12;
        let bidirectional_stream_descriptor_amount = (gctl & 0b0000_0000_1111_1000) >> 3;

        let mut input_stream_descriptors = Vec::new();
        for index in 0..input_stream_descriptor_amount {
            input_stream_descriptors.push(StreamDescriptorRegisters::new(
                mmio_base_address
                    + OFFSET_OF_FIRST_SOUND_DESCRIPTOR
                    + (SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES * index)
            ));
        }

        let mut output_stream_descriptors = Vec::new();
        for index in 0..output_stream_descriptor_amount {
            output_stream_descriptors.push(StreamDescriptorRegisters::new(
                mmio_base_address
                    + OFFSET_OF_FIRST_SOUND_DESCRIPTOR
                    + (SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES * (input_stream_descriptor_amount + index))
            ));
        }

        let mut bidirectional_stream_descriptors = Vec::new();
        for index in 0..bidirectional_stream_descriptor_amount {
            bidirectional_stream_descriptors.push(StreamDescriptorRegisters::new(
                mmio_base_address
                    + OFFSET_OF_FIRST_SOUND_DESCRIPTOR
                    + (SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES * (input_stream_descriptor_amount + output_stream_descriptor_amount + index))
            ));
        }

        Self {
            gcap: Register::new(mmio_base_address as *mut u16, "GCAP"),
            vmin: Register::new((mmio_base_address + 0x2) as *mut u8, "VMIN"),
            vmaj: Register::new((mmio_base_address + 0x3) as *mut u8, "VMAJ"),
            outpay: Register::new((mmio_base_address + 0x4) as *mut u16, "OUTPAY"),
            inpay: Register::new((mmio_base_address + 0x6) as *mut u16, "INPAY"),
            gctl: Register::new((mmio_base_address + 0x8) as *mut u32, "GCTL"),
            wakeen: Register::new((mmio_base_address + 0xC) as *mut u16, "WAKEEN"),
            wakests: Register::new((mmio_base_address + 0xE) as *mut u16, "WAKESTS"),
            gsts: Register::new((mmio_base_address + 0x10) as *mut u16, "GSTS"),
            // bytes with offset 0x12 to 0x17 are reserved
            outstrmpay: Register::new((mmio_base_address + 0x18) as *mut u16, "OUTSTRMPAY"),
            instrmpay: Register::new((mmio_base_address + 0x1A) as *mut u16, "INSTRMPAY"),
            // bytes with offset 0x1C to 0x1F are reserved
            intctl: Register::new((mmio_base_address + 0x20) as *mut u32, "INTCTL"),
            intsts: Register::new((mmio_base_address + 0x24) as *mut u32, "INTSTS"),
            // bytes with offset 0x28 to 0x2F are reserved
            walclk: Register::new((mmio_base_address + 0x30) as *mut u32, "WALCLK"),
            // bytes with offset 0x34 to 0x37 are reserved
            ssync: Register::new((mmio_base_address + 0x38) as *mut u32, "SSYNC"),
            // bytes with offset 0x3C to 0x3F are reserved
            corblbase: Register::new((mmio_base_address + 0x40) as *mut u32, "CORBLBASE"),
            corbubase: Register::new((mmio_base_address + 0x44) as *mut u32, "CORBUBASE"),
            corbwp: Register::new((mmio_base_address + 0x48) as *mut u16, "CORBWP"),
            corbrp: Register::new((mmio_base_address + 0x4A) as *mut u16, "CORBRP"),
            corbctl: Register::new((mmio_base_address + 0x4C) as *mut u8, "CORBCTL"),
            corbsts: Register::new((mmio_base_address + 0x4D) as *mut u8, "CORBSTS"),
            corbsize: Register::new((mmio_base_address + 0x4E) as *mut u8, "CORBSIZE"),
            // byte with offset 0x4F is reserved
            rirblbase: Register::new((mmio_base_address + 0x50) as *mut u32, "RIRBLBASE"),
            rirbubase: Register::new((mmio_base_address + 0x54) as *mut u32, "RIRBUBASE"),
            rirbwp: Register::new((mmio_base_address + 0x58) as *mut u16, "RIRBWP"),
            rintcnt: Register::new((mmio_base_address + 0x5A) as *mut u16, "RINTCNT"),
            rirbctl: Register::new((mmio_base_address + 0x5C) as *mut u8, "RIRBCTL"),
            rirbsts: Register::new((mmio_base_address + 0x5D) as *mut u8, "RIRBSTS"),
            rirbsize: Register::new((mmio_base_address + 0x5E) as *mut u8, "RIRBSIZE"),
            // byte with offset 0x5F is reserved
            // the following three immediate command registers from bytes 0x60 to 0x69 are optional
            icoi: Register::new((mmio_base_address + 0x60) as *mut u32, "ICOI"),
            icii: Register::new((mmio_base_address + 0x64) as *mut u32, "ICII"),
            icis: Register::new((mmio_base_address + 0x68) as *mut u16, "ICIS"),
            // bytes with offset 0x6A to 0x6F are reserved
            dpiblbase: Register::new((mmio_base_address + 0x70) as *mut u32, "DPIBLBASE"),
            dpibubase: Register::new((mmio_base_address + 0x74) as *mut u32, "DPIBUBASE"),
            // bytes with offset 0x78 to 0x7F are reserved

            input_stream_descriptors,
            output_stream_descriptors,
            bidirectional_stream_descriptors,

            walclk_alias: Register::new((mmio_base_address + 0x2030) as *mut u32, "WALCLKA"),
            // sdlpiba_aliases: Vec<Register<u32>>,
        }
    }

    fn immediate_command(&self, command: u32) -> u32 {
        self.icis().write(0b10);
        self.icoi().write(command);
        self.icis().write(0b1);
        let start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        while (self.icis().read() & 0b10) != 0b10 {
            if timer().read().systime_ms() > start_timer + IMMEDIATE_COMMAND_TIMEOUT_IN_MS {
                panic!("IHDA immediate command timed out")
            }
        }
        self.icii().read()
    }
}

#[derive(Getters)]
pub struct RegisterInterface {
    crs: ControllerRegisterSet,
}

impl RegisterInterface {
    pub fn new(mmio_base_address: u64) -> Self {
        RegisterInterface {
            crs: ControllerRegisterSet::new(mmio_base_address),
        }
    }

    pub fn send_command(&self, addr: &NodeAddress, command: &Command) -> Response {
        let parsed_command = CommandBuilder::build_command(&addr, command);
        let response = self.crs.immediate_command(parsed_command);
        ResponseParser::parse_response(response, command)
    }
}

#[derive(Debug, Getters)]
pub struct NodeAddress {
    codec_address: u8,
    node_id: u8,
}

impl NodeAddress {
    pub fn new(codec_address: u8, node_id: u8) -> Self {
        if codec_address >= MAX_AMOUNT_OF_CODECS { panic!("IHDA only supports up to {} codecs!", MAX_AMOUNT_OF_CODECS) };
        NodeAddress {
            codec_address,
            node_id,
        }
    }
}

#[derive(Debug, Getters)]
pub struct Codec {
    codec_address: u8,
    root_node: RootNode,
}

impl Codec {
    pub fn new(codec_address: u8, root_node: RootNode) -> Self {
        Codec {
            codec_address,
            root_node,
        }
    }
}

pub trait Node {
    fn address(&self) -> &NodeAddress;
}

#[derive(Debug, Getters)]
pub struct RootNode {
    address: NodeAddress,
    vendor_id: VendorIdResponse,
    revision_id: RevisionIdResponse,
    subordinate_node_count: SubordinateNodeCountResponse,
    function_group_nodes: Vec<FunctionGroupNode>,
}

impl Node for RootNode {
    fn address(&self) -> &NodeAddress {
        &self.address
    }
}

impl RootNode {
    pub fn new(
        codec_address: u8,
        vendor_id: VendorIdResponse,
        revision_id: RevisionIdResponse,
        subordinate_node_count: SubordinateNodeCountResponse,
        function_group_nodes: Vec<FunctionGroupNode>
    ) -> Self {
        RootNode {
            address: NodeAddress::new(codec_address, 0),
            vendor_id,
            revision_id,
            subordinate_node_count,
            function_group_nodes,
        }
    }
}

#[derive(Debug, Getters)]
pub struct FunctionGroupNode {
    address: NodeAddress,
    subordinate_node_count: SubordinateNodeCountResponse,
    function_group_type: FunctionGroupTypeResponse,
    audio_function_group_caps: AudioFunctionGroupCapabilitiesResponse,
    sample_size_rate_caps: SampleSizeRateCAPsResponse,
    supported_stream_formats: SupportedStreamFormatsResponse,
    input_amp_caps: AmpCapabilitiesResponse,
    output_amp_caps: AmpCapabilitiesResponse,
    // function group node must provide a SupportedPowerStatesInfo, but QEMU doesn't do it... so this only an Option<SupportedPowerStatesInfo> for now
    supported_power_states: SupportedPowerStatesResponse,
    gpio_count: GPIOCountResponse,
    widgets: Vec<WidgetNode>,
}

impl Node for FunctionGroupNode {
    fn address(&self) -> &NodeAddress {
        &self.address
    }
}

impl FunctionGroupNode {
    pub fn new(
        address: NodeAddress,
        subordinate_node_count: SubordinateNodeCountResponse,
        function_group_type: FunctionGroupTypeResponse,
        audio_function_group_caps: AudioFunctionGroupCapabilitiesResponse,
        sample_size_rate_caps: SampleSizeRateCAPsResponse,
        supported_stream_formats: SupportedStreamFormatsResponse,
        input_amp_caps: AmpCapabilitiesResponse,
        output_amp_caps: AmpCapabilitiesResponse,
        supported_power_states: SupportedPowerStatesResponse,
        gpio_count: GPIOCountResponse,
        widgets: Vec<WidgetNode>
    ) -> Self {
        FunctionGroupNode {
            address,
            subordinate_node_count,
            function_group_type,
            audio_function_group_caps,
            sample_size_rate_caps,
            supported_stream_formats,
            input_amp_caps,
            output_amp_caps,
            supported_power_states,
            gpio_count,
            widgets
        }
    }
}

#[derive(Debug, Getters)]
pub struct WidgetNode {
    address: NodeAddress,
    audio_widget_capabilities: AudioWidgetCapabilitiesResponse,
    widget_info: WidgetInfoContainer,
}

impl Node for WidgetNode {
    fn address(&self) -> &NodeAddress {
        &self.address
    }
}

impl WidgetNode {
    pub fn new(address: NodeAddress, audio_widget_capabilities: AudioWidgetCapabilitiesResponse, widget_info: WidgetInfoContainer) -> Self {
        WidgetNode {
            address,
            audio_widget_capabilities,
            widget_info
        }
    }

    pub fn max_number_of_channels(&self) -> u8 {
        // this formula can be found in section 7.3.4.6, Audio Widget Capabilities of the specification
        (self.audio_widget_capabilities.chan_count_ext() << 1) + (*self.audio_widget_capabilities.chan_count_lsb() as u8) + 1
    }
}

#[derive(Debug)]
pub enum WidgetInfoContainer {
    AudioOutputConverter(
        SampleSizeRateCAPsResponse,
        SupportedStreamFormatsResponse,
        AmpCapabilitiesResponse,
        SupportedPowerStatesResponse,
        ProcessingCapabilitiesResponse,
    ),
    AudioInputConverter(
        SampleSizeRateCAPsResponse,
        SupportedStreamFormatsResponse,
        AmpCapabilitiesResponse,
        ConnectionListLengthResponse,
        SupportedPowerStatesResponse,
        ProcessingCapabilitiesResponse,
    ),
    // first AmpCapabilitiesInfo is input amp caps and second AmpCapabilitiesInfo is output amp caps
    PinComplex(
        PinCapabilitiesResponse,
        AmpCapabilitiesResponse,
        AmpCapabilitiesResponse,
        ConnectionListLengthResponse,
        SupportedPowerStatesResponse,
        ProcessingCapabilitiesResponse,
        ConfigurationDefaultResponse,
        ConnectionListEntryResponse,
    ),
    Mixer,
    Selector,
    Power,
    VolumeKnob,
    BeepGenerator,
    VendorDefined,
}

pub struct CommandBuilder;

impl CommandBuilder {
    pub fn build_command(node_address: &NodeAddress, command: &Command) -> u32 {
        match command {
            Command::GetParameter(parameter) => Self::command_with_12bit_identifier_verb(node_address, command.id(), parameter.id()),
            Command::GetConnectionSelect => Self::command_with_12bit_identifier_verb(node_address, command.id(), 0x0),
            Command::SetConnectionSelect(payload) => Self::command_with_12bit_identifier_verb(node_address, command.id(), *payload.connection_index()),
            Command::GetConnectionListEntry(payload) => Self::command_with_12bit_identifier_verb(node_address, command.id(), *payload.offset()),
            Command::GetAmplifierGainMute(payload)
                => Self::command_with_4bit_identifier_verb(node_address, command.id(), Self::parse_get_amplifier_gain_mute(payload.amp_type(), payload.side(), *payload.index())),
            Command::SetAmplifierGainMute(payload)
                => Self::command_with_4bit_identifier_verb(node_address, command.id(), Self::parse_set_amplifier_gain_mute(payload.amp_type(), payload.side(), *payload.index(), *payload.mute(), *payload.gain())),
            Command::GetStreamFormat => Self::command_with_4bit_identifier_verb(node_address, command.id(), 0x0),
            Command::SetStreamFormat(payload) => Self::command_with_4bit_identifier_verb(node_address, command.id(), payload.as_u16()),
            Command::GetChannelStreamId => Self::command_with_12bit_identifier_verb(node_address, command.id(), 0x0),
            Command::SetChannelStreamId(payload) => Self::command_with_12bit_identifier_verb(node_address, command.id(), payload.as_u8()),
            Command::GetPinWidgetControl => Self::command_with_12bit_identifier_verb(node_address, command.id(), 0x0),
            Command::SetPinWidgetControl(payload) => Self::command_with_12bit_identifier_verb(node_address, command.id(), payload.as_u8()),
            Command::GetEAPDBTLEnable => Self::command_with_12bit_identifier_verb(node_address, command.id(), 0x0),
            Command::SetEAPDBTLEnable(payload) => Self::command_with_12bit_identifier_verb(node_address, command.id(), payload.as_u8()),
            Command::GetConfigurationDefault => Self::command_with_12bit_identifier_verb(node_address, command.id(), 0x0),
        }
    }

    fn command_with_12bit_identifier_verb(node_address: &NodeAddress, verb_id: u16, payload: u8) -> u32 {
        (node_address.codec_address as u32) << 28
            | (node_address.node_id as u32) << 20
            | (verb_id as u32) << 8
            | payload as u32
    }

    fn command_with_4bit_identifier_verb(node_address: &NodeAddress, verb_id: u16, payload: u16) -> u32 {
        (node_address.codec_address as u32) << 28
            | (node_address.node_id as u32) << 20
            | (verb_id as u32) << 16
            | payload as u32
    }

    fn parse_get_amplifier_gain_mute(amp_type: &GetAmplifierGainMuteType, side: &GetAmplifierGainMuteSide, index: u8) -> u16 {
        let amp_type: u16 = match amp_type  {
            GetAmplifierGainMuteType::Input => 0,
            GetAmplifierGainMuteType::Output => 1,
        };
        let side: u16 = match side  {
            GetAmplifierGainMuteSide::Right => 0,
            GetAmplifierGainMuteSide::Left => 1,
        };
        if index > MAX_AMOUNT_OF_AMPLIFIERS_IN_AMP_WIDGET { panic!("Index for amplifier out of range") }
        debug!("get_amplifier: {:#x}", amp_type << 15 | side << 13 | index as u16);

        amp_type << 15 | side << 13 | index as u16
    }

    fn parse_set_amplifier_gain_mute(amp_type: &SetAmplifierGainMuteType, side: &SetAmplifierGainMuteSide, index: u8, mute: bool, gain: u8) -> u16 {
        let amp_type: u16 = match amp_type  {
            SetAmplifierGainMuteType::Input => 0b01,
            SetAmplifierGainMuteType::Output => 0b10,
            SetAmplifierGainMuteType::Both => 0b11,
        };
        let side: u16 = match side  {
            SetAmplifierGainMuteSide::Right => 0b01,
            SetAmplifierGainMuteSide::Left => 0b10,
            SetAmplifierGainMuteSide::Both => 0b11,
        };
        if index > MAX_AMOUNT_OF_AMPLIFIERS_IN_AMP_WIDGET { panic!("Index for amplifier out of range") }
        if gain > MAX_AMPLIFIER_GAIN { panic!("Trying to set amplifier gain higher than max value") }
        debug!("set_amplifier: {:#x}", amp_type << 14 | side << 12 | (index as u16) << 8 | (mute as u16) << 7 | gain as u16);

        amp_type << 14 | side << 12 | (index as u16) << 8 | (mute as u16) << 7 | gain as u16
    }
}

#[derive(Debug)]
pub enum Command {
    GetParameter(Parameter),
    GetConnectionSelect,
    SetConnectionSelect(SetConnectionSelectPayload),
    GetConnectionListEntry(GetConnectionListEntryPayload),
    GetAmplifierGainMute(GetAmplifierGainMutePayload),
    SetAmplifierGainMute(SetAmplifierGainMutePayload),
    GetStreamFormat,
    SetStreamFormat(SetStreamFormatPayload),
    GetChannelStreamId,
    SetChannelStreamId(SetChannelStreamIdPayload),
    GetPinWidgetControl,
    SetPinWidgetControl(SetPinWidgetControlPayload),
    GetEAPDBTLEnable,
    SetEAPDBTLEnable(SetEAPDBTLEnablePayload),
    GetConfigurationDefault,
}

impl Command {
    pub fn id(&self) -> u16 {
        match self {
            Command::GetParameter(_) => 0xF00,
            Command::GetConnectionSelect => 0xF01,
            Command::SetConnectionSelect { .. } => 0x701,
            Command::GetConnectionListEntry { .. } => 0xF02,
            Command::GetAmplifierGainMute { .. } => 0xB,
            Command::SetAmplifierGainMute { .. } => 0x3,
            Command::GetStreamFormat => 0xA,
            Command::SetStreamFormat(_) => 0x2,
            Command::GetChannelStreamId => 0xF06,
            Command::SetChannelStreamId(_) => 0x706,
            Command::GetPinWidgetControl => 0xF07,
            Command::SetPinWidgetControl(_) => 0x707,
            Command::GetEAPDBTLEnable => 0xF0C,
            Command::SetEAPDBTLEnable(_) => 0x70C,
            Command::GetConfigurationDefault => 0xF1C,
        }
    }
}

// compare to table 140 in section 7.3.6 of the specification
#[derive(Debug)]
pub enum Parameter {
    VendorId,
    RevisionId,
    SubordinateNodeCount,
    FunctionGroupType,
    AudioFunctionGroupCapabilities,
    AudioWidgetCapabilities,
    SampleSizeRateCAPs,
    SupportedStreamFormats,
    PinCapabilities,
    InputAmpCapabilities,
    OutputAmpCapabilities,
    ConnectionListLength,
    SupportedPowerStates,
    ProcessingCapabilities,
    GPIOCount,
    VolumeKnobCapabilities,
}

impl Parameter {
    pub fn id(&self) -> u8 {
        match self {
            Parameter::VendorId => 0x00,
            Parameter::RevisionId => 0x02,
            Parameter::SubordinateNodeCount => 0x04,
            Parameter::FunctionGroupType => 0x05,
            Parameter::AudioFunctionGroupCapabilities => 0x08,
            Parameter::AudioWidgetCapabilities => 0x09,
            Parameter::SampleSizeRateCAPs => 0x0A,
            Parameter::SupportedStreamFormats => 0x0B,
            Parameter::PinCapabilities => 0x0C,
            Parameter::InputAmpCapabilities => 0x0D,
            Parameter::OutputAmpCapabilities => 0x12,
            Parameter::ConnectionListLength => 0x0E,
            Parameter::SupportedPowerStates => 0x0F,
            Parameter::ProcessingCapabilities => 0x10,
            Parameter::GPIOCount => 0x11,
            Parameter::VolumeKnobCapabilities => 0x13,
        }
    }
}

#[derive(Debug, Getters)]
pub struct SetConnectionSelectPayload {
    connection_index: u8,
}

impl SetConnectionSelectPayload {
    pub fn new(connection_index: u8) -> Self {
        Self {
            connection_index,
        }
    }
}

#[derive(Debug, Getters)]
pub struct GetConnectionListEntryPayload {
    offset: u8,
}

impl GetConnectionListEntryPayload {
    pub fn new(offset: u8) -> Self {
        Self {
            offset,
        }
    }
}

#[derive(Debug, Getters)]
pub struct GetAmplifierGainMutePayload {
    amp_type: GetAmplifierGainMuteType,
    side: GetAmplifierGainMuteSide,
    index: u8,
}

impl GetAmplifierGainMutePayload {
    pub fn new(amp_type: GetAmplifierGainMuteType, side: GetAmplifierGainMuteSide, index: u8) -> Self {
        Self {
            amp_type,
            side,
            index,
        }
    }
}

#[derive(Debug, Getters)]
pub struct SetAmplifierGainMutePayload {
    amp_type: SetAmplifierGainMuteType,
    side: SetAmplifierGainMuteSide,
    index: u8,
    mute: bool,
    gain: u8,
}

impl SetAmplifierGainMutePayload {
    pub fn new(amp_type: SetAmplifierGainMuteType, side: SetAmplifierGainMuteSide, index: u8, mute: bool, gain: u8) -> Self {
        if gain > 127 { panic!("gain is a 7 bit parameter, writing 8 bit values will leak into mute bit and are therefore prohibited") }
        Self {
            amp_type,
            side,
            index,
            mute: false,
            gain: 0,
        }
    }
}

#[derive(Debug, Getters)]
pub struct SetStreamFormatPayload {
    number_of_channels: u8,
    bits_per_sample: BitsPerSample,
    sample_base_rate_divisor: u8,
    sample_base_rate_multiple: u8,
    sample_base_rate: u16,
    stream_type: StreamType,
}

impl SetStreamFormatPayload {
    pub fn new(
        number_of_channels: u8,
        bits_per_sample: BitsPerSample,
        sample_base_rate_divisor: u8,
        sample_base_rate_multiple: u8,
        sample_base_rate: u16,
        stream_type: StreamType,
    ) -> Self {
        Self {
            number_of_channels,
            bits_per_sample,
            sample_base_rate_divisor,
            sample_base_rate_multiple,
            sample_base_rate,
            stream_type,
        }
    }

    pub fn as_u16(&self) -> u16 {
        let number_of_channels = self.number_of_channels - 1;
        let bits_per_sample = match self.bits_per_sample {
            BitsPerSample::Eight => 0b000,
            BitsPerSample::Sixteen => 0b001,
            BitsPerSample::Twenty => 0b010,
            BitsPerSample::Twentyfour => 0b011,
            BitsPerSample::Thirtytwo => 0b100,
            _ => panic!("This arm should be unreachable as the only constructor of StreamFormatInfo doesn't let you create an instance with invalid values for bit depth")
        };
        let sample_base_rate_divisor = self.sample_base_rate_divisor - 1;
        let sample_base_rate_multiple = self.sample_base_rate_multiple - 1;
        let sample_base_rate = if self.sample_base_rate == 44100 { 1 } else { 0 };
        let stream_type = match self.stream_type {
            StreamType::PCM => 0,
            StreamType::NonPCM => 1,
        };
        (stream_type as u16) << 15
            | (sample_base_rate as u16) << 14
            | (sample_base_rate_multiple as u16) << 11
            | (sample_base_rate_divisor as u16) << 8
            | (bits_per_sample as u16) << 4
            | number_of_channels as u16
    }

    pub fn from_response(stream_format: StreamFormatResponse) -> Self {
        Self {
            number_of_channels: *stream_format.number_of_channels(),
            bits_per_sample: match stream_format.bits_per_sample() {
                BitsPerSample::Eight => BitsPerSample::Eight,
                BitsPerSample::Sixteen => BitsPerSample::Sixteen,
                BitsPerSample::Twenty => BitsPerSample::Twenty,
                BitsPerSample::Twentyfour => BitsPerSample::Twentyfour,
                BitsPerSample::Thirtytwo => BitsPerSample::Thirtytwo,
            },
            sample_base_rate_divisor: *stream_format.sample_base_rate_divisor(),
            sample_base_rate_multiple: *stream_format.sample_base_rate_multiple(),
            sample_base_rate: *stream_format.sample_base_rate(),
            stream_type: match stream_format.stream_type() {
                StreamType::PCM => StreamType::PCM,
                StreamType::NonPCM => StreamType::NonPCM,
            },
        }
    }
}

#[derive(Debug, Getters)]
pub struct SetChannelStreamIdPayload {
    channel: u8,
    stream: u8,
}

impl SetChannelStreamIdPayload {
    pub fn new(channel: u8, stream: u8,) -> Self {
        Self {
            channel,
            stream,
        }
    }

    pub fn as_u8(&self) -> u8 {
        (self.stream << 4) | self.channel
    }
}

#[derive(Debug, Getters)]
pub struct SetPinWidgetControlPayload {
    voltage_reference_enable: VoltageReferenceSignalLevel,
    in_enable: bool,
    out_enable: bool,
    h_phn_enable: bool,
}

impl SetPinWidgetControlPayload {
    pub fn new(
        voltage_reference_enable: VoltageReferenceSignalLevel,
        in_enable: bool,
        out_enable: bool,
        h_phn_enable: bool,
    ) -> Self {
        Self {
            voltage_reference_enable,
            in_enable,
            out_enable,
            h_phn_enable,
        }
    }

    pub fn as_u8(&self) -> u8 {
        let voltage_reference_enable = match self.voltage_reference_enable {
            VoltageReferenceSignalLevel::HiZ => 0b000,
            VoltageReferenceSignalLevel::FiftyPercent => 0b001,
            VoltageReferenceSignalLevel::Ground0V => 0b010,
            VoltageReferenceSignalLevel::EightyPercent => 0b100,
            VoltageReferenceSignalLevel::HundredPercent => 0b101,
        };
        (self.h_phn_enable as u8) << 7 | (self.out_enable as u8) << 6 | (self.in_enable as u8) << 5 | voltage_reference_enable
    }
}

#[derive(Debug, Getters)]
pub struct SetEAPDBTLEnablePayload {
    btl_enable: bool,
    eapd_enable: bool,
    lr_swap: bool,
}

impl SetEAPDBTLEnablePayload {
    pub fn new(
        btl_enable: bool,
        eapd_enable: bool,
        lr_swap: bool,
    ) -> Self {
        Self {
            btl_enable,
            eapd_enable,
            lr_swap,
        }
    }

    pub fn as_u8(&self) -> u8 {
        (self.btl_enable as u8) << 2 | (self.eapd_enable as u8) << 1 | self.lr_swap as u8
    }
}

pub struct ResponseParser;

impl ResponseParser {
    pub fn parse_response(response: u32, command: &Command) -> Response {
        match command {
            Command::GetParameter(parameter) => {
                match parameter {
                    Parameter::VendorId => Response::VendorId(VendorIdResponse::new(response)),
                    Parameter::RevisionId => Response::RevisionId(RevisionIdResponse::new(response)),
                    Parameter::SubordinateNodeCount => Response::SubordinateNodeCount(SubordinateNodeCountResponse::new(response)),
                    Parameter::FunctionGroupType => Response::FunctionGroupType(FunctionGroupTypeResponse::new(response)),
                    Parameter::AudioFunctionGroupCapabilities => Response::AudioFunctionGroupCapabilities(AudioFunctionGroupCapabilitiesResponse::new(response)),
                    Parameter::AudioWidgetCapabilities => Response::AudioWidgetCapabilities(AudioWidgetCapabilitiesResponse::new(response)),
                    Parameter::SampleSizeRateCAPs => Response::SampleSizeRateCAPs(SampleSizeRateCAPsResponse::new(response)),
                    Parameter::SupportedStreamFormats => Response::SupportedStreamFormats(SupportedStreamFormatsResponse::new(response)),
                    Parameter::PinCapabilities => Response::PinCapabilities(PinCapabilitiesResponse::new(response)),
                    Parameter::InputAmpCapabilities => Response::InputAmpCapabilities(AmpCapabilitiesResponse::new(response)),
                    Parameter::OutputAmpCapabilities => Response::OutputAmpCapabilities(AmpCapabilitiesResponse::new(response)),
                    Parameter::ConnectionListLength => Response::ConnectionListLength(ConnectionListLengthResponse::new(response)),
                    Parameter::SupportedPowerStates => Response::SupportedPowerStates(SupportedPowerStatesResponse::new(response)),
                    Parameter::ProcessingCapabilities => Response::ProcessingCapabilities(ProcessingCapabilitiesResponse::new(response)),
                    Parameter::GPIOCount => Response::GPIOCount(GPIOCountResponse::new(response)),
                    Parameter::VolumeKnobCapabilities => Response::VolumeKnobCapabilities(VolumeKnobCapabilitiesResponse::new(response)),
                }
            }
            Command::GetConnectionSelect => Response::ConnectionSelect(ConnectionSelectResponse::new(response)),
            Command::SetConnectionSelect { .. } => Response::SetInfo,
            Command::GetConnectionListEntry { .. } => Response::ConnectionListEntry(ConnectionListEntryResponse::new(response)),
            Command::GetAmplifierGainMute { .. } => Response::AmplifierGainMute(AmplifierGainMuteResponse::new(response)),
            Command::SetAmplifierGainMute { .. } => Response::SetInfo,
            Command::GetStreamFormat { .. } => Response::StreamFormat(StreamFormatResponse::new(response)),
            Command::SetStreamFormat { .. } => Response::SetInfo,
            Command::GetChannelStreamId => Response::ChannelStreamId(ChannelStreamIdResponse::new(response)),
            Command::SetChannelStreamId(_) => Response::SetInfo,
            Command::GetPinWidgetControl => Response::PinWidgetControl(PinWidgetControlResponse::new(response)),
            Command::SetPinWidgetControl { .. } => Response::SetInfo,
            Command::GetEAPDBTLEnable => Response::EAPDBTLEnable(EAPDBTLEnableResponse::new(response)),
            Command::SetEAPDBTLEnable(_) => Response::SetInfo,
            Command::GetConfigurationDefault => Response::ConfigurationDefault(ConfigurationDefaultResponse::new(response)),
        }
    }
}

#[derive(Debug, Getters)]
pub struct BufferDescriptorListEntry {
    address: u64,
    length_in_bytes: u32,
    interrupt_on_completion: bool,
}

impl BufferDescriptorListEntry {
    pub fn new(frame_range: PhysFrameRange, interrupt_on_completion: bool) -> Self {
        let address;
        let length_in_bytes;
        match frame_range {
            PhysFrameRange { start, end } => {
                address = start.start_address().as_u64();
                length_in_bytes = ((end.start_address().as_u64() - address) / 8 ) as u32;
            }
        }
        Self {
            address,
            length_in_bytes,
            interrupt_on_completion,
        }
    }

    pub fn from(raw_data: u128) -> Self {
        Self {
            address: (raw_data & 0xFFFF_FFFF_FFFF_FFFF) as u64,
            length_in_bytes: ((raw_data >> 64) & 0xFFFF_FFFF) as u32,
            // probably better use get_bit() function from ihda_node_infos.rs, after moving it to a better place
            // or even better: use a proper library for all the bit operations on unsigned integers
            interrupt_on_completion: ((raw_data >> 96) & 1) == 1,
        }
    }

    pub fn as_u128(&self) -> u128 {
        (self.interrupt_on_completion as u128) << 96 | (self.length_in_bytes as u128) << 64 | self.address as u128
    }

    pub fn get_buffer_entry(&self, index: u32) -> u32 {
        unsafe { ((self.address + (index as u64 * 32u64)) as *mut u32).read() }
    }

    pub fn set_buffer_entry(&self, index: u32, entry: u32) {
        unsafe { ((self.address + (index as u64 * 32u64)) as *mut u32).write(entry) };

    }
}

#[derive(Debug, Getters)]
pub struct BufferDescriptorList {
    base_address: u64,
    max_amount_of_entries: u16
}

impl BufferDescriptorList {
    pub fn new(bdl_frame_range: PhysFrameRange) -> Self {
        let (bdl_base_address, max_amount_of_entries) = match bdl_frame_range {
            PhysFrameRange { start, end } => {
                let start = start.start_address().as_u64();
                let mut max_amount_of_entries = (end.start_address().as_u64() - start) / BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BITS as u64;
                if max_amount_of_entries > MAX_AMOUNT_OF_BUFFER_DESCRIPTOR_LIST_ENTRIES as u64 {
                    max_amount_of_entries = MAX_AMOUNT_OF_BUFFER_DESCRIPTOR_LIST_ENTRIES as u64;
                    info!("WARNING: More memory for buffer descriptor list allocated than necessary")
                }
                (start, max_amount_of_entries as u16)
            }
        };

        Self {
            base_address: bdl_base_address,
            max_amount_of_entries,
        }
    }

    pub fn get_entry(&self, index: u8) -> BufferDescriptorListEntry {
        let raw_data = unsafe { ((self.base_address as u128 + (index as u128 * BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BITS as u128)) as *mut u128).read() };
        BufferDescriptorListEntry::from(raw_data)
    }

    pub fn set_entry(&self, index: u8, entry: &BufferDescriptorListEntry) {
        unsafe { ((self.base_address as u128 + (index as u128 * BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BITS as u128)) as *mut u128).write(entry.as_u128()) };

    }

    pub fn last_valid_index(&self) -> u8 {
        (self.max_amount_of_entries - 1) as u8
    }
}
