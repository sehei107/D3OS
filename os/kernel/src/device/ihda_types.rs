use alloc::vec::Vec;
use core::fmt::LowerHex;
use core::ops::BitAnd;
use log::debug;
use num_traits::int::PrimInt;
use crate::timer;
use derive_getters::Getters;

// representation of an IHDA register
pub struct Register<T> {
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

    pub unsafe fn read(&self) -> T {
        self.ptr.read()
    }

    pub unsafe fn write(&self, value: T) {
        self.ptr.write(value);
    }

    pub unsafe fn set_bit(&self, index: u8) {
        let bitmask: u32 = 0x1 << index;
        self.write(self.read() | T::from(bitmask).expect("As only u8, u16 and u32 are used as types for T, this should only fail if index is out of register range"));
    }

    pub unsafe fn clear_bit(&self, index: u8) {
        let bitmask: u32 = 0x1 << index;
        self.write(self.read() & !T::from(bitmask).expect("As only u8, u16 and u32 are used as types for T, this should only fail if index is out of register range"));
    }

    pub unsafe fn set_all_bits(&self) {
        self.write(!T::from(0).expect("As only u8, u16 and u32 are used as types for T, this should never fail"));
    }

    pub unsafe fn clear_all_bits(&self) {
        self.write(T::from(0).expect("As only u8, u16 and u32 are used as types for T, this should never fail"));
    }

    pub unsafe fn assert_bit(&self, index: u8) -> bool {
        let bitmask: u32 = 0x1 << index;
        (self.read() & T::from(bitmask).expect("As only u8, u16 and u32 are used as types for T, this should only fail if index is out of register range"))
            != T::from(0).expect("As only u8, u16 and u32 are used as types for T, this should never fail")
    }

