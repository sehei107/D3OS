#![allow(dead_code)]

use alloc::vec::Vec;
use core::ops::BitAnd;
use derive_getters::Getters;

pub const MAX_AMOUNT_OF_CODECS: u8 = 15;
const MAX_AMOUNT_OF_AMPLIFIERS_IN_AMP_WIDGET: u8 = 16;
const MAX_AMPLIFIER_GAIN: u8 = u8::MAX;



// ############################################## IHDA commands ##############################################

#[derive(Clone, Copy, Debug, Getters)]
pub struct NodeAddress {
    codec_address: CodecAddress,
    node_id: u8,
}

impl NodeAddress {
    pub fn new(codec_address: CodecAddress, node_id: u8) -> Self {
        if codec_address.codec_address >= MAX_AMOUNT_OF_CODECS { panic!("IHDA only supports up to {} codecs!", MAX_AMOUNT_OF_CODECS) };
        NodeAddress {
            codec_address,
            node_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Getters)]
pub struct CodecAddress {
    codec_address: u8,
}

impl CodecAddress {
    pub fn new(codec_address: u8) -> Self {
        if codec_address >= MAX_AMOUNT_OF_CODECS { panic!("IHDA only supports up to {} codecs!", MAX_AMOUNT_OF_CODECS) };
        CodecAddress {
            codec_address,
        }
    }
}

#[derive(Debug, Getters)]
pub struct Codec {
    codec_address: CodecAddress,
    vendor_id: VendorIdResponse,
    revision_id: RevisionIdResponse,
    function_groups: Vec<FunctionGroup>
}

impl Codec {
    pub fn new(
        codec_address: CodecAddress,
        vendor_id: VendorIdResponse,
        revision_id: RevisionIdResponse,
        function_groups: Vec<FunctionGroup>
    ) -> Self {
        Codec {
            codec_address,
            vendor_id,
            revision_id,
            function_groups,
        }
    }
}

#[derive(Debug, Getters)]
pub struct FunctionGroup {
    function_group_node_address: NodeAddress,
    function_group_type: FunctionGroupTypeResponse,
    audio_function_group_caps: AudioFunctionGroupCapabilitiesResponse,
    sample_size_rate_caps: SampleSizeRateCAPsResponse,
    supported_stream_formats: SupportedStreamFormatsResponse,
    input_amp_caps: AmpCapabilitiesResponse,
    output_amp_caps: AmpCapabilitiesResponse,
    supported_power_states: SupportedPowerStatesResponse,
    gpio_count: GPIOCountResponse,
    widgets: Vec<Widget>,
}

impl FunctionGroup {
    pub fn new(
        function_group_node_address: NodeAddress,
        function_group_type: FunctionGroupTypeResponse,
        audio_function_group_caps: AudioFunctionGroupCapabilitiesResponse,
        sample_size_rate_caps: SampleSizeRateCAPsResponse,
        supported_stream_formats: SupportedStreamFormatsResponse,
        input_amp_caps: AmpCapabilitiesResponse,
        output_amp_caps: AmpCapabilitiesResponse,
        supported_power_states: SupportedPowerStatesResponse,
        gpio_count: GPIOCountResponse,
        widgets: Vec<Widget>
    ) -> Self {
        FunctionGroup {
            function_group_node_address,
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

    pub fn find_line_out_pin_widgets_connected_to_jack(&self) -> Vec<&Widget> {
        let mut pin_widgets_connected_to_jack = Vec::new();
        for widget in self.widgets().iter() {
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

    pub fn find_widget_path_for_line_out_playback(&self) -> Vec<&Widget> {
        let mut widgets_on_path = Vec::new();
        let mut widget = Some(*self.find_line_out_pin_widgets_connected_to_jack().get(0).unwrap());
        while widget.is_some() {
            widgets_on_path.push(widget.unwrap());
            widget = self.get_predecessor(widget.unwrap());
        }
        widgets_on_path
    }

    fn get_predecessor(&self, widget: &Widget) -> Option<&Widget> {
        let connection_list_entries = match widget.widget_info() {
            WidgetInfoContainer::AudioOutputConverter(_, _, _, _, _) => { None }
            WidgetInfoContainer::AudioInputConverter(_, _, _, _, _, _) => { None }
            WidgetInfoContainer::PinComplex(_, _, _, _, _, _, _, connection_list_entries) => { Some(connection_list_entries) }
            WidgetInfoContainer::Mixer(_, _, _, _, _, connection_list_entries) => { Some(connection_list_entries) }
            WidgetInfoContainer::Selector => { None }
            WidgetInfoContainer::Power => { None }
            WidgetInfoContainer::VolumeKnob => { None }
            WidgetInfoContainer::BeepGenerator => { None }
            WidgetInfoContainer::VendorDefined => { None }
        };

        if connection_list_entries.is_some() {
            let default_predecessor_node_id = *connection_list_entries.unwrap().first_entry();
            for widget in self.widgets().iter() {
                if *widget.address().node_id() == default_predecessor_node_id {
                    return Some(widget);
                }
            }
        }

        None
    }
}

#[derive(Debug, Getters)]
pub struct Widget {
    address: NodeAddress,
    audio_widget_capabilities: AudioWidgetCapabilitiesResponse,
    widget_info: WidgetInfoContainer,
}

impl Widget {
    pub fn new(
        address: NodeAddress,
        audio_widget_capabilities: AudioWidgetCapabilitiesResponse,
        widget_info: WidgetInfoContainer
    ) -> Self {
        Widget {
            address,
            audio_widget_capabilities,
            widget_info
        }
    }

    pub fn max_number_of_channels(&self) -> u8 {
        // this formula can be found in section 7.3.4.6, Audio Widget Capabilities of the specification
        (self.audio_widget_capabilities.chan_count_ext() << 1) + (*self.audio_widget_capabilities.chan_count_lsb() as u8) + 1u8
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
    Mixer(
        AmpCapabilitiesResponse,
        AmpCapabilitiesResponse,
        ConnectionListLengthResponse,
        SupportedPowerStatesResponse,
        ProcessingCapabilitiesResponse,
        ConnectionListEntryResponse,
    ),
    Selector,
    Power,
    VolumeKnob,
    BeepGenerator,
    VendorDefined,
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    GetParameter(NodeAddress, Parameter),
    GetConnectionSelect(NodeAddress),
    SetConnectionSelect(NodeAddress, SetConnectionSelectPayload),
    GetConnectionListEntry(NodeAddress, GetConnectionListEntryPayload),
    GetAmplifierGainMute(NodeAddress, GetAmplifierGainMutePayload),
    SetAmplifierGainMute(NodeAddress, SetAmplifierGainMutePayload),
    GetStreamFormat(NodeAddress),
    SetStreamFormat(NodeAddress, SetStreamFormatPayload),
    GetChannelStreamId(NodeAddress),
    SetChannelStreamId(NodeAddress, SetChannelStreamIdPayload),
    GetPinWidgetControl(NodeAddress),
    SetPinWidgetControl(NodeAddress, SetPinWidgetControlPayload),
    GetEAPDBTLEnable(NodeAddress),
    SetEAPDBTLEnable(NodeAddress, SetEAPDBTLEnablePayload),
    GetConfigurationDefault(NodeAddress),
    GetConverterChannelCount(NodeAddress),
    SetConverterChannelCount(NodeAddress, SetConverterChannelCountPayload),
}

impl Command {
    pub fn id(&self) -> u16 {
        match self {
            Command::GetParameter(..) => 0xF00,
            Command::GetConnectionSelect(..) => 0xF01,
            Command::SetConnectionSelect(..) => 0x701,
            Command::GetConnectionListEntry(..) => 0xF02,
            Command::GetAmplifierGainMute(..) => 0xB,
            Command::SetAmplifierGainMute(..) => 0x3,
            Command::GetStreamFormat(..) => 0xA,
            Command::SetStreamFormat(..) => 0x2,
            Command::GetChannelStreamId(..) => 0xF06,
            Command::SetChannelStreamId(..) => 0x706,
            Command::GetPinWidgetControl(..) => 0xF07,
            Command::SetPinWidgetControl(..) => 0x707,
            Command::GetEAPDBTLEnable(..) => 0xF0C,
            Command::SetEAPDBTLEnable(..) => 0x70C,
            Command::GetConfigurationDefault(..) => 0xF1C,
            Command::GetConverterChannelCount(..) => 0xF2D,
            Command::SetConverterChannelCount(..) => 0x72D,
        }
    }

    pub fn as_u32(&self) -> u32 {
        match self {
            Command::GetParameter(node_address, parameter) => Self::command_with_12bit_identifier_verb(node_address, self.id(), parameter.id()),
            Command::GetConnectionSelect(node_address) => Self::command_with_12bit_identifier_verb(node_address, self.id(), 0x0),
            Command::SetConnectionSelect(node_address, payload) => Self::command_with_12bit_identifier_verb(node_address, self.id(), payload.as_u8()),
            Command::GetConnectionListEntry(node_address, payload) => Self::command_with_12bit_identifier_verb(node_address, self.id(), payload.as_u8()),
            Command::GetAmplifierGainMute(node_address, payload) => Self::command_with_4bit_identifier_verb(node_address, self.id(), payload.as_u16()),
            Command::SetAmplifierGainMute(node_address, payload) => Self::command_with_4bit_identifier_verb(node_address, self.id(), payload.as_u16()),
            Command::GetStreamFormat(node_address) => Self::command_with_4bit_identifier_verb(node_address, self.id(), 0x0),
            Command::SetStreamFormat(node_address, payload) => Self::command_with_4bit_identifier_verb(node_address, self.id(), payload.as_u16()),
            Command::GetChannelStreamId(node_address) => Self::command_with_12bit_identifier_verb(node_address, self.id(), 0x0),
            Command::SetChannelStreamId(node_address, payload) => Self::command_with_12bit_identifier_verb(node_address, self.id(), payload.as_u8()),
            Command::GetPinWidgetControl(node_address) => Self::command_with_12bit_identifier_verb(node_address, self.id(), 0x0),
            Command::SetPinWidgetControl(node_address, payload) => Self::command_with_12bit_identifier_verb(node_address, self.id(), payload.as_u8()),
            Command::GetEAPDBTLEnable(node_address) => Self::command_with_12bit_identifier_verb(node_address, self.id(), 0x0),
            Command::SetEAPDBTLEnable(node_address, payload) => Self::command_with_12bit_identifier_verb(node_address, self.id(), payload.as_u8()),
            Command::GetConfigurationDefault(node_address) => Self::command_with_12bit_identifier_verb(node_address, self.id(), 0x0),
            Command::GetConverterChannelCount(node_address) => Self::command_with_12bit_identifier_verb(node_address, self.id(), 0x0),
            Command::SetConverterChannelCount(node_address, payload) => Self::command_with_12bit_identifier_verb(node_address, self.id(), payload.as_u8()),
        }
    }

    fn command_with_12bit_identifier_verb(node_address: &NodeAddress, verb_id: u16, payload: u8) -> u32 {
        (node_address.codec_address().codec_address as u32) << 28
            | (*node_address.node_id() as u32) << 20
            | (verb_id as u32) << 8
            | payload as u32
    }

    fn command_with_4bit_identifier_verb(node_address: &NodeAddress, verb_id: u16, payload: u16) -> u32 {
        (node_address.codec_address().codec_address as u32) << 28
            | (*node_address.node_id() as u32) << 20
            | (verb_id as u32) << 16
            | payload as u32
    }
}

// compare to table 140 in section 7.3.6 of the specification
#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug)]
pub struct SetConnectionSelectPayload {
    connection_index: u8,
}

impl SetConnectionSelectPayload {
    pub fn new(connection_index: u8) -> Self {
        Self {
            connection_index,
        }
    }

    pub fn as_u8(&self) -> u8 {
        self.connection_index
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GetConnectionListEntryPayload {
    offset: u8,
}

impl GetConnectionListEntryPayload {
    pub fn new(offset: u8) -> Self {
        Self {
            offset,
        }
    }

    pub fn as_u8(&self) -> u8 {
        self.offset
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GetAmplifierGainMutePayload {
    amp_type: GetAmplifierGainMuteType,
    side: GetAmplifierGainMuteSide,
    index: u8,
}

impl GetAmplifierGainMutePayload {
    pub fn new(amp_type: GetAmplifierGainMuteType, side: GetAmplifierGainMuteSide, index: u8) -> Self {
        if index > MAX_AMOUNT_OF_AMPLIFIERS_IN_AMP_WIDGET { panic!("Index for amplifier out of range") };
        Self {
            amp_type,
            side,
            index,
        }
    }

    fn as_u16(&self) -> u16 {
        let amp_type: u16 = match self.amp_type  {
            GetAmplifierGainMuteType::Input => 0,
            GetAmplifierGainMuteType::Output => 1,
        };
        let side: u16 = match self.side  {
            GetAmplifierGainMuteSide::Right => 0,
            GetAmplifierGainMuteSide::Left => 1,
        };

        amp_type << 15 | side << 13 | self.index as u16
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SetAmplifierGainMutePayload {
    amp_type: SetAmplifierGainMuteType,
    side: SetAmplifierGainMuteSide,
    index: u8,
    mute: bool,
    gain: u8,
}

impl SetAmplifierGainMutePayload {
    pub fn new(amp_type: SetAmplifierGainMuteType, side: SetAmplifierGainMuteSide, index: u8, mute: bool, gain: u8) -> Self {
        if gain > MAX_AMPLIFIER_GAIN { panic!("gain is a 7 bit parameter, writing 8 bit values will leak into mute bit and are therefore prohibited") }
        if index > MAX_AMOUNT_OF_AMPLIFIERS_IN_AMP_WIDGET { panic!("Index for amplifier out of range") }
        Self {
            amp_type,
            side,
            index,
            mute,
            gain,
        }
    }

    fn as_u16(&self) -> u16 {
        let amp_type: u16 = match self.amp_type  {
            SetAmplifierGainMuteType::Input => 0b01,
            SetAmplifierGainMuteType::Output => 0b10,
            SetAmplifierGainMuteType::Both => 0b11,
        };
        let side: u16 = match self.side  {
            SetAmplifierGainMuteSide::Right => 0b01,
            SetAmplifierGainMuteSide::Left => 0b10,
            SetAmplifierGainMuteSide::Both => 0b11,
        };

        amp_type << 14 | side << 12 | (self.index as u16) << 8 | (self.mute as u16) << 7 | self.gain as u16
    }
}

#[derive(Clone, Copy, Debug)]
pub enum GetAmplifierGainMuteType {
    Input,
    Output,
}

#[derive(Clone, Copy, Debug)]
pub enum GetAmplifierGainMuteSide {
    Right,
    Left,
}

#[derive(Clone, Copy, Debug)]
pub enum SetAmplifierGainMuteType {
    Input,
    Output,
    Both,
}

#[derive(Clone, Copy, Debug)]
pub enum SetAmplifierGainMuteSide {
    Right,
    Left,
    Both,
}


#[derive(Clone, Copy, Debug, Getters)]
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

    fn as_u16(&self) -> u16 {
        let number_of_channels = self.number_of_channels - 1;
        let bits_per_sample = match self.bits_per_sample {
            BitsPerSample::Eight => 0b000,
            BitsPerSample::Sixteen => 0b001,
            BitsPerSample::Twenty => 0b010,
            BitsPerSample::Twentyfour => 0b011,
            BitsPerSample::Thirtytwo => 0b100,
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

#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug)]
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

    pub fn enable_input_and_output_amps(pin_widget_control_response: PinWidgetControlResponse) -> Self {
       Self::new(
            match pin_widget_control_response.voltage_reference_enable() {
                VoltageReferenceSignalLevel::HiZ => VoltageReferenceSignalLevel::HiZ,
                VoltageReferenceSignalLevel::FiftyPercent => VoltageReferenceSignalLevel::FiftyPercent,
                VoltageReferenceSignalLevel::Ground0V => VoltageReferenceSignalLevel::Ground0V,
                VoltageReferenceSignalLevel::EightyPercent => VoltageReferenceSignalLevel::EightyPercent,
                VoltageReferenceSignalLevel::HundredPercent => VoltageReferenceSignalLevel::HundredPercent,
            },
            true,
            true,
            *pin_widget_control_response.h_phn_enable()
        )
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

#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug)]
pub struct SetConverterChannelCountPayload {
    converter_channel_count: u8,
}

impl SetConverterChannelCountPayload {
    pub fn new(converter_channel_count: u8) -> Self {
        Self {
            converter_channel_count,
        }
    }

    pub fn as_u8(&self) -> u8 {
        self.converter_channel_count
    }
}



// ############################################## IHDA responses ##############################################

pub struct RawResponse {
    raw_value: u32,
}

impl RawResponse {
    pub fn new(response: u32) -> Self {
        Self {
            raw_value: response,
        }
    }

    fn get_bit(&self, index: usize) -> bool {
        (self.raw_value >> index).bitand(1) != 0
    }
}

#[derive(Debug)]
pub enum Response {
    VendorId(VendorIdResponse),
    RevisionId(RevisionIdResponse),
    SubordinateNodeCount(SubordinateNodeCountResponse),
    FunctionGroupType(FunctionGroupTypeResponse),
    AudioFunctionGroupCapabilities(AudioFunctionGroupCapabilitiesResponse),
    AudioWidgetCapabilities(AudioWidgetCapabilitiesResponse),
    SampleSizeRateCAPs(SampleSizeRateCAPsResponse),
    SupportedStreamFormats(SupportedStreamFormatsResponse),
    PinCapabilities(PinCapabilitiesResponse),
    InputAmpCapabilities(AmpCapabilitiesResponse),
    OutputAmpCapabilities(AmpCapabilitiesResponse),
    ConnectionListLength(ConnectionListLengthResponse),
    SupportedPowerStates(SupportedPowerStatesResponse),
    ProcessingCapabilities(ProcessingCapabilitiesResponse),
    GPIOCount(GPIOCountResponse),
    VolumeKnobCapabilities(VolumeKnobCapabilitiesResponse),

    ConnectionSelect(ConnectionSelectResponse),
    ConnectionListEntry(ConnectionListEntryResponse),
    AmplifierGainMute(AmplifierGainMuteResponse),
    ChannelStreamId(ChannelStreamIdResponse),
    StreamFormat(StreamFormatResponse),
    PinWidgetControl(PinWidgetControlResponse),
    EAPDBTLEnable(EAPDBTLEnableResponse),
    ConfigurationDefault(ConfigurationDefaultResponse),
    ConverterChannelCount(ConverterChannelCountResponse),
    Zeros,
}

impl Response {
    pub fn new(response: RawResponse, associated_command: Command) -> Response {
        match associated_command {
            Command::GetParameter(_, parameter) => {
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
            Command::GetConnectionSelect(..) => Response::ConnectionSelect(ConnectionSelectResponse::new(response)),
            Command::SetConnectionSelect(..) => Response::Zeros,
            Command::GetConnectionListEntry(..) => Response::ConnectionListEntry(ConnectionListEntryResponse::new(response)),
            Command::GetAmplifierGainMute(..) => Response::AmplifierGainMute(AmplifierGainMuteResponse::new(response)),
            Command::SetAmplifierGainMute(..) => Response::Zeros,
            Command::GetStreamFormat(..) => Response::StreamFormat(StreamFormatResponse::new(response)),
            Command::SetStreamFormat(..) => Response::Zeros,
            Command::GetChannelStreamId(..) => Response::ChannelStreamId(ChannelStreamIdResponse::new(response)),
            Command::SetChannelStreamId(..) => Response::Zeros,
            Command::GetPinWidgetControl(..) => Response::PinWidgetControl(PinWidgetControlResponse::new(response)),
            Command::SetPinWidgetControl(..) => Response::Zeros,
            Command::GetEAPDBTLEnable(..) => Response::EAPDBTLEnable(EAPDBTLEnableResponse::new(response)),
            Command::SetEAPDBTLEnable(..) => Response::Zeros,
            Command::GetConfigurationDefault(..) => Response::ConfigurationDefault(ConfigurationDefaultResponse::new(response)),
            Command::GetConverterChannelCount(..) => Response::ConverterChannelCount(ConverterChannelCountResponse::new(response)),
            Command::SetConverterChannelCount(..) => Response::Zeros,
        }
    }
}

#[derive(Debug, Getters)]
pub struct VendorIdResponse {
    device_id: u16,
    vendor_id: u16,
}

impl VendorIdResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            device_id: response.raw_value.bitand(0xFFFF) as u16,
            vendor_id: (response.raw_value >> 16).bitand(0xFFFF) as u16,
        }

    }
}

impl TryFrom<Response> for VendorIdResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
                    Response::VendorId(info) => Ok(info),
                    e => Err(e),
                }
    }
}

#[derive(Debug, Getters)]
pub struct RevisionIdResponse {
    stepping_id: u8,
    revision_id: u8,
    minor_revision: u8,
    major_revision: u8,
}

impl RevisionIdResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            stepping_id: response.raw_value.bitand(0xFF) as u8,
            revision_id: (response.raw_value >> 8).bitand(0xFF) as u8,
            minor_revision: (response.raw_value >> 16).bitand(0xF) as u8,
            major_revision: (response.raw_value >> 20).bitand(0xF) as u8,
        }
    }
}

impl TryFrom<Response> for RevisionIdResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::RevisionId(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct SubordinateNodeCountResponse {
    total_number_of_nodes: u8,
    starting_node_number: u8,
}

impl SubordinateNodeCountResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            total_number_of_nodes: response.raw_value.bitand(0xFF) as u8,
            starting_node_number: (response.raw_value >> 16).bitand(0xFF) as u8,
        }

    }
}

impl TryFrom<Response> for SubordinateNodeCountResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::SubordinateNodeCount(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct FunctionGroupTypeResponse {
    node_type: FunctionGroupTypeEnum,
    unsolicited_response_capable: bool,
}

impl FunctionGroupTypeResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            node_type: match response.raw_value.bitand(0xFF) as u8 {
                0x1 => FunctionGroupTypeEnum::AudioFunctionGroup,
                0x2 => FunctionGroupTypeEnum::VendorDefinedFunctionGroup,
                0x80..=0xFF => FunctionGroupTypeEnum::VendorDefinedModemFunctionGroup,
                _ => panic!("Unknown function group node type!")
            },
            unsolicited_response_capable: response.get_bit(8),
        }

    }
}

impl TryFrom<Response> for FunctionGroupTypeResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::FunctionGroupType(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug)]
pub enum FunctionGroupTypeEnum {
    AudioFunctionGroup,
    VendorDefinedModemFunctionGroup,
    VendorDefinedFunctionGroup,
}

#[derive(Debug, Getters)]
pub struct AudioFunctionGroupCapabilitiesResponse {
    output_delay: u8,
    input_delay: u8,
    beep_gen: bool,
}

impl AudioFunctionGroupCapabilitiesResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            output_delay: response.raw_value.bitand(0xF) as u8,
            input_delay: (response.raw_value >> 8).bitand(0xF) as u8,
            beep_gen: response.get_bit(16),
        }
    }
}

