use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::LowerHex;
use core::ops::BitAnd;
use num_traits::int::PrimInt;
use derive_getters::Getters;

const MAX_AMOUNT_OF_CODECS: u8 = 15;

// representation of an IHDA register
#[derive(Getters)]
pub struct Register<T: LowerHex + PrimInt> {
    ptr: *mut T,
    name: String,
}

// the LowerHex type bound is only necessary because of the dump function which displays T as a hex value
// the PrimeInt type bound is necessary because of the bit operations | and <<
impl<T: LowerHex + PrimInt> Register<T> {
    fn new(ptr: *mut T, name: String) -> Self {
        Self {
            ptr,
            name,
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
    pub fn new(mmio_address: u32) -> Self {
        Self {
            gcap: Register::new(mmio_address as *mut u16, String::from("GCAP")),
            vmin: Register::new((mmio_address + 0x2) as *mut u8, String::from("VMIN")),
            vmaj: Register::new((mmio_address + 0x3) as *mut u8, String::from("VMAJ")),
            outpay: Register::new((mmio_address + 0x4) as *mut u16, String::from("OUTPAY")),
            inpay: Register::new((mmio_address + 0x6) as *mut u16, String::from("INPAY")),
            gctl: Register::new((mmio_address + 0x8) as *mut u32, String::from("GCTL")),
            wakeen: Register::new((mmio_address + 0xC) as *mut u16, String::from("WAKEEN")),
            wakests: Register::new((mmio_address + 0xE) as *mut u16, String::from("WAKESTS")),
            gsts: Register::new((mmio_address + 0x10) as *mut u16, String::from("GSTS")),
            // bytes with offset 0x12 to 0x17 are reserved
            outstrmpay: Register::new((mmio_address + 0x18) as *mut u16, String::from("OUTSTRMPAY")),
            instrmpay: Register::new((mmio_address + 0x1A) as *mut u16, String::from("INSTRMPAY")),
            // bytes with offset 0x1C to 0x1F are reserved
            intctl: Register::new((mmio_address + 0x20) as *mut u32, String::from("INTCTL")),
            intsts: Register::new((mmio_address + 0x24) as *mut u32, String::from("INTSTS")),
            // bytes with offset 0x28 to 0x2F are reserved
            walclk: Register::new((mmio_address + 0x30) as *mut u32, String::from("WALCLK")),
            // bytes with offset 0x34 to 0x37 are reserved
            ssync: Register::new((mmio_address + 0x38) as *mut u32, String::from("SSYNC")),
            // bytes with offset 0x3C to 0x3F are reserved
            corblbase: Register::new((mmio_address + 0x40) as *mut u32, String::from("CORBLBASE")),
            corbubase: Register::new((mmio_address + 0x44) as *mut u32, String::from("CORBUBASE")),
            corbwp: Register::new((mmio_address + 0x48) as *mut u16, String::from("CORBWP")),
            corbrp: Register::new((mmio_address + 0x4A) as *mut u16, String::from("CORBRP")),
            corbctl: Register::new((mmio_address + 0x4C) as *mut u8, String::from("CORBCTL")),
            corbsts: Register::new((mmio_address + 0x4D) as *mut u8, String::from("CORBSTS")),
            corbsize: Register::new((mmio_address + 0x4E) as *mut u8, String::from("CORBSIZE")),
            // byte with offset 0x4F is reserved
            rirblbase: Register::new((mmio_address + 0x50) as *mut u32, String::from("RIRBLBASE")),
            rirbubase: Register::new((mmio_address + 0x54) as *mut u32, String::from("RIRBUBASE")),
            rirbwp: Register::new((mmio_address + 0x58) as *mut u16, String::from("RIRBWP")),
            rintcnt: Register::new((mmio_address + 0x5A) as *mut u16, String::from("RINTCNT")),
            rirbctl: Register::new((mmio_address + 0x5C) as *mut u8, String::from("RIRBCTL")),
            rirbsts: Register::new((mmio_address + 0x5D) as *mut u8, String::from("RIRBSTS")),
            rirbsize: Register::new((mmio_address + 0x5E) as *mut u8, String::from("RIRBSIZE")),
            // byte with offset 0x5F is reserved
            // the following three immediate command registers from bytes 0x60 to 0x69 are optional
            icoi: Register::new((mmio_address + 0x60) as *mut u32, String::from("ICOI")),
            icii: Register::new((mmio_address + 0x64) as *mut u32, String::from("ICII")),
            icis: Register::new((mmio_address + 0x68) as *mut u16, String::from("ICIS")),
            // bytes with offset 0x6A to 0x6F are reserved
            dpiblbase: Register::new((mmio_address + 0x70) as *mut u32, String::from("DPIBLBASE")),
            dpibubase: Register::new((mmio_address + 0x74) as *mut u32, String::from("DPIBUBASE")),
            // bytes with offset 0x78 to 0x7F are reserved
            // careful: the sd0ctl register is only 3 bytes long, so that reading the register as an u32 also reads the sd0sts register in the last byte
            // the last byte of the read value should therefore not be manipulated
            sd0ctl: Register::new((mmio_address + 0x80) as *mut u32, String::from("SD0CTL")),
            sd0sts: Register::new((mmio_address + 0x83) as *mut u8, String::from("SD0STS")),
            sd0lpib: Register::new((mmio_address + 0x84) as *mut u32, String::from("SD0LPIB")),
            sd0cbl: Register::new((mmio_address + 0x88) as *mut u32, String::from("SD0CBL")),
            sd0lvi: Register::new((mmio_address + 0x8C) as *mut u16, String::from("SD0LVI")),
            // bytes with offset 0x8E to 0x8F are reserved
            sd0fifod: Register::new((mmio_address + 0x90) as *mut u16, String::from("SD0FIFOD")),
            sd0fmt: Register::new((mmio_address + 0x92) as *mut u16, String::from("SD0FMT")),
            // bytes with offset 0x94 to 0x97 are reserved
            sd0bdpl: Register::new((mmio_address + 0x98) as *mut u32, String::from("SD0DPL")),
            sd0bdpu: Register::new((mmio_address + 0x9C) as *mut u32, String::from("SD0DPU")),
            // registers for additional stream descriptors starting from byte A0 are optional
            walclka: Register::new((mmio_address + 0x2030) as *mut u32, String::from("WALCLKA")),
            sd0lpiba: Register::new((mmio_address + 0x2084) as *mut u32, String::from("SD0LPIBA")),
            // registers for additional link positions starting from byte 20A0 are optional
        }
    }
}

#[derive(Getters)]
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

#[derive(Getters)]
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

#[derive(Getters)]
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

#[derive(Getters)]
pub struct FunctionGroupNode {
    address: NodeAddress,
    subordinate_node_count: SubordinateNodeCountInfo,
    function_group_type: FunctionGroupTypeInfo,
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
        widgets: Vec<WidgetNode>
    ) -> Self {
        FunctionGroupNode {
            address,
            subordinate_node_count,
            function_group_type,
            widgets
        }
    }
}

#[derive(Getters)]
pub struct WidgetNode {
    address: NodeAddress,
    audio_widget_capabilities: AudioWidgetCapabilitiesInfo,
}

impl Node for WidgetNode {
    fn address(&self) -> &NodeAddress {
        &self.address
    }
}

impl WidgetNode {
    pub fn new(address: NodeAddress, audio_widget_capabilities: AudioWidgetCapabilitiesInfo) -> Self {
        WidgetNode {
            address,
            audio_widget_capabilities,
        }
    }

