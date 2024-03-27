use alloc::vec::Vec;
use core::fmt::LowerHex;
use core::ops::BitAnd;
use log::debug;
use num_traits::int::PrimInt;
use derive_getters::Getters;
use crate::device::ihda_node_infos::{AmpCapabilitiesInfo, AudioFunctionGroupCapabilitiesInfo, AudioWidgetCapabilitiesInfo, ChannelStreamIdInfo, ConfigurationDefaultInfo, ConnectionListEntryInfo, ConnectionListLengthInfo, ConnectionSelectInfo, FunctionGroupTypeInfo, GPIOCountInfo, Info, PinCapabilitiesInfo, PinWidgetControlInfo, ProcessingCapabilitiesInfo, RevisionIdInfo, SampleSizeRateCAPsInfo, StreamFormatInfo, SubordinateNodeCountInfo, SupportedPowerStatesInfo, SupportedStreamFormatsInfo, VendorIdInfo, VolumeKnobCapabilitiesInfo};
use crate::timer;

const MAX_AMOUNT_OF_CODECS: u8 = 15;
const IMMEDIATE_COMMAND_TIMEOUT_IN_MS: usize = 100;

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
    pub unsafe fn clear_bit(&self, index: u8) {
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
    sd0ctl: Register<u32>,
    sd0sts: Register<u8>,
    sd0lpib: Register<u32>,
    sd0cbl: Register<u32>,
    sd0lvi: Register<u16>,
    sd0fifod: Register<u16>,
    sd0fmt: Register<u16>,
    sd0bdpl: Register<u32>,
    sd0bdpu: Register<u32>,
    walclka: Register<u32>,
    sd0lpiba: Register<u32>,
}