impl TryFrom<Response> for AudioFunctionGroupCapabilitiesResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::AudioFunctionGroupCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct AudioWidgetCapabilitiesResponse {
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

impl AudioWidgetCapabilitiesResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            chan_count_lsb: response.get_bit(0),
            in_amp_present: response.get_bit(1),
            out_amp_present: response.get_bit(2),
            amp_param_override: response.get_bit(3),
            format_override: response.get_bit(4),
            stripe: response.get_bit(5),
            proc_widget: response.get_bit(6),
            unsol_capable: response.get_bit(7),
            conn_list: response.get_bit(8),
            digital: response.get_bit(9),
            power_cntrl: response.get_bit(10),
            lr_swap: response.get_bit(11),
            cp_caps: response.get_bit(12),
            chan_count_ext: (response.raw_value >> 13).bitand(0b111) as u8,
            delay: (response.raw_value >> 16).bitand(0xF) as u8,
            widget_type: match (response.raw_value >> 20).bitand(0xF) as u8 {
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

impl TryFrom<Response> for AudioWidgetCapabilitiesResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::AudioWidgetCapabilities(info) => Ok(info),
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
pub struct SampleSizeRateCAPsResponse {
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

impl SampleSizeRateCAPsResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            support_8000hz: response.get_bit(0),
            support_11025hz: response.get_bit(1),
            support_16000hz: response.get_bit(2),
            support_22050hz: response.get_bit(3),
            support_32000hz: response.get_bit(4),
            support_44100hz: response.get_bit(5),
            support_48000hz: response.get_bit(6),
            support_88200hz: response.get_bit(7),
            support_96000hz: response.get_bit(8),
            support_176400hz: response.get_bit(9),
            support_192000hz: response.get_bit(10),
            support_384000hz: response.get_bit(11),
            support_8bit: response.get_bit(16),
            support_16bit: response.get_bit(17),
            support_20bit: response.get_bit(18),
            support_24bit: response.get_bit(19),
            support_32bit: response.get_bit(20),
        }
    }
}