    pub fn max_number_of_channels(&self) -> u8 {
        // this formula can be found in section 7.3.4.6, Audio Widget Capabilities of the specification
        (self.audio_widget_capabilities.chan_count_ext << 1) + (self.audio_widget_capabilities.chan_count_lsb as u8) + 1
    }
}

pub struct CommandBuilder;

impl CommandBuilder {
    pub fn get_parameter(node_address: &NodeAddress, parameter: Parameter) -> u32 {
        Self::command_with_12bit_identifier_verb(node_address, 0xF00, parameter.id())
    }

    // two example commands (temporarily unused)
    pub fn get_connection_select(node_address: &NodeAddress) -> u32 {
        Self::command_with_12bit_identifier_verb(node_address, 0xF01, 0x0)
    }

    pub fn set_connection_select(node_address: &NodeAddress, connection_index_value: u8) -> u32 {
        Self::command_with_12bit_identifier_verb(node_address, 0x701, connection_index_value)
    }

    fn command_with_12bit_identifier_verb(node_address: &NodeAddress, verb_id: u16, payload: u8) -> u32 {
        (node_address.codec_address as u32) << 28
            | (node_address.node_id as u32) << 20
            | (verb_id as u32) << 8
            | payload as u32
    }
}

// compare to table 140 in section 7.3.6 of the specification
pub enum Parameter {
    VendorId,
    RevisionId,
    SubordinateNodeCount,
    FunctionGroupType,
    AudioFunctionGroupCapabilities,
    AudioWidgetCapabilities,
    SampleSizeRateCAPs,
    StreamFormats,
    PinCapabilities,
    InputAmpCapabilities,
    OutputAmpCapabilities,
    ConnectionLengthList,
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
            Parameter::StreamFormats => 0x0B,
            Parameter::PinCapabilities => 0x0C,
            Parameter::InputAmpCapabilities => 0x0D,
            Parameter::OutputAmpCapabilities => 0x12,
            Parameter::ConnectionLengthList => 0x0E,
            Parameter::SupportedPowerStates => 0x0F,
            Parameter::ProcessingCapabilities => 0x10,
            Parameter::GPIOCount => 0x11,
            Parameter::VolumeKnobCapabilities => 0x13,
        }
    }
}

