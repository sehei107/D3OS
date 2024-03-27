use core::fmt::LowerHex;
use core::ops::BitAnd;
use num_traits::int::PrimInt;
use derive_getters::Getters;

#[derive(Debug)]
pub enum Info {
    VendorId(VendorIdInfo),
    RevisionId(RevisionIdInfo),
    SubordinateNodeCount(SubordinateNodeCountInfo),
    FunctionGroupType(FunctionGroupTypeInfo),
    AudioFunctionGroupCapabilities(AudioFunctionGroupCapabilitiesInfo),
    AudioWidgetCapabilities(AudioWidgetCapabilitiesInfo),
    SampleSizeRateCAPs(SampleSizeRateCAPsInfo),
    SupportedStreamFormats(SupportedStreamFormatsInfo),
    PinCapabilities(PinCapabilitiesInfo),
    InputAmpCapabilities(AmpCapabilitiesInfo),
    OutputAmpCapabilities(AmpCapabilitiesInfo),
    ConnectionListLength(ConnectionListLengthInfo),
    SupportedPowerStates(SupportedPowerStatesInfo),
    ProcessingCapabilities(ProcessingCapabilitiesInfo),
    GPIOCount(GPIOCountInfo),
    VolumeKnobCapabilities(VolumeKnobCapabilitiesInfo),

    ConnectionSelect(ConnectionSelectInfo),
    ConnectionListEntry(ConnectionListEntryInfo),

    ChannelStreamId(ChannelStreamIdInfo),

    StreamFormat(StreamFormatInfo),

    PinWidgetControl(PinWidgetControlInfo),

    ConfigurationDefault(ConfigurationDefaultInfo),

    SetInfo,
}

#[derive(Debug, Getters)]
pub struct VendorIdInfo {
    device_id: u16,
    vendor_id: u16,
}

impl VendorIdInfo {
    pub fn new(response: u32) -> Self {
        VendorIdInfo {
            device_id: response.bitand(0xFFFF) as u16,
            vendor_id: (response >> 16).bitand(0xFFFF) as u16,
        }

    }
}

impl TryFrom<Info> for VendorIdInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
                    Info::VendorId(info) => Ok(info),
                    e => Err(e),
                }
    }
}

#[derive(Debug, Getters)]
pub struct RevisionIdInfo {
    stepping_id: u8,
    revision_id: u8,
    minor_revision: u8,
    major_revision: u8,
}

impl RevisionIdInfo {
    pub fn new(response: u32) -> Self {
        RevisionIdInfo {
            stepping_id: response.bitand(0xFF) as u8,
            revision_id: (response >> 8).bitand(0xFF) as u8,
            minor_revision: (response >> 16).bitand(0xF) as u8,
            major_revision: (response >> 20).bitand(0xF) as u8,
        }
    }
}