impl TryFrom<Response> for SampleSizeRateCAPsResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::SampleSizeRateCAPs(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct SupportedStreamFormatsResponse {
    pcm: bool,
    float32: bool,
    ac3: bool,
}

impl SupportedStreamFormatsResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            pcm: response.get_bit(0),
            float32: response.get_bit(1),
            ac3: response.get_bit(2),
        }
    }
}

impl TryFrom<Response> for SupportedStreamFormatsResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::SupportedStreamFormats(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct PinCapabilitiesResponse {
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

impl PinCapabilitiesResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            impedence_sense_capable: response.get_bit(0),
            trigger_required: response.get_bit(1),
            presence_detect_capable: response.get_bit(2),
            headphone_drive_capable: response.get_bit(3),
            output_capable: response.get_bit(4),
            input_capable: response.get_bit(5),
            balanced_io_pins: response.get_bit(6),
            hdmi: response.get_bit(7),
            vref_control: (response.raw_value >> 8).bitand(0xFF) as u8,
            eapd_capable: response.get_bit(16),
            display_port: response.get_bit(24),
            high_bit_rate: response.get_bit(27),
        }
    }
}

impl TryFrom<Response> for PinCapabilitiesResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::PinCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct AmpCapabilitiesResponse {
    offset: u8,
    num_steps: u8,
    step_size: u8,
    mute_capable: bool,
}