pub struct ResponseParser;

impl ResponseParser {
    pub fn get_parameter_vendor_id(response: u32) -> VendorIdInfo {
        VendorIdInfo::new(response)
    }

    pub fn get_parameter_revision_id(response: u32) -> RevisionIdInfo {
        RevisionIdInfo::new(response)
    }

    pub fn get_parameter_subordinate_node_count(response: u32) -> SubordinateNodeCountInfo {
        SubordinateNodeCountInfo::new(response)
    }

    pub fn get_parameter_function_group_type(response: u32) -> FunctionGroupTypeInfo {
        FunctionGroupTypeInfo::new(response)
    }

    pub fn get_parameter_audio_widget_capabilities(response: u32) -> AudioWidgetCapabilitiesInfo {
        AudioWidgetCapabilitiesInfo::new(response)
    }
}

#[derive(Getters)]
pub struct VendorIdInfo {
    device_id: u16,
    vendor_id: u16,
}

impl VendorIdInfo {
    fn new(response: u32) -> Self {
        VendorIdInfo {
            device_id: response.bitand(0xFFFF) as u16,
            vendor_id: (response >> 16) as u16,
        }

    }
}

#[derive(Getters)]
pub struct RevisionIdInfo {
    stepping_id: u8,
    revision_id: u8,
    minor_revision: u8,
    major_revision: u8,
}

impl RevisionIdInfo {
    fn new(response: u32) -> Self {
        RevisionIdInfo {
            stepping_id: response.bitand(0xFF) as u8,
            revision_id: (response >> 8).bitand(0xFF) as u8,
            minor_revision: (response >> 16).bitand(0xF) as u8,
            major_revision: (response >> 20) as u8,
        }
    }
}

#[derive(Getters)]
pub struct SubordinateNodeCountInfo {
    total_number_of_nodes: u8,
    starting_node_number: u8,
}