impl ControllerRegisterSet {
    pub fn new(mmio_base_address: u64) -> Self {
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
            // careful: the sd0ctl register is only 3 bytes long, so that reading the register as an u32 also reads the sd0sts register in the last byte
            // the last byte of the read value should therefore not be manipulated
            sd0ctl: Register::new((mmio_base_address + 0x80) as *mut u32, "SD0CTL"),
            sd0sts: Register::new((mmio_base_address + 0x83) as *mut u8, "SD0STS"),
            sd0lpib: Register::new((mmio_base_address + 0x84) as *mut u32, "SD0LPIB"),
            sd0cbl: Register::new((mmio_base_address + 0x88) as *mut u32, "SD0CBL"),
            sd0lvi: Register::new((mmio_base_address + 0x8C) as *mut u16, "SD0LVI"),
            // bytes with offset 0x8E to 0x8F are reserved
            sd0fifod: Register::new((mmio_base_address + 0x90) as *mut u16, "SD0FIFOD"),
            sd0fmt: Register::new((mmio_base_address + 0x92) as *mut u16, "SD0FMT"),
            // bytes with offset 0x94 to 0x97 are reserved
            sd0bdpl: Register::new((mmio_base_address + 0x98) as *mut u32, "SD0DPL"),
            sd0bdpu: Register::new((mmio_base_address + 0x9C) as *mut u32, "SD0DPU"),
            // registers for additional stream descriptors starting from byte A0 are optional
            walclka: Register::new((mmio_base_address + 0x2030) as *mut u32, "WALCLKA"),
            sd0lpiba: Register::new((mmio_base_address + 0x2084) as *mut u32, "SD0LPIBA"),
            // registers for additional link positions starting from byte 20A0 are optional
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

    pub fn send_command(&self, addr: &NodeAddress, command: &Command) -> Info {
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
    vendor_id: VendorIdInfo,
    revision_id: RevisionIdInfo,
    subordinate_node_count: SubordinateNodeCountInfo,
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
        vendor_id: VendorIdInfo,
        revision_id: RevisionIdInfo,
        subordinate_node_count: SubordinateNodeCountInfo,
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
    subordinate_node_count: SubordinateNodeCountInfo,
    function_group_type: FunctionGroupTypeInfo,
    audio_function_group_caps: AudioFunctionGroupCapabilitiesInfo,
    sample_size_rate_caps: SampleSizeRateCAPsInfo,
    supported_stream_formats: SupportedStreamFormatsInfo,
    input_amp_caps: AmpCapabilitiesInfo,
    output_amp_caps: AmpCapabilitiesInfo,
    // function group node must provide a SupportedPowerStatesInfo, but QEMU doesn't do it... so this only an Option<SupportedPowerStatesInfo> for now
    supported_power_states: SupportedPowerStatesInfo,
    gpio_count: GPIOCountInfo,
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
        subordinate_node_count: SubordinateNodeCountInfo,
        function_group_type: FunctionGroupTypeInfo,
        audio_function_group_caps: AudioFunctionGroupCapabilitiesInfo,
        sample_size_rate_caps: SampleSizeRateCAPsInfo,
        supported_stream_formats: SupportedStreamFormatsInfo,
        input_amp_caps: AmpCapabilitiesInfo,
        output_amp_caps: AmpCapabilitiesInfo,
        supported_power_states: SupportedPowerStatesInfo,
        gpio_count: GPIOCountInfo,
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
    audio_widget_capabilities: AudioWidgetCapabilitiesInfo,
    widget_info: WidgetInfoContainer,
}

impl Node for WidgetNode {
    fn address(&self) -> &NodeAddress {
        &self.address
    }
}

impl WidgetNode {
    pub fn new(address: NodeAddress, audio_widget_capabilities: AudioWidgetCapabilitiesInfo, widget_info: WidgetInfoContainer) -> Self {
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
        SampleSizeRateCAPsInfo,
        SupportedStreamFormatsInfo,
        AmpCapabilitiesInfo,
        SupportedPowerStatesInfo,
        ProcessingCapabilitiesInfo,
    ),
    AudioInputConverter(
        SampleSizeRateCAPsInfo,
        SupportedStreamFormatsInfo,
        AmpCapabilitiesInfo,
        ConnectionListLengthInfo,
        SupportedPowerStatesInfo,
        ProcessingCapabilitiesInfo,
    ),
    // first AmpCapabilitiesInfo is input amp caps and second AmpCapabilitiesInfo is output amp caps
    PinComplex(
        PinCapabilitiesInfo,
        AmpCapabilitiesInfo,
        AmpCapabilitiesInfo,
        ConnectionListLengthInfo,
        SupportedPowerStatesInfo,
        ProcessingCapabilitiesInfo,
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
            Command::SetConnectionSelect { connection_index } => Self::command_with_12bit_identifier_verb(node_address, command.id(), *connection_index),
            Command::GetConnectionListEntry { offset } => Self::command_with_12bit_identifier_verb(node_address, command.id(), *offset),
            Command::GetStreamFormat => Self::command_with_4bit_identifier_verb(node_address, command.id(), 0x0),
            Command::SetStreamFormat(stream_format) => Self::command_with_4bit_identifier_verb(node_address, command.id(), stream_format.as_u16()),
            Command::GetChannelStreamId => Self::command_with_12bit_identifier_verb(node_address, command.id(), 0x0),
            Command::SetChannelStreamId(channel_stream_id) => Self::command_with_12bit_identifier_verb(node_address, command.id(), channel_stream_id.as_u8()),
            Command::GetPinWidgetControl => Self::command_with_12bit_identifier_verb(node_address, command.id(), 0x0),
            Command::SetPinWidgetControl(pin_control) => Self::command_with_12bit_identifier_verb(node_address, command.id(), pin_control.as_u8()),
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
}

#[derive(Debug)]
pub enum Command {
    GetParameter(Parameter),
    GetConnectionSelect,
    SetConnectionSelect { connection_index: u8 },
    GetConnectionListEntry { offset: u8 },
    GetStreamFormat,
    SetStreamFormat(StreamFormatInfo),
    GetChannelStreamId,
    SetChannelStreamId(ChannelStreamIdInfo),
    GetPinWidgetControl,
    SetPinWidgetControl(PinWidgetControlInfo),
    GetConfigurationDefault,
}

impl Command {
    pub fn id(&self) -> u16 {
        match self {
            Command::GetParameter(_) => 0xF00,
            Command::GetConnectionSelect => 0xF01,
            Command::SetConnectionSelect { connection_index: _ } => 0x701,
            Command::GetConnectionListEntry { offset: _ } => 0xF02,
            Command::GetStreamFormat => 0xA,
            Command::SetStreamFormat(_) => 0x2,
            Command::GetChannelStreamId => 0xF06,
            Command::SetChannelStreamId(_) => 0x706,
            Command::GetPinWidgetControl => 0xF07,
            Command::SetPinWidgetControl(_) => 0x707,
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

pub struct ResponseParser;

impl ResponseParser {
    pub fn parse_response(response: u32, command: &Command) -> Info {
        match command {
            Command::GetParameter(parameter) => {
                match parameter {
                    Parameter::VendorId => Info::VendorId(VendorIdInfo::new(response)),
                    Parameter::RevisionId => Info::RevisionId(RevisionIdInfo::new(response)),
                    Parameter::SubordinateNodeCount => Info::SubordinateNodeCount(SubordinateNodeCountInfo::new(response)),
                    Parameter::FunctionGroupType => Info::FunctionGroupType(FunctionGroupTypeInfo::new(response)),
                    Parameter::AudioFunctionGroupCapabilities => Info::AudioFunctionGroupCapabilities(AudioFunctionGroupCapabilitiesInfo::new(response)),
                    Parameter::AudioWidgetCapabilities => Info::AudioWidgetCapabilities(AudioWidgetCapabilitiesInfo::new(response)),
                    Parameter::SampleSizeRateCAPs => Info::SampleSizeRateCAPs(SampleSizeRateCAPsInfo::new(response)),
                    Parameter::SupportedStreamFormats => Info::SupportedStreamFormats(SupportedStreamFormatsInfo::new(response)),
                    Parameter::PinCapabilities => Info::PinCapabilities(PinCapabilitiesInfo::new(response)),
                    Parameter::InputAmpCapabilities => Info::InputAmpCapabilities(AmpCapabilitiesInfo::new(response)),
                    Parameter::OutputAmpCapabilities => Info::OutputAmpCapabilities(AmpCapabilitiesInfo::new(response)),
                    Parameter::ConnectionListLength => Info::ConnectionListLength(ConnectionListLengthInfo::new(response)),
                    Parameter::SupportedPowerStates => Info::SupportedPowerStates(SupportedPowerStatesInfo::new(response)),
                    Parameter::ProcessingCapabilities => Info::ProcessingCapabilities(ProcessingCapabilitiesInfo::new(response)),
                    Parameter::GPIOCount => Info::GPIOCount(GPIOCountInfo::new(response)),
                    Parameter::VolumeKnobCapabilities => Info::VolumeKnobCapabilities(VolumeKnobCapabilitiesInfo::new(response)),
                }
            }
            Command::GetConnectionSelect => Info::ConnectionSelect(ConnectionSelectInfo::new(response)),
            Command::SetConnectionSelect { .. } => Info::SetInfo,
            Command::GetConnectionListEntry { .. } => Info::ConnectionListEntry(ConnectionListEntryInfo::new(response)),
            Command::GetStreamFormat { .. } => Info::StreamFormat(StreamFormatInfo::new(response)),
            Command::SetStreamFormat { .. } => Info::SetInfo,
            Command::GetChannelStreamId => Info::ChannelStreamId(ChannelStreamIdInfo::new(response)),
            Command::SetChannelStreamId(_) => Info::SetInfo,
            Command::GetConfigurationDefault => Info::ConfigurationDefault(ConfigurationDefaultInfo::new(response)),
            Command::GetPinWidgetControl => Info::PinWidgetControl(PinWidgetControlInfo::new(response)),
            Command::SetPinWidgetControl { .. } => Info::SetInfo,
        }
    }
}