impl AmpCapabilitiesResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            offset: response.raw_value.bitand(0b0111_1111) as u8,
            num_steps: (response.raw_value >> 8).bitand(0b0111_1111) as u8,
            step_size: (response.raw_value >> 16).bitand(0b0111_1111) as u8,
            mute_capable: response.get_bit(31),
        }
    }
}

impl TryFrom<Response> for AmpCapabilitiesResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::InputAmpCapabilities(info) => Ok(info),
            Response::OutputAmpCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct ConnectionListLengthResponse {
    connection_list_length: u8,
    long_form: bool,
}

impl ConnectionListLengthResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            connection_list_length: response.raw_value.bitand(0b0111_1111) as u8,
            long_form: response.get_bit(7),
        }
    }
}

impl TryFrom<Response> for ConnectionListLengthResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::ConnectionListLength(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct SupportedPowerStatesResponse {
    d0_sup: bool,
    d1_sup: bool,
    d2_sup: bool,
    d3_sup: bool,
    d3cold_sup: bool,
    s3d3cold_sup: bool,
    clkstop: bool,
    epss: bool,
}

impl SupportedPowerStatesResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            d0_sup: response.get_bit(0),
            d1_sup: response.get_bit(1),
            d2_sup: response.get_bit(2),
            d3_sup: response.get_bit(3),
            d3cold_sup: response.get_bit(4),
            s3d3cold_sup: response.get_bit(29),
            clkstop: response.get_bit(30),
            epss: response.get_bit(31),
        }
    }
}