    pub unsafe fn dump(&self) {
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
    pub const fn new(mmio_address: u32) -> Self {
        Self {
            gcap: Register::new(mmio_address as *mut u16, "GCAP"),
            vmin: Register::new((mmio_address + 0x2) as *mut u8, "VMIN"),
            vmaj: Register::new((mmio_address + 0x3) as *mut u8, "VMAJ"),
            outpay: Register::new((mmio_address + 0x4) as *mut u16, "OUTPAY"),
            inpay: Register::new((mmio_address + 0x6) as *mut u16, "INPAY"),
            gctl: Register::new((mmio_address + 0x8) as *mut u32, "GCTL"),
            wakeen: Register::new((mmio_address + 0xC) as *mut u16, "WAKEEN"),
            wakests: Register::new((mmio_address + 0xE) as *mut u16, "WAKESTS"),
            gsts: Register::new((mmio_address + 0x10) as *mut u16, "GSTS"),
            // bytes with offset 0x12 to 0x17 are reserved
            outstrmpay: Register::new((mmio_address + 0x18) as *mut u16, "OUTSTRMPAY"),
            instrmpay: Register::new((mmio_address + 0x1A) as *mut u16, "INSTRMPAY"),
            // bytes with offset 0x1C to 0x1F are reserved
            intctl: Register::new((mmio_address + 0x20) as *mut u32, "INTCTL"),
            intsts: Register::new((mmio_address + 0x24) as *mut u32, "INTSTS"),
            // bytes with offset 0x28 to 0x2F are reserved
            walclk: Register::new((mmio_address + 0x30) as *mut u32, "WALCLK"),
            // bytes with offset 0x34 to 0x37 are reserved
            ssync: Register::new((mmio_address + 0x38) as *mut u32, "SSYNC"),
            // bytes with offset 0x3C to 0x3F are reserved
            corblbase: Register::new((mmio_address + 0x40) as *mut u32, "CORBLBASE"),
            corbubase: Register::new((mmio_address + 0x44) as *mut u32, "CORBUBASE"),
            corbwp: Register::new((mmio_address + 0x48) as *mut u16, "CORBWP"),
            corbrp: Register::new((mmio_address + 0x4A) as *mut u16, "CORBRP"),
            corbctl: Register::new((mmio_address + 0x4C) as *mut u8, "CORBCTL"),
            corbsts: Register::new((mmio_address + 0x4D) as *mut u8, "CORBSTS"),
            corbsize: Register::new((mmio_address + 0x4E) as *mut u8, "CORBSIZE"),
            // byte with offset 0x4F is reserved
            rirblbase: Register::new((mmio_address + 0x50) as *mut u32, "RIRBLBASE"),
            rirbubase: Register::new((mmio_address + 0x54) as *mut u32, "RIRBUBASE"),
            rirbwp: Register::new((mmio_address + 0x58) as *mut u16, "RIRBWP"),
            rintcnt: Register::new((mmio_address + 0x5A) as *mut u16, "RINTCNT"),
            rirbctl: Register::new((mmio_address + 0x5C) as *mut u8, "RIRBCTL"),
            rirbsts: Register::new((mmio_address + 0x5D) as *mut u8, "RIRBSTS"),
            rirbsize: Register::new((mmio_address + 0x5E) as *mut u8, "RIRBSIZE"),
            // byte with offset 0x5F is reserved
            // the following three immediate command registers from bytes 0x60 to 0x69 are optional
            icoi: Register::new((mmio_address + 0x60) as *mut u32, "ICOI"),
            icii: Register::new((mmio_address + 0x64) as *mut u32, "ICII"),
            icis: Register::new((mmio_address + 0x68) as *mut u16, "ICIS"),
            // bytes with offset 0x6A to 0x6F are reserved
            dpiblbase: Register::new((mmio_address + 0x70) as *mut u32, "DPIBLBASE"),
            dpibubase: Register::new((mmio_address + 0x74) as *mut u32, "DPIBUBASE"),
            // bytes with offset 0x78 to 0x7F are reserved
            // careful: the sd0ctl register is only 3 bytes long, so that reading the register as an u32 also reads the sd0sts register in the last byte
            // the last byte of the read value should therefore not be manipulated
            sd0ctl: Register::new((mmio_address + 0x80) as *mut u32, "SD0CTL"),
            sd0sts: Register::new((mmio_address + 0x83) as *mut u8, "SD0STS"),
            sd0lpib: Register::new((mmio_address + 0x84) as *mut u32, "SD0LPIB"),
            sd0cbl: Register::new((mmio_address + 0x88) as *mut u32, "SD0CBL"),
            sd0lvi: Register::new((mmio_address + 0x8C) as *mut u16, "SD0LVI"),
            // bytes with offset 0x8E to 0x8F are reserved
            sd0fifod: Register::new((mmio_address + 0x90) as *mut u16, "SD0FIFOD"),
            sd0fmt: Register::new((mmio_address + 0x92) as *mut u16, "SD0FMT"),
            // bytes with offset 0x94 to 0x97 are reserved
            sd0bdpl: Register::new((mmio_address + 0x98) as *mut u32, "SD0DPL"),
            sd0bdpu: Register::new((mmio_address + 0x9C) as *mut u32, "SD0DPU"),
            // registers for additional stream descriptors starting from byte A0 are optional
            walclka: Register::new((mmio_address + 0x2030) as *mut u32, "WALCLKA"),
            sd0lpiba: Register::new((mmio_address + 0x2084) as *mut u32, "SD0LPIBA"),
            // registers for additional link positions starting from byte 20A0 are optional
        }
    }

    pub unsafe fn immediate_command(&self, command: Command) -> u32 {
        self.icis.write(0b10);
        self.icoi.write(command.value());
        self.icis.write(0b1);
        let start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        const ICIS_TIMEOUT: usize = 100;
        while (self.icis.read() & 0b10) != 0b10 {
            if timer().read().systime_ms() > start_timer + ICIS_TIMEOUT {
                panic!("IHDA immediate command timed out")
            }
        }
        self.icii.read()
    }
}

#[derive(Getters)]
pub struct NodeAddress {
    codec_address: u8,
    node_id: u8,
}

impl NodeAddress {
    pub fn new(codec_address: u8, node_id: u8) -> Self {
        NodeAddress {
            codec_address,
            node_id,
        }
    }
}

#[derive(Getters)]
pub struct Command {
    codec_address: u8,
    node_id: u8,
    verb: u16,
    parameter: u8,
}

impl Command {
    pub fn new(address: &NodeAddress, verb: u16, parameter: u8,) -> Self {
        Command {
            codec_address: address.codec_address,
            node_id: address.node_id,
            verb,
            parameter,
        }
    }

    pub fn get_parameter(address: &NodeAddress, parameter: ParameterType) -> Self {
        Command::new(address, 0xF00, parameter.parameter_id())
    }