impl SubordinateNodeCountInfo {
    fn new(response: u32) -> Self {
        SubordinateNodeCountInfo {
            total_number_of_nodes: response.bitand(0xFF) as u8,
            starting_node_number: (response >> 16) as u8,
        }

    }
}

#[derive(Getters)]
pub struct FunctionGroupTypeInfo {
    node_type: FunctionGroupNodeType,
    unsolicited_response_capable: bool,
}

impl FunctionGroupTypeInfo {
    fn new(response: u32) -> Self {
        FunctionGroupTypeInfo {
            node_type: match response.bitand(0xFF) as u8 {
                0x1 => FunctionGroupNodeType::AudioFunctionGroup,
                0x2 => FunctionGroupNodeType::VendorDefinedFunctionGroup,
                0x80..=0xFF => FunctionGroupNodeType::VendorDefinedModemFunctionGroup,
                _ => panic!("Unknown function group node type!")
            },
            unsolicited_response_capable: (response >> 8) != 0,
        }

    }
}

pub enum FunctionGroupNodeType {
    AudioFunctionGroup,
    VendorDefinedModemFunctionGroup,
    VendorDefinedFunctionGroup,
}

#[derive(Debug, Getters)]
pub struct AudioWidgetCapabilitiesInfo {
    chan_count_lsb: bool,
    in_amp_present: bool,
    out_amp_present: bool,
    amp_param_override: bool,
    format_override: bool,
    stripe: bool,
    proc_widget: bool,
    unsol_capable: bool,
    conn_list: bool,
    digital: bool,
    power_cntrl: bool,
    lr_swap: bool,
    cp_caps: bool,
    chan_count_ext: u8,
    delay: u8,
    widget_type: WidgetType,
}

impl AudioWidgetCapabilitiesInfo {
    fn new(response: u32) -> Self {
        AudioWidgetCapabilitiesInfo {
            chan_count_lsb: get_bit(response, 0),
            in_amp_present: get_bit(response, 1),
            out_amp_present: get_bit(response, 2),
            amp_param_override: get_bit(response, 3),
            format_override: get_bit(response, 4),
            stripe: get_bit(response, 5),
            proc_widget: get_bit(response, 6),
            unsol_capable: get_bit(response, 7),
            conn_list: get_bit(response, 8),
            digital: get_bit(response, 9),
            power_cntrl: get_bit(response, 10),
            lr_swap: get_bit(response, 11),
            cp_caps: get_bit(response, 12),
            chan_count_ext: (response >> 13).bitand(0b111) as u8,
            delay: (response >> 16).bitand(0xF) as u8,
            widget_type: match (response >> 20).bitand(0xF) as u8 {
                0x0 => WidgetType::AudioOutput,
                0x1 => WidgetType::AudioInput,
                0x2 => WidgetType::AudioMixer,
                0x3 => WidgetType::AudioSelector,
                0x4 => WidgetType::PinComplex,
                0x5 => WidgetType::PowerWidget,
                0x6 => WidgetType::VolumeKnobWidget,
                0x7 => WidgetType::BeepGeneratorWidget,
                0xF => WidgetType::VendorDefinedAudioWidget,
                _ => panic!("Unsupported widget type!")
            }
        }
    }
}

#[derive(Debug)]
pub enum WidgetType {
    AudioOutput,
    AudioInput,
    AudioMixer,
    AudioSelector,
    PinComplex,
    PowerWidget,
    VolumeKnobWidget,
    BeepGeneratorWidget,
    VendorDefinedAudioWidget,
}

fn get_bit<T: LowerHex + PrimInt>(input: T, index: usize) -> bool {
    let zero = T::from(0x0).expect("As only u8, u16 and u32 are used as types for T, this should never fail");
    let one = T::from(0x1).expect("As only u8, u16 and u32 are used as types for T, this should never fail");
    (input >> index).bitand(one) != zero
}