impl TryFrom<Response> for SupportedPowerStatesResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::SupportedPowerStates(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct ProcessingCapabilitiesResponse {
    benign: bool,
    num_coeff: u8,
}

impl ProcessingCapabilitiesResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            benign: response.get_bit(0),
            num_coeff: (response.raw_value >> 8).bitand(0xFF) as u8,
        }
    }
}

impl TryFrom<Response> for ProcessingCapabilitiesResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::ProcessingCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct GPIOCountResponse {
    num_gpios: u8,
    num_gpos: u8,
    num_gpis: u8,
    gpi_unsol: bool,
    gpi_wake: bool,
}

impl GPIOCountResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            num_gpios: response.raw_value.bitand(0xFF) as u8,
            num_gpos: (response.raw_value >> 8).bitand(0xFF) as u8,
            num_gpis: (response.raw_value >> 16).bitand(0xFF) as u8,
            gpi_unsol: response.get_bit(30),
            gpi_wake: response.get_bit(31),
        }
    }
}

impl TryFrom<Response> for GPIOCountResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::GPIOCount(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct VolumeKnobCapabilitiesResponse {
    num_steps: u8,
    delta: bool,
}

impl VolumeKnobCapabilitiesResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            num_steps: response.raw_value.bitand(0b0111_1111) as u8,
            delta: response.get_bit(7),
        }
    }
}