    pub fn value(&self) -> u32 {
        (self.codec_address as u32) << 28 | (self.node_id as u32) << 20 | (self.verb as u32) << 8 | self.parameter as u32
    }
}

// compare to table 140 in section 7.3.6 of the specification
pub enum ParameterType {
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

impl ParameterType {
    pub fn parameter_id(&self) -> u8 {
        match self {
            ParameterType::VendorId => 0x00,
            ParameterType::RevisionId => 0x02,
            ParameterType::SubordinateNodeCount => 0x04,
            ParameterType::FunctionGroupType => 0x05,
            ParameterType::AudioFunctionGroupCapabilities => 0x08,
            ParameterType::AudioWidgetCapabilities => 0x09,
            ParameterType::SampleSizeRateCAPs => 0x0A,
            ParameterType::StreamFormats => 0x0B,
            ParameterType::PinCapabilities => 0x0C,
            ParameterType::InputAmpCapabilities => 0x0D,
            ParameterType::OutputAmpCapabilities => 0x12,
            ParameterType::ConnectionLengthList => 0x0E,
            ParameterType::SupportedPowerStates => 0x0F,
            ParameterType::ProcessingCapabilities => 0x10,
            ParameterType::GPIOCount => 0x11,
            ParameterType::VolumeKnobCapabilities => 0x13,
        }
    }
}

#[derive(Getters)]
pub struct Codec {
    codec_address: u8,
    root_node: RootNode,
    function_group_nodes: Vec<FunctionGroupNode>,
}

impl Codec {
    pub fn new(codec_address: u8, root_node: RootNode, function_group_nodes: Vec<FunctionGroupNode>) -> Self {
        Codec {
            codec_address,
            root_node,
            function_group_nodes,
        }
    }
}

pub trait Node {
    fn address(&self) -> &NodeAddress;
}

#[derive(Getters)]
pub struct RootNode {
    address: NodeAddress,
}

impl Node for RootNode {
    fn address(&self) -> &NodeAddress {
        &self.address
    }
}

impl RootNode {
    pub fn new(codec_address: u8) -> Self {
        RootNode {
            address: NodeAddress::new(codec_address, 0),
        }
    }

    pub fn get_parameter(&self, parameter: ParameterType,) -> Command {
        Command::get_parameter(self.address(), parameter)
    }
}

#[derive(Getters)]
pub struct FunctionGroupNode {
    address: NodeAddress,
    widgets: Vec<WidgetNode>,
}

impl Node for FunctionGroupNode {
    fn address(&self) -> &NodeAddress {
        &self.address
    }
}

impl FunctionGroupNode {
    pub fn new(address: NodeAddress, widgets: Vec<WidgetNode>) -> Self {
        FunctionGroupNode {
            address,
            widgets
        }
    }
}

enum WidgetType {
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

pub struct WidgetNode {
    address: NodeAddress,
    widget_type: WidgetType,
    delay: u8,
    chan_count_ext: u8,
    cp_caps: bool,
    lr_swap: bool,
    power_cntrl: bool,
    digital: bool,
    conn_list: bool,
    unsol_capable: bool,
    proc_widget: bool,
    stripe: bool,
    format_override: bool,
    amp_param_override: bool,
    out_amp_present: bool,
    in_amp_present: bool,
    chan_count_lsb: bool,
}

impl Node for WidgetNode {
    fn address(&self) -> &NodeAddress {
        &self.address
    }
}

impl WidgetNode {
    pub fn new(address: NodeAddress, response: u32) -> Self {
        let widget_type = match (response >> 20).bitand(0xF) as u8 {
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
        };

        WidgetNode {
            address,
            widget_type,
            delay: (response >> 16).bitand(0xF) as u8,
            chan_count_ext: (response >> 13).bitand(0xFF) as u8,
            cp_caps: (response >> 12).bitand(0x1) != 0,
            lr_swap: (response >> 11).bitand(0x1) != 0,
            power_cntrl: (response >> 10).bitand(0x1) != 0,
            digital: (response >> 9).bitand(0x1) != 0,
            conn_list: (response >> 8).bitand(0x1) != 0,
            unsol_capable: (response >> 7).bitand(0x1) != 0,
            proc_widget: (response >> 6).bitand(0x1) != 0,
            stripe: (response >> 5).bitand(0x1) != 0,
            format_override: (response >> 4).bitand(0x1) != 0,
            amp_param_override: (response >> 3).bitand(0x1) != 0,
            out_amp_present: (response >> 2).bitand(0x1) != 0,
            in_amp_present: (response >> 1).bitand(0x1) != 0,
            chan_count_lsb: response.bitand(0x1) != 0,
        }
    }

    pub fn max_number_of_channels(&self) -> u8 {
        // this formula can be found in section 7.3.4.6, Audio Widget Capabilities of the specification
        (self.chan_count_ext << 1) + (self.chan_count_lsb as u8) + 1
    }
}

fn subordinate_node_count<T: Node>(node: &T) -> Command {
    Command::get_parameter(node.address(), ParameterType::SubordinateNodeCount)
}