impl TryFrom<Info> for RevisionIdInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::RevisionId(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct SubordinateNodeCountInfo {
    total_number_of_nodes: u8,
    starting_node_number: u8,
}

impl SubordinateNodeCountInfo {
    pub fn new(response: u32) -> Self {
        SubordinateNodeCountInfo {
            total_number_of_nodes: response.bitand(0xFF) as u8,
            starting_node_number: (response >> 16).bitand(0xFF) as u8,
        }

    }
}

impl TryFrom<Info> for SubordinateNodeCountInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::SubordinateNodeCount(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct FunctionGroupTypeInfo {
    node_type: FunctionGroupType,
    unsolicited_response_capable: bool,
}

impl FunctionGroupTypeInfo {
    pub fn new(response: u32) -> Self {
        FunctionGroupTypeInfo {
            node_type: match response.bitand(0xFF) as u8 {
                0x1 => FunctionGroupType::AudioFunctionGroup,
                0x2 => FunctionGroupType::VendorDefinedFunctionGroup,
                0x80..=0xFF => FunctionGroupType::VendorDefinedModemFunctionGroup,
                _ => panic!("Unknown function group node type!")
            },
            unsolicited_response_capable: get_bit(response, 8),
        }

    }
}

impl TryFrom<Info> for FunctionGroupTypeInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::FunctionGroupType(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug)]
pub enum FunctionGroupType {
    AudioFunctionGroup,
    VendorDefinedModemFunctionGroup,
    VendorDefinedFunctionGroup,
}

#[derive(Debug, Getters)]
pub struct AudioFunctionGroupCapabilitiesInfo {
    output_delay: u8,
    input_delay: u8,
    beep_gen: bool,
}

impl AudioFunctionGroupCapabilitiesInfo {
    pub fn new(response: u32) -> Self {
    AudioFunctionGroupCapabilitiesInfo {
            output_delay: response.bitand(0xF) as u8,
            input_delay: (response >> 8).bitand(0xF) as u8,
            beep_gen: get_bit(response, 16),
        }
    }
}

impl TryFrom<Info> for AudioFunctionGroupCapabilitiesInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::AudioFunctionGroupCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
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
    pub fn new(response: u32) -> Self {
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

impl TryFrom<Info> for AudioWidgetCapabilitiesInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::AudioWidgetCapabilities(info) => Ok(info),
            e => Err(e),
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

#[derive(Debug, Getters)]
pub struct SampleSizeRateCAPsInfo {
    support_8000hz: bool,
    support_11025hz: bool,
    support_16000hz: bool,
    support_22050hz: bool,
    support_32000hz: bool,
    support_44100hz: bool,
    support_48000hz: bool,
    support_88200hz: bool,
    support_96000hz: bool,
    support_176400hz: bool,
    support_192000hz: bool,
    support_384000hz: bool,
    support_8bit: bool,
    support_16bit: bool,
    support_20bit: bool,
    support_24bit: bool,
    support_32bit: bool,
}

impl SampleSizeRateCAPsInfo {
    pub fn new(response: u32) -> Self {
        SampleSizeRateCAPsInfo {
            support_8000hz: get_bit(response, 0),
            support_11025hz: get_bit(response, 1),
            support_16000hz: get_bit(response, 2),
            support_22050hz: get_bit(response, 3),
            support_32000hz: get_bit(response, 4),
            support_44100hz: get_bit(response, 5),
            support_48000hz: get_bit(response, 6),
            support_88200hz: get_bit(response, 7),
            support_96000hz: get_bit(response, 8),
            support_176400hz: get_bit(response, 9),
            support_192000hz: get_bit(response, 10),
            support_384000hz: get_bit(response, 11),
            support_8bit: get_bit(response, 16),
            support_16bit: get_bit(response, 17),
            support_20bit: get_bit(response, 18),
            support_24bit: get_bit(response, 19),
            support_32bit: get_bit(response, 20),
        }
    }
}

impl TryFrom<Info> for SampleSizeRateCAPsInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::SampleSizeRateCAPs(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct SupportedStreamFormatsInfo {
    pcm: bool,
    float32: bool,
    ac3: bool,
}

impl SupportedStreamFormatsInfo {
    pub fn new(response: u32) -> Self {
        SupportedStreamFormatsInfo {
            pcm: get_bit(response, 0),
            float32: get_bit(response, 1),
            ac3: get_bit(response, 2),
        }
    }
}

impl TryFrom<Info> for SupportedStreamFormatsInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::SupportedStreamFormats(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct PinCapabilitiesInfo {
    impedence_sense_capable: bool,
    trigger_required: bool,
    presence_detect_capable: bool,
    headphone_drive_capable: bool,
    output_capable: bool,
    input_capable: bool,
    balanced_io_pins: bool,
    hdmi: bool,
    vref_control: u8,
    eapd_capable: bool,
    display_port: bool,
    high_bit_rate: bool,
}

impl PinCapabilitiesInfo {
    pub fn new(response: u32) -> Self {
        PinCapabilitiesInfo {
            impedence_sense_capable: get_bit(response, 0),
            trigger_required: get_bit(response, 1),
            presence_detect_capable: get_bit(response, 2),
            headphone_drive_capable: get_bit(response, 3),
            output_capable: get_bit(response, 4),
            input_capable: get_bit(response, 5),
            balanced_io_pins: get_bit(response, 6),
            hdmi: get_bit(response, 7),
            vref_control: (response >> 8).bitand(0xFF) as u8,
            eapd_capable: get_bit(response, 16),
            display_port: get_bit(response, 24),
            high_bit_rate: get_bit(response, 27),
        }
    }
}

impl TryFrom<Info> for PinCapabilitiesInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::PinCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct AmpCapabilitiesInfo {
    offset: u8,
    num_steps: u8,
    step_size: u8,
    mute_capable: bool,
}

impl AmpCapabilitiesInfo {
    pub fn new(response: u32) -> Self {
        AmpCapabilitiesInfo {
            offset: response.bitand(0b0111_1111) as u8,
            num_steps: (response >> 8).bitand(0b0111_1111) as u8,
            step_size: (response >> 16).bitand(0b0111_1111) as u8,
            mute_capable: get_bit(response, 31),
        }
    }
}

impl TryFrom<Info> for AmpCapabilitiesInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::InputAmpCapabilities(info) => Ok(info),
            Info::OutputAmpCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct ConnectionListLengthInfo {
    connection_list_length: u8,
    long_form: bool,
}

impl ConnectionListLengthInfo {
    pub fn new(response: u32) -> Self {
        ConnectionListLengthInfo {
            connection_list_length: response.bitand(0b0111_1111) as u8,
            long_form: get_bit(response, 7),
        }
    }
}

impl TryFrom<Info> for ConnectionListLengthInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::ConnectionListLength(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct SupportedPowerStatesInfo {
    d0_sup: bool,
    d1_sup: bool,
    d2_sup: bool,
    d3_sup: bool,
    d3cold_sup: bool,
    s3d3cold_sup: bool,
    clkstop: bool,
    epss: bool,
}

impl SupportedPowerStatesInfo {
    pub fn new(response: u32) -> Self {
        SupportedPowerStatesInfo {
            d0_sup: get_bit(response, 0),
            d1_sup: get_bit(response, 1),
            d2_sup: get_bit(response, 2),
            d3_sup: get_bit(response, 3),
            d3cold_sup: get_bit(response, 4),
            s3d3cold_sup: get_bit(response, 29),
            clkstop: get_bit(response, 30),
            epss: get_bit(response, 31),
        }
    }
}

impl TryFrom<Info> for SupportedPowerStatesInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::SupportedPowerStates(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct ProcessingCapabilitiesInfo {
    benign: bool,
    num_coeff: u8,
}

impl ProcessingCapabilitiesInfo {
    pub fn new(response: u32) -> Self {
        ProcessingCapabilitiesInfo {
            benign: get_bit(response, 0),
            num_coeff: (response >> 8).bitand(0xFF) as u8,
        }
    }
}

impl TryFrom<Info> for ProcessingCapabilitiesInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::ProcessingCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct GPIOCountInfo {
    num_gpios: u8,
    num_gpos: u8,
    num_gpis: u8,
    gpi_unsol: bool,
    gpi_wake: bool,
}

impl GPIOCountInfo {
    pub fn new(response: u32) -> Self {
        GPIOCountInfo {
            num_gpios: response.bitand(0xFF) as u8,
            num_gpos: (response >> 8).bitand(0xFF) as u8,
            num_gpis: (response >> 16).bitand(0xFF) as u8,
            gpi_unsol: get_bit(response, 30),
            gpi_wake: get_bit(response, 31),
        }
    }
}

impl TryFrom<Info> for GPIOCountInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::GPIOCount(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct VolumeKnobCapabilitiesInfo {
    num_steps: u8,
    delta: bool,
}

impl VolumeKnobCapabilitiesInfo {
    pub fn new(response: u32) -> Self {
        VolumeKnobCapabilitiesInfo {
            num_steps: response.bitand(0b0111_1111) as u8,
            delta: get_bit(response, 7),
        }
    }
}

impl TryFrom<Info> for VolumeKnobCapabilitiesInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::VolumeKnobCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct ConnectionSelectInfo {
    currently_set_connection_index: u8,
}

impl ConnectionSelectInfo {
    pub fn new(response: u32) -> Self {
        ConnectionSelectInfo {
            currently_set_connection_index: response.bitand(0xFF) as u8,
        }
    }
}

impl TryFrom<Info> for ConnectionSelectInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::ConnectionSelect(info) => Ok(info),
            e => Err(e),
        }
    }
}


// temporarily only short form implemented (see section 7.3.3.3 of the specification)
#[derive(Debug, Getters)]
pub struct ConnectionListEntryInfo {
    connection_list_entry_at_offset_index: u8,
    connection_list_entry_at_offset_index_plus_one: u8,
    connection_list_entry_at_offset_index_plus_two: u8,
    connection_list_entry_at_offset_index_plus_three: u8,
}

impl ConnectionListEntryInfo {
    pub fn new(response: u32) -> Self {
        ConnectionListEntryInfo {
            connection_list_entry_at_offset_index: response.bitand(0xFF) as u8,
            connection_list_entry_at_offset_index_plus_one: (response >> 8).bitand(0xFF) as u8,
            connection_list_entry_at_offset_index_plus_two: (response >> 16).bitand(0xFF) as u8,
            connection_list_entry_at_offset_index_plus_three: (response >> 24).bitand(0xFF) as u8,
        }
    }
}

impl TryFrom<Info> for ConnectionListEntryInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::ConnectionListEntry(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct StreamFormatInfo {
    number_of_channels: u8,
    bits_per_sample: u8,
    sample_base_rate_divisor: u8,
    sample_base_rate_multiple: u8,
    sample_base_rate: u16,
    stream_type: StreamType,
}

impl StreamFormatInfo {
    pub fn new(response: u32) -> Self {
        let sample_base_rate_multiple = (response >> 11).bitand(0b111) as u8 + 1;
        if sample_base_rate_multiple > 4 {
            panic!("Unsupported sample rate base multiple, see table 53 in section 3.7.1: Stream Format Structure of the specification");
        }

        StreamFormatInfo {
            number_of_channels: (response.bitand(0xF) as u8) + 1,
            bits_per_sample: match (response >> 4).bitand(0b111) {
                0b000 => 8,
                0b001 => 16,
                0b010 => 20,
                0b011 => 24,
                0b100 => 32,
                // 0b101 to 0b111 reserved
                _ => panic!("Unsupported bit depth, see table 53 in section 3.7.1: Stream Format Structure of the specification")
            },
            sample_base_rate_divisor: (response >> 8).bitand(0b111) as u8 + 1,
            sample_base_rate_multiple,
            sample_base_rate: if get_bit(response, 14) { 44100 } else { 48000 },
            stream_type: if get_bit(response, 15) { StreamType::NonPCM } else { StreamType::PCM },
        }
    }

    pub fn as_u16(&self) -> u16 {
        let number_of_channels = self.number_of_channels - 1;
        let bits_per_sample = match self.bits_per_sample {
            8 => 0b000,
            16 => 0b001,
            20 => 0b010,
            24 => 0b011,
            32 => 0b100,
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
}

impl TryFrom<Info> for StreamFormatInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::StreamFormat(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct ChannelStreamIdInfo {
    channel: u8,
    stream: u8,
}

impl ChannelStreamIdInfo {
    pub fn new(response: u32) -> Self {
        ChannelStreamIdInfo {
            channel: response.bitand(0xF) as u8,
            stream: (response >> 4).bitand(0xF) as u8,
        }
    }

    pub fn as_u8(&self) -> u8 {
        (self.stream << 4) | self.channel
    }
}

impl TryFrom<Info> for ChannelStreamIdInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::ChannelStreamId(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug)]
pub enum StreamType {
    PCM,
    NonPCM,
}

#[derive(Debug, Getters)]
pub struct PinWidgetControlInfo {
    // Voltage Reference Enable applies only to non-digital pin widgets (see section 7.3.3.13 of the specification)
    // for digital pin widgets (e.g. HDMI and Display Port), the same bits represent Encoded Packet Type instead
    // but a case distinction is not implemented yet so this code will fail for digital pin widgets
    voltage_reference_enable: VoltageReferenceSignalLevel,
    in_enable: bool,
    out_enable: bool,
    h_phn_enable: bool,
}

impl PinWidgetControlInfo {
    pub fn new(response: u32) -> Self {
        PinWidgetControlInfo {
            voltage_reference_enable: match response.bitand(0b111) {
                0b000 => VoltageReferenceSignalLevel::HiZ,
                0b001 => VoltageReferenceSignalLevel::FiftyPercent,
                0b010 => VoltageReferenceSignalLevel::Ground0V,
                // 0b010 reserved
                0b100 => VoltageReferenceSignalLevel::EightyPercent,
                0b101 => VoltageReferenceSignalLevel::HundredPercent,
                // 0b110 and 0b111 reserved
                _ => panic!("Unsupported type of voltage reference signal level")
            },
            in_enable: get_bit(response, 5),
            out_enable: get_bit(response, 6),
            h_phn_enable: get_bit(response, 7),
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

impl TryFrom<Info> for PinWidgetControlInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::PinWidgetControl(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug)]
pub enum VoltageReferenceSignalLevel {
    HiZ,
    FiftyPercent,
    Ground0V,
    EightyPercent,
    HundredPercent,
}

#[derive(Debug, Getters)]
pub struct ConfigurationDefaultInfo {
    sequence: u8,
    default_association: u8,
    jack_detect_override: bool,
    color: ConfigDefColor,
    connection_type: ConfigDefConnectionType,
    default_device: ConfigDefDefaultDevice,
    geometric_location: ConfigDefGeometricLocation,
    gross_location: ConfigDefGrossLocation,
    port_connectivity: ConfigDefPortConnectivity,
}

impl ConfigurationDefaultInfo {
    pub fn new(response: u32) -> Self {
        let gross_location = match (response >> 28).bitand(0b11) {
            0b00 => ConfigDefGrossLocation::ExternalOnPrimaryChassis,
            0b01 => ConfigDefGrossLocation::Internal,
            0b10 => ConfigDefGrossLocation::SeparateChassis,
            0b11 => ConfigDefGrossLocation::Other,
            _ => panic!("This arm can never be reached as all cases are covered")
        };

        ConfigurationDefaultInfo {
            sequence: response.bitand(0xF) as u8,
            default_association: (response >> 4).bitand(0xF) as u8,
            jack_detect_override: get_bit(response, 8),
            color: match (response >> 12).bitand(0xF) {
                0x0 => ConfigDefColor::Unknown,
                0x1 => ConfigDefColor::Black,
                0x2 => ConfigDefColor::Grey,
                0x3 => ConfigDefColor::Blue,
                0x4 => ConfigDefColor::Green,
                0x5 => ConfigDefColor::Red,
                0x6 => ConfigDefColor::Orange,
                0x7 => ConfigDefColor::Yellow,
                0x8 => ConfigDefColor::Purple,
                0x9 => ConfigDefColor::Pink,
                // 0xA to 0xD are reserved
                0xE => ConfigDefColor::White,
                0xF => ConfigDefColor::Other,

                // I first threw a panic here but the pyhsical sound card in my testing device returned the reserved value 0xC...
                _ => ConfigDefColor::Unknown,
            },
            connection_type: match (response >> 16).bitand(0xF) {
                0x0 => ConfigDefConnectionType::Unknown,
                0x1 => ConfigDefConnectionType::EighthInchStereoMono,
                0x2 => ConfigDefConnectionType::QuarterInchStereoMono,
                0x3 => ConfigDefConnectionType::ATAPIInternal,
                0x4 => ConfigDefConnectionType::RCA,
                0x5 => ConfigDefConnectionType::Optical,
                0x6 => ConfigDefConnectionType::OtherDigital,
                0x7 => ConfigDefConnectionType::OtherAnalog,
                0x8 => ConfigDefConnectionType::MultichannelAnalogDIN,
                0x9 => ConfigDefConnectionType::XLRProfessional,
                0xA => ConfigDefConnectionType::RJ11Modem,
                0xB => ConfigDefConnectionType::Combination,
                // 0xC to 0xE are not defined in specification
                0xF => ConfigDefConnectionType::Other,
                _ => panic!("Unsupported connection type")
            },
            default_device: match (response >> 20).bitand(0xF) {
                0x0 => ConfigDefDefaultDevice::LineOut,
                0x1 => ConfigDefDefaultDevice::Speaker,
                0x2 => ConfigDefDefaultDevice::HPOut,
                0x3 => ConfigDefDefaultDevice::CD,
                0x4 => ConfigDefDefaultDevice::SPDIFOut,
                0x5 => ConfigDefDefaultDevice::DigitalOtherOut,
                0x6 => ConfigDefDefaultDevice::ModemLineSide,
                0x7 => ConfigDefDefaultDevice::ModemHandsetSide,
                0x8 => ConfigDefDefaultDevice::LineIn,
                0x9 => ConfigDefDefaultDevice::AUX,
                0xA => ConfigDefDefaultDevice::MicIn,
                0xB => ConfigDefDefaultDevice::Telephony,
                0xC => ConfigDefDefaultDevice::SPDIFIn,
                0xD => ConfigDefDefaultDevice::DigitalOtherIn,
                // 0xE is reserved
                0xF => ConfigDefDefaultDevice::Other,
                _ => panic!("Unsupported Type of Default Device")
            },
            geometric_location: match (response >> 24).bitand(0xF) {
                0x0 => ConfigDefGeometricLocation::NotAvailable,
                0x1 => ConfigDefGeometricLocation::Rear,
                0x2 => ConfigDefGeometricLocation::Front,
                0x3 => ConfigDefGeometricLocation::Left,
                0x4 => ConfigDefGeometricLocation::Right,
                0x5 => ConfigDefGeometricLocation::Top,
                0x6 => ConfigDefGeometricLocation::Bottom,
                0x7 => match gross_location {
                    ConfigDefGrossLocation::ExternalOnPrimaryChassis => ConfigDefGeometricLocation::RearPanel,
                    ConfigDefGrossLocation::Internal => ConfigDefGeometricLocation::Riser,
                    ConfigDefGrossLocation::Other => ConfigDefGeometricLocation::MobileLidInside,
                    _ => panic!("Unsupported type of geometric location")
                },
                0x8 => match gross_location {
                    ConfigDefGrossLocation::ExternalOnPrimaryChassis => ConfigDefGeometricLocation::DriveBay,
                    ConfigDefGrossLocation::Internal => ConfigDefGeometricLocation::DigitalDisplay,
                    ConfigDefGrossLocation::Other => ConfigDefGeometricLocation::MobileLidOutside,
                    _ => panic!("Unsupported type of geometric location")
                }
                0x9 => match gross_location {
                    ConfigDefGrossLocation::Internal => ConfigDefGeometricLocation::ATAPI,
                    _ => panic!("Unsupported type of geometric location")
                }
                _ => panic!("Unsupported type of geometric location")
            },
            gross_location,
            port_connectivity: match (response >> 30).bitand(0b11) {
                0b00 => ConfigDefPortConnectivity::Jack,
                0b01 => ConfigDefPortConnectivity::NoPhysicalConnection,
                0b10 => ConfigDefPortConnectivity::InternalDevice,
                0b11 => ConfigDefPortConnectivity::JackAndInternalDevice,
                _ => panic!("This arm can never be reached as all cases are covered")
            },
        }
    }
}

impl TryFrom<Info> for ConfigurationDefaultInfo {
    type Error = Info;

    fn try_from(info_wrapped: Info) -> Result<Self, Self::Error> {
        match info_wrapped {
            Info::ConfigurationDefault(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug)]
pub enum ConfigDefPortConnectivity {
    Jack,
    NoPhysicalConnection,
    InternalDevice,
    JackAndInternalDevice,
}

#[derive(Debug)]
pub enum ConfigDefGrossLocation {
    ExternalOnPrimaryChassis,
    Internal,
    SeparateChassis,
    Other,
}

#[derive(Debug)]
pub enum ConfigDefGeometricLocation {
    NotAvailable,
    Rear,
    Front,
    Left,
    Right,
    Top,
    Bottom,
    RearPanel,
    Riser,
    MobileLidInside,
    DriveBay,
    DigitalDisplay,
    MobileLidOutside,
    ATAPI,
    //Specials of table 110 in section 7.3.3.31 not implemented
}

#[derive(Debug)]
pub enum ConfigDefDefaultDevice {
    LineOut,
    Speaker,
    HPOut,
    CD,
    SPDIFOut,
    DigitalOtherOut,
    ModemLineSide,
    ModemHandsetSide,
    LineIn,
    AUX,
    MicIn,
    Telephony,
    SPDIFIn,
    DigitalOtherIn,
    Other,
}

#[derive(Debug)]
pub enum ConfigDefConnectionType {
    Unknown,
    EighthInchStereoMono,
    QuarterInchStereoMono,
    ATAPIInternal,
    RCA,
    Optical,
    OtherDigital,
    OtherAnalog,
    MultichannelAnalogDIN,
    XLRProfessional,
    RJ11Modem,
    Combination,
    Other,
}

#[derive(Debug)]
pub enum ConfigDefColor {
    Unknown,
    Black,
    Grey,
    Blue,
    Green,
    Red,
    Orange,
    Yellow,
    Purple,
    Pink,
    White,
    Other
}

fn get_bit<T: LowerHex + PrimInt>(input: T, index: usize) -> bool {
    let zero = T::from(0x0).expect("As only u8, u16 and u32 are used as types for T, this should never fail");
    let one = T::from(0x1).expect("As only u8, u16 and u32 are used as types for T, this should never fail");
    (input >> index).bitand(one) != zero
}