impl TryFrom<Response> for VolumeKnobCapabilitiesResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::VolumeKnobCapabilities(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct ConnectionSelectResponse {
    currently_set_connection_index: u8,
}

impl ConnectionSelectResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            currently_set_connection_index: response.raw_value.bitand(0xFF) as u8,
        }
    }
}

impl TryFrom<Response> for ConnectionSelectResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::ConnectionSelect(info) => Ok(info),
            e => Err(e),
        }
    }
}


// temporarily only short form implemented (see section 7.3.3.3 of the specification)
#[derive(Debug, Getters)]
pub struct ConnectionListEntryResponse {
    first_entry: u8,
    second_entry: u8,
    third_entry: u8,
    fourth_entry: u8,
}

impl ConnectionListEntryResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            first_entry: response.raw_value.bitand(0xFF) as u8,
            second_entry: (response.raw_value >> 8).bitand(0xFF) as u8,
            third_entry: (response.raw_value >> 16).bitand(0xFF) as u8,
            fourth_entry: (response.raw_value >> 24).bitand(0xFF) as u8,
        }
    }
}

impl TryFrom<Response> for ConnectionListEntryResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::ConnectionListEntry(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct AmplifierGainMuteResponse {
    amplifier_gain: u8,
    amplifier_mute: bool,
}

impl AmplifierGainMuteResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            amplifier_gain: (response.raw_value & 0b0111_1111) as u8,
            amplifier_mute: response.get_bit(7),
        }
    }
}

impl TryFrom<Response> for AmplifierGainMuteResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::AmplifierGainMute(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct StreamFormatResponse {
    number_of_channels: u8,
    bits_per_sample: BitsPerSample,
    sample_base_rate_divisor: u8,
    sample_base_rate_multiple: u8,
    sample_base_rate: u16,
    stream_type: StreamType,
}

impl StreamFormatResponse {
    pub fn new(response: RawResponse) -> Self {
        let sample_base_rate_multiple = (response.raw_value >> 11).bitand(0b111) as u8 + 1;
        if sample_base_rate_multiple > 4 {
            panic!("Unsupported sample rate base multiple, see table 53 in section 3.7.1: Stream Format Structure of the specification");
        }
        let number_of_channels = (response.raw_value.bitand(0xF) as u8) + 1;
        let bits_per_sample = match (response.raw_value >> 4).bitand(0b111) {
            0b000 => BitsPerSample::Eight,
            0b001 => BitsPerSample::Sixteen,
            0b010 => BitsPerSample::Twenty,
            0b011 => BitsPerSample::Twentyfour,
            0b100 => BitsPerSample::Thirtytwo,
            // 0b101 to 0b111 reserved
            _ => panic!("Unsupported bit depth, see table 53 in section 3.7.1: Stream Format Structure of the specification")
        };
        let sample_base_rate_divisor = (response.raw_value >> 8).bitand(0b111) as u8 + 1;
        let sample_base_rate = if response.get_bit(14) { 44100 } else { 48000 };
        let stream_type = if response.get_bit(15) { StreamType::NonPCM } else { StreamType::PCM };

        Self {
            number_of_channels,
            bits_per_sample,
            sample_base_rate_divisor,
            sample_base_rate_multiple,
            sample_base_rate,
            stream_type
        }
    }
}

impl TryFrom<Response> for StreamFormatResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::StreamFormat(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BitsPerSample {
    Eight,
    Sixteen,
    Twenty,
    Twentyfour,
    Thirtytwo,
}

#[derive(Clone, Copy, Debug)]
pub enum StreamType {
    PCM,
    NonPCM,
}

#[derive(Debug, Getters)]
pub struct ChannelStreamIdResponse {
    channel: u8,
    stream: u8,
}

impl ChannelStreamIdResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            channel: response.raw_value.bitand(0xF) as u8,
            stream: (response.raw_value >> 4).bitand(0xF) as u8,
        }
    }
}

impl TryFrom<Response> for ChannelStreamIdResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::ChannelStreamId(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct PinWidgetControlResponse {
    // Voltage Reference Enable applies only to non-digital pin widgets (see section 7.3.3.13 of the specification)
    // for digital pin widgets (e.g. HDMI and Display Port), the same bits represent Encoded Packet Type instead
    // but a case distinction is not implemented yet so this code will fail for digital pin widgets
    voltage_reference_enable: VoltageReferenceSignalLevel,
    in_enable: bool,
    out_enable: bool,
    h_phn_enable: bool,
}

impl PinWidgetControlResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            voltage_reference_enable: match response.raw_value.bitand(0b111) {
                0b000 => VoltageReferenceSignalLevel::HiZ,
                0b001 => VoltageReferenceSignalLevel::FiftyPercent,
                0b010 => VoltageReferenceSignalLevel::Ground0V,
                // 0b010 reserved
                0b100 => VoltageReferenceSignalLevel::EightyPercent,
                0b101 => VoltageReferenceSignalLevel::HundredPercent,
                // 0b110 and 0b111 reserved
                _ => panic!("Unsupported type of voltage reference signal level")
            },
            in_enable: response.get_bit(5),
            out_enable: response.get_bit(6),
            h_phn_enable: response.get_bit(7),
        }
    }
}

impl TryFrom<Response> for PinWidgetControlResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::PinWidgetControl(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum VoltageReferenceSignalLevel {
    HiZ,
    FiftyPercent,
    Ground0V,
    EightyPercent,
    HundredPercent,
}

#[derive(Debug, Getters)]
pub struct EAPDBTLEnableResponse {
    btl_enable: bool,
    eapd_enable: bool,
    lr_swap: bool,
}

impl EAPDBTLEnableResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            btl_enable: response.get_bit(0),
            eapd_enable: response.get_bit(1),
            lr_swap: response.get_bit(2),
        }
    }
}

impl TryFrom<Response> for EAPDBTLEnableResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::EAPDBTLEnable(info) => Ok(info),
            e => Err(e),
        }
    }
}

#[derive(Debug, Getters)]
pub struct ConfigurationDefaultResponse {
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

impl ConfigurationDefaultResponse {
    pub fn new(response: RawResponse) -> Self {
        let gross_location = match (response.raw_value >> 28).bitand(0b11) {
            0b00 => ConfigDefGrossLocation::ExternalOnPrimaryChassis,
            0b01 => ConfigDefGrossLocation::Internal,
            0b10 => ConfigDefGrossLocation::SeparateChassis,
            0b11 => ConfigDefGrossLocation::Other,
            _ => panic!("This arm can never be reached as all cases are covered")
        };

        Self {
            sequence: response.raw_value.bitand(0xF) as u8,
            default_association: (response.raw_value >> 4).bitand(0xF) as u8,
            jack_detect_override: response.get_bit(8),
            color: match (response.raw_value >> 12).bitand(0xF) {
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
            connection_type: match (response.raw_value >> 16).bitand(0xF) {
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
            default_device: match (response.raw_value >> 20).bitand(0xF) {
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
            geometric_location: match (response.raw_value >> 24).bitand(0xF) {
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
            port_connectivity: match (response.raw_value >> 30).bitand(0b11) {
                0b00 => ConfigDefPortConnectivity::Jack,
                0b01 => ConfigDefPortConnectivity::NoPhysicalConnection,
                0b10 => ConfigDefPortConnectivity::InternalDevice,
                0b11 => ConfigDefPortConnectivity::JackAndInternalDevice,
                _ => panic!("This arm can never be reached as all cases are covered")
            },
        }
    }
}

impl TryFrom<Response> for ConfigurationDefaultResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::ConfigurationDefault(info) => Ok(info),
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

#[derive(Debug, Getters)]
pub struct ConverterChannelCountResponse {
    converter_channel_count: u8,
}

impl ConverterChannelCountResponse {
    pub fn new(response: RawResponse) -> Self {
        Self {
            converter_channel_count: response.raw_value.bitand(0xFF) as u8,
        }
    }
}

impl TryFrom<Response> for ConverterChannelCountResponse {
    type Error = Response;

    fn try_from(wrapped_response: Response) -> Result<Self, Self::Error> {
        match wrapped_response {
            Response::ConverterChannelCount(info) => Ok(info),
            e => Err(e),
        }
    }
}
