#![allow(dead_code)]

use alloc::vec::Vec;
use core::fmt::LowerHex;
use core::ptr::NonNull;
use log::debug;
use num_traits::int::PrimInt;
use derive_getters::Getters;
use volatile::{VolatilePtr};
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::{Page, PageTableFlags, PhysFrame};
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::device::pit::Timer;
use crate::{memory, process_manager, timer};
use crate::device::ihda_codec::{AmpCapabilitiesResponse, AudioFunctionGroupCapabilitiesResponse, AudioWidgetCapabilitiesResponse, Codec, Command, ConfigurationDefaultResponse, ConnectionListEntryResponse, ConnectionListLengthResponse, FunctionGroup, FunctionGroupTypeResponse, GetConnectionListEntryPayload, GPIOCountResponse, MAX_AMOUNT_OF_CODECS, NodeAddress, PinCapabilitiesResponse, PinWidgetControlResponse, ProcessingCapabilitiesResponse, RawResponse, Response, RevisionIdResponse, SampleSizeRateCAPsResponse, SetAmplifierGainMutePayload, SetAmplifierGainMuteSide, SetAmplifierGainMuteType, SetChannelStreamIdPayload, SetPinWidgetControlPayload, SetStreamFormatPayload, SubordinateNodeCountResponse, SupportedPowerStatesResponse, SupportedStreamFormatsResponse, VendorIdResponse, WidgetInfoContainer, Widget, WidgetType, StreamFormat};
use crate::device::ihda_codec::Command::{GetConfigurationDefault, GetConnectionListEntry, GetParameter, GetPinWidgetControl, SetAmplifierGainMute, SetChannelStreamId, SetPinWidgetControl, SetStreamFormat};
use crate::device::ihda_codec::Parameter::{AudioFunctionGroupCapabilities, AudioWidgetCapabilities, ConnectionListLength, FunctionGroupType, GPIOCount, InputAmpCapabilities, OutputAmpCapabilities, PinCapabilities, ProcessingCapabilities, RevisionId, SampleSizeRateCAPs, SubordinateNodeCount, SupportedPowerStates, SupportedStreamFormats, VendorId};
use crate::memory::PAGE_SIZE;

const SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES: u64 = 0x20;
const OFFSET_OF_FIRST_SOUND_DESCRIPTOR: u64 = 0x80;
const MAX_AMOUNT_OF_BIDIRECTIONAL_STREAMS: u8 = 30;
const MAX_AMOUNT_OF_SDIN_SIGNALS: u8 = 15;
const MAX_AMOUNT_OF_CHANNELS_PER_STREAM: u8 = 16;
// TIMEOUT values arbitrarily chosen
const BIT_ASSERTION_TIMEOUT_IN_MS: usize = 10000;
const IMMEDIATE_COMMAND_TIMEOUT_IN_MS: usize = 100;
const BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BYTES: u64 = 16;
const MAX_AMOUNT_OF_BUFFER_DESCRIPTOR_LIST_ENTRIES: u64 = 256;
const DMA_POSITION_IN_BUFFER_ENTRY_SIZE_IN_BYTES: u64 = 4;
const CONTAINER_8BIT_SIZE_IN_BYTES: u32 = 1;
const CONTAINER_16BIT_SIZE_IN_BYTES: u32 = 2;
const CONTAINER_32BIT_SIZE_IN_BYTES: u32 = 4;



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
    // The register SDFIFOW is only defined in 8-series-chipset-pch-datasheet.pdf for the chipset on the used testing device.
    // As the IHDA specification doesn't mention this register at all, it might not exist for other IHDA sound cards.
    sdfifow: Register<u16>,
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
            sdfifow: Register::new((sd_base_address + 0xE) as *mut u16, "SDFIFOW"),
            // bytes with offset 0x8E to 0x8F are reserved
            sdfifod: Register::new((sd_base_address + 0x10) as *mut u16, "SDFIFOD"),
            sdfmt: Register::new((sd_base_address + 0x12) as *mut u16, "SDFMT"),
            // bytes with offset 0x94 to 0x97 are reserved
            sdbdpl: Register::new((sd_base_address + 0x18) as *mut u32, "SDDPL"),
            sdbdpu: Register::new((sd_base_address + 0x1C) as *mut u32, "SDDPU"),
        }
    }

    // ########## SDCTL ##########
    pub fn reset_stream(&self) {
        self.clear_stream_run_bit();

        self.sdctl.set_bit(0);
        let mut start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        while !self.sdctl.assert_bit(0) {
            if timer().read().systime_ms() > start_timer + BIT_ASSERTION_TIMEOUT_IN_MS {
                panic!("stream reset timed out after setting SRST bit")
            }
        }

        self.sdctl.clear_bit(0);
        start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        while self.sdctl.assert_bit(0) {
            if timer().read().systime_ms() > start_timer + BIT_ASSERTION_TIMEOUT_IN_MS {
                panic!("stream reset timed out after clearing SRST bit")
            }
        }
    }

    pub fn assert_stream_run_bit(&self) -> bool {
        self.sdctl.assert_bit(1)
    }

    pub fn set_stream_run_bit(&self) {
        self.sdctl.set_bit(1);
    }

    pub fn clear_stream_run_bit(&self) {
        self.sdctl.clear_bit(1);
    }

    pub fn assert_interrupt_on_completion_bit(&self) -> bool {
        self.sdctl.assert_bit(2)
    }

    pub fn set_interrupt_on_completion_enable_bit(&self) {
        self.sdctl.set_bit(2);
    }

    pub fn clear_interrupt_on_completion_bit(&self) {
        self.sdctl.clear_bit(2);
    }

    pub fn assert_fifo_error_interrupt_enable_bit(&self) -> bool {
        self.sdctl.assert_bit(3)
    }

    pub fn set_fifo_error_interrupt_enable_bit(&self) {
        self.sdctl.set_bit(3);
    }

    pub fn clear_fifo_error_interrupt_enable_bit(&self) {
        self.sdctl.clear_bit(3);
    }

    pub fn assert_descriptor_error_interrupt_enable_bit(&self) -> bool {
        self.sdctl.assert_bit(4)
    }

    pub fn set_descriptor_error_interrupt_enable_bit(&self) {
        self.sdctl.set_bit(4);
    }

    pub fn clear_descriptor_error_interrupt_enable_bit(&self) {
        self.sdctl.clear_bit(4);
    }

    // fn stripe_control();
    // fn set_stripe_control();

    pub fn assert_traffic_priority_enable_bit(&self) -> bool {
        self.sdctl.assert_bit(18)
    }

    pub fn set_traffic_priority_enable_bit(&self) {
        self.sdctl.set_bit(18);
    }

    pub fn clear_traffic_priority_enable_bit(&self) {
        self.sdctl.clear_bit(18);
    }

    // fn set_bidirectional_stream_as_input()
    // fn set_bidirectional_stream_as_output()

    pub fn stream_id(&self) -> u8 {
        match (self.sdctl.read() >> 20) & 0xF {
            0 => panic!("IHDA sound card reports an invalid stream number"),
            stream_number => stream_number as u8,
        }
    }

    pub fn set_stream_id(&self, stream_id: u8) {
        // REMINDER: the highest byte of self.sdctl.read() is the sdsts register and should not be modified
        self.sdctl.write((self.sdctl.read() & 0xFF0F_FFFF) | ((stream_id as u32) << 20));
    }

    // ########## SDSTS ##########
    pub fn assert_buffer_completion_interrupt_status_bit(&self) -> bool {
        self.sdsts.assert_bit(2)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.9)
    pub fn clear_buffer_completion_interrupt_status_bit(&self) {
        self.sdsts.set_bit(2);
    }

    pub fn assert_fifo_error_bit(&self) -> bool {
        self.sdsts.assert_bit(3)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.9)
    pub fn clear_fifo_error_bit(&self) {
        self.sdsts.set_bit(3);
    }

    pub fn assert_descriptor_error_bit(&self) -> bool {
        self.sdsts.assert_bit(4)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.9)
    pub fn clear_descriptor_error_bit(&self) {
        self.sdsts.set_bit(4);
    }

    pub fn fifo_ready(&self) {
        self.sdsts.assert_bit(5);
    }

    // ########## SDLPIB ##########
    pub fn link_position_in_buffer(&self) -> u32 {
        self.sdlpib.read()
    }

    // ########## SDCBL ##########
    pub fn cyclic_buffer_lenght(&self) -> u32 {
        self.sdcbl.read()
    }

    pub fn set_cyclic_buffer_lenght(&self, length: u32) {
        if self.assert_stream_run_bit() {
            panic!("Trying to write to SDCBL register while stream running is not allowed (see specification, section 3.3.38)");
        }
        self.sdcbl.write(length);
    }

    // ########## SDLVI ##########
    pub fn last_valid_index(&self) -> u8 {
        (self.sdlvi.read() & 0xFF) as u8
    }

    pub fn set_last_valid_index(&self, length: u8) {
        if self.assert_stream_run_bit() {
            panic!("Trying to write to SDLVI register while stream running is not allowed (see specification, section 3.3.38)");
        }
        self.sdlvi.write(length as u16);
    }

    // ########## SDFIFOW ##########
    pub fn fifo_watermark(&self) -> FIFOWatermark {
        match (self.sdfifow.read() & 0b111) as u8 {
            0b100 => FIFOWatermark::Bit32,
            0b101 => FIFOWatermark::Bit64,
            _ => panic!("Unsupported FIFO Watermark for stream reported by sound card")
        }
    }

    pub fn set_fifo_watermark(&self, watermark: FIFOWatermark) {
        match watermark {
            FIFOWatermark::Bit32 => self.sdfifow.write(0b100),
            FIFOWatermark::Bit64 => self.sdfifow.write(0b101),
        }
    }

    // ########## SDFIFOD ##########
    pub fn fifo_size(&self) -> u16 {
        self.sdfifod.read()
    }

    // ########## SDFMT ##########
    // _TODO_: maybe refactor by returning StreamFormat struct (not existing yet), as StreamFormatResponse should only be associated to converter widgets' stream format, not the format of a stream
    pub fn stream_format(&self) -> StreamFormat {
        StreamFormat::from_u16(self.sdfmt.read())
    }

    pub fn set_stream_format(&self, stream_format: StreamFormat) {
        self.sdfmt.write(stream_format.as_u16());
    }

    // ########## SDBDPL and SDBDPU ##########
    pub fn set_bdl_pointer_address(&self, address: u64) {
        if self.assert_stream_run_bit() {
            panic!("Trying to write to BDL address registers while stream running is not allowed (see specification, section 3.3.38)");
        }

        self.sdbdpl.write((address & 0xFFFFFFFF) as u32);
        self.sdbdpu.write(((address & 0xFFFFFFFF_00000000) >> 32) as u32);
    }

    pub fn bdl_pointer_address(&self) -> u64 {
        ((self.sdbdpu.read() as u64) << 32) | self.sdbdpl.read() as u64
    }
}


#[derive(Clone, Debug)]
pub enum FIFOWatermark {
    Bit32,
    Bit64,
}

// representation of all IHDA registers
#[derive(Getters)]
pub struct Controller {
    gcap: Register<u16>,
    vmin: Register<u8>,
    vmaj: Register<u8>,
    outpay: Register<u16>,
    inpay: Register<u16>,
    gctl: Register<u32>,
    wakeen: Register<u16>,
    wakests: Register<u16>,
    gsts: Register<u16>,
    // The register GCAP2 is only defined in 8-series-chipset-pch-datasheet.pdf for the chipset on the used testing device.
    // As the IHDA specification doesn't mention this register at all, it might not exist for other IHDA sound cards.
    gcap2: Register<u16>,
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

impl Controller {
    pub fn new(mmio_base_address: VirtAddr) -> Self {
        let mmio_base_address = mmio_base_address.as_u64();

        // gcap contains amount of input, output and bidirectional stream descriptors of the specific IHDA controller (see section 3.3.2 of the specification)
        let gcap = Register::new(mmio_base_address as *mut u16, "GCAP");
        let input_stream_descriptor_amount = (gcap.read() >> 8) & 0xF;
        let output_stream_descriptor_amount = (gcap.read() >> 12) & 0xF;
        let bidirectional_stream_descriptor_amount = (gcap.read() >> 3) & 0b1_1111;

        let mut input_stream_descriptors = Vec::new();
        for index in 0..input_stream_descriptor_amount {
            input_stream_descriptors.push(StreamDescriptorRegisters::new(
                mmio_base_address
                    + OFFSET_OF_FIRST_SOUND_DESCRIPTOR
                    + (SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES * index as u64)
            ));
        }

        let mut output_stream_descriptors = Vec::new();
        for index in 0..output_stream_descriptor_amount {
            output_stream_descriptors.push(StreamDescriptorRegisters::new(
                mmio_base_address
                    + OFFSET_OF_FIRST_SOUND_DESCRIPTOR
                    + (SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES * (input_stream_descriptor_amount + index) as u64)
            ));
        }

        let mut bidirectional_stream_descriptors = Vec::new();
        for index in 0..bidirectional_stream_descriptor_amount {
            bidirectional_stream_descriptors.push(StreamDescriptorRegisters::new(
                mmio_base_address
                    + OFFSET_OF_FIRST_SOUND_DESCRIPTOR
                    + (SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES * (input_stream_descriptor_amount + output_stream_descriptor_amount + index) as u64)
            ));
        }

        Self {
            gcap,
            vmin: Register::new((mmio_base_address + 0x2) as *mut u8, "VMIN"),
            vmaj: Register::new((mmio_base_address + 0x3) as *mut u8, "VMAJ"),
            outpay: Register::new((mmio_base_address + 0x4) as *mut u16, "OUTPAY"),
            inpay: Register::new((mmio_base_address + 0x6) as *mut u16, "INPAY"),
            gctl: Register::new((mmio_base_address + 0x8) as *mut u32, "GCTL"),
            wakeen: Register::new((mmio_base_address + 0xC) as *mut u16, "WAKEEN"),
            wakests: Register::new((mmio_base_address + 0xE) as *mut u16, "WAKESTS"),
            gsts: Register::new((mmio_base_address + 0x10) as *mut u16, "GSTS"),
            // gcap2 only specified in phc-spec, not in IHDA-spec
            gcap2: Register::new((mmio_base_address + 0x12) as *mut u16, "GCAP2"),
            // bytes with offset 0x14 to 0x17 are reserved
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

    // ########## GCAP ##########
    fn supports_64bit_bdl_addresses(&self) -> bool {
        self.gcap.assert_bit(0)
    }

    fn number_of_serial_data_out_signals(&self) -> u8 {
        match (self.gcap.read() >> 1) & 0b11 {
            0b00 => 1,
            0b01 => 2,
            0b10 => 4,
            _ => panic!("IHDA sound card reports an invalid number of Serial Data Out Signals")
        }
    }

    fn number_of_bidirectional_streams_supported(&self) -> u8 {
        let bss = ((self.gcap.read() >> 3) & 0b1_1111) as u8;
        if bss > MAX_AMOUNT_OF_BIDIRECTIONAL_STREAMS {
            panic!("IHDA sound card reports an invalid number of Bidirectional Streams Supported")
        }
        bss
    }

    fn number_of_input_streams_supported(&self) -> u8 {
        ((self.gcap.read() >> 8) & 0xF) as u8
    }

    fn number_of_output_streams_supported(&self) -> u8 {
        ((self.gcap.read() >> 12) & 0xF) as u8
    }

    // ########## VMIN and VMAJ ##########
    fn specification_version(&self) -> (u8, u8) {
        (self.vmaj.read(), self.vmin.read())
    }

    // ########## OUTPAY ##########
    fn output_payload_capacity_in_words(&self) -> u16 {
        self.outpay.read()
    }

    // ########## INPAY ##########
    fn input_payload_capacity_in_words(&self) -> u16 {
        self.inpay.read()
    }

    // ########## GCTL ##########
    pub fn reset(&self) {
        self.gctl.set_bit(0);
        let start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        while !self.gctl.assert_bit(0) {
            if timer().read().systime_ms() > start_timer + BIT_ASSERTION_TIMEOUT_IN_MS {
                panic!("IHDA controller reset timed out")
            }
        }

        // according to IHDA specification (section 4.3 Codec Discovery), the system should at least wait .521 ms after reading CRST as 1, so that the codecs have time to self-initialize
        Timer::wait(1);
    }

    // fn initiate_flush();

    fn assert_unsolicited_response_enable_bit(&self) -> bool {
        self.gctl.assert_bit(8)
    }

    fn set_unsolicited_response_enable_bit(&self) {
        self.gctl.set_bit(8);
    }

    fn clear_unsolicited_response_enable_bit(&self) {
        self.gctl.clear_bit(8);
    }

    // ########## WAKEEN ##########

    fn assert_sdin_wake_enable_bit(&self, sdin_index: u8) -> bool {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakeen.assert_bit(sdin_index)
    }

    fn set_sdin_wake_enable_bit(&self, sdin_index : u8) {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakeen.set_bit(sdin_index);
    }

    fn clear_sdin_wake_enable_bit(&self, sdin_index : u8) {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakeen.clear_bit(sdin_index);
    }

    // ########## WAKESTS ##########

    fn assert_sdin_state_change_status_bit(&self, sdin_index: u8) -> bool {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakests.assert_bit(sdin_index)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.9)
    fn clear_sdin_state_change_status_bit(&self, sdin_index : u8) {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakests.set_bit(sdin_index);
    }

    // ########## GSTS ##########

     fn assert_flush_status_bit(&self) -> bool {
        self.gsts.assert_bit(1)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.10)
     fn clear_flush_status_bit(&self) {
        self.gctl.set_bit(1);
    }

    // ########## GCAP2 ##########
     fn energy_efficient_audio_capability(&self) -> bool {
        self.gsts.assert_bit(0)
    }

    // ########## OUTSTRMPAY ##########
     fn output_stream_payload_capability_in_words(&self) -> u16 {
        self.outstrmpay.read()
    }

    // ########## INSTRMPAY ##########
     fn input_stream_payload_capability_in_words(&self) -> u16 {
        self.instrmpay.read()
    }

    // ########## INTCTL ##########

    //  fn assert_stream_interrupt_enable_bit(&self) -> bool;
    //
    //  fn set_stream_interrupt_enable_bit(&self);
    //
    //  fn clear_stream_interrupt_enable_bit(&self);

     fn assert_controller_interrupt_enable_bit(&self) -> bool {
        self.intctl.assert_bit(30)
    }

     fn set_controller_interrupt_enable_bit(&self) {
        self.intctl.set_bit(30);
    }

     fn clear_controller_interrupt_enable_bit(&self) {
        self.intctl.clear_bit(30);
    }

     fn assert_global_interrupt_enable_bit(&self) -> bool {
        self.intctl.assert_bit(31)
    }

     fn set_global_interrupt_enable_bit(&self) {
        self.intctl.set_bit(31);
    }

     fn clear_global_interrupt_enable_bit(&self) {
        self.intctl.clear_bit(31);
    }

    // ########## INTCTL ##########

    // not implemented yet

    // ########## WALCLK ##########

     fn wall_clock_counter(&self) -> u32 {
        self.walclk.read()
    }

    // ########## SSYNC ##########

    // not implemented yet

    // ########## CORBLBASE and CORBUBASE ##########

     fn set_corb_address(&self, start_frame: PhysFrame) {
        // _TODO_: assert that the DMA engine is not running before writing to CORBLASE and CORBUBASE (see specification, section 3.3.18 and 3.3.19)
        let start_address = start_frame.start_address().as_u64();
        let lbase = (start_address & 0xFFFFFFFF) as u32;
        let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;

        self.corblbase.write(lbase);
        self.corbubase.write(ubase);
    }

     fn corb_address(&self) -> u64 {
        (self.corbubase.read() as u64) << 32 | (self.corblbase.read() >> 1 << 1) as u64
    }

    // ########## CORBWP ##########

    fn corb_write_pointer(&self) -> u8 {
        (self.corbwp.read() & 0xFF) as u8
    }

    fn set_corb_write_pointer(&self, offset: u8) {
        self.corbwp.write(offset as u16);
    }

    fn reset_corb_write_pointer(&self) {
        self.corbwp.clear_all_bits();
    }

    // ########## CORBRP ##########

    fn corb_read_pointer(&self) -> u8 {
        (self.corbrp.read() & 0xFF) as u8
    }

    fn reset_corb_read_pointer(&self) {
        self.corbrp.set_bit(15);
        let start_timer = timer().read().systime_ms();
        // value for CORBRPRST_TIMEOUT arbitrarily chosen
        
        while !self.corbrp.assert_bit(15) {
            if timer().read().systime_ms() > start_timer + BIT_ASSERTION_TIMEOUT_IN_MS {
                panic!("CORB read pointer reset timed out")
            }
        }

        self.corbrp.clear_bit(15);
    }

    // ########## CORBCTL ##########

     fn assert_corb_memory_error_interrupt_enable_bit(&self) -> bool {
        self.corbctl.assert_bit(0)
    }

     fn set_corb_memory_error_interrupt_enable_bit(&self) {
        self.corbctl.set_bit(0);
    }

     fn clear_corb_memory_error_interrupt_enable_bit(&self) {
        self.corbctl.clear_bit(0);
    }

     fn start_corb_dma(&self) {
        self.corbctl.set_bit(1);
        
        // software must read back value (see specification, section 3.3.22)
        let start_timer = timer().read().systime_ms();
        while !self.corbctl.assert_bit(1) {
            if timer().read().systime_ms() > start_timer + BIT_ASSERTION_TIMEOUT_IN_MS {
                panic!("IHDA controller reset timed out")
            }
        }
    }

     fn stop_corb_dma(&self) {
        self.corbctl.clear_bit(1);

        // software must read back value (see specification, section 3.3.22)
        let start_timer = timer().read().systime_ms();
        while self.corbctl.assert_bit(1) {
            if timer().read().systime_ms() > start_timer + BIT_ASSERTION_TIMEOUT_IN_MS {
                panic!("IHDA controller reset timed out")
            }
        }
    }

    // ########## CORBSTS ##########

     fn assert_corb_memory_error_indication_bit(&self) -> bool {
        self.corbsts.assert_bit(0)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.10)
     fn clear_corb_memory_error_indication_bit(&self) {
        self.corbsts.set_bit(0);
    }

    // ########## CORBSIZE ##########

     fn corb_size_in_entries(&self) -> CorbSize {
        match (self.corbsize.read()) & 0b11 {
            0b00 => CorbSize::TwoEntries,
            0b01 => CorbSize::SixteenEntries,
            0b10 => CorbSize::TwoHundredFiftySixEntries,
            _ => panic!("IHDA sound card reports an invalid CORB size")
        }
    }

     fn set_corb_size_in_entries(&self, corb_size: CorbSize) {
        match corb_size {
            CorbSize::TwoEntries => self.corbsize.write(self.corbsize.read() & 0b1111_11_00),
            CorbSize::SixteenEntries => self.corbsize.write(self.corbsize.read() & 0b1111_11_00 | 0b01),
            CorbSize::TwoHundredFiftySixEntries => self.corbsize.write(self.corbsize.read() & 0b1111_11_00 | 0b10),
        }
    }

     fn corb_size_capability(&self) -> RingbufferCapability {
        RingbufferCapability::new(
            self.corbsize.assert_bit(4),
            self.corbsize.assert_bit(5),
            self.corbsize.assert_bit(6),
        )
    }

    pub fn init_corb(&self) {
        // disable CORB DMA engine (CORBRUN) and CORB memory error interrupt (CMEIE)
        self.clear_corb_memory_error_interrupt_enable_bit();
        self.stop_corb_dma();

        // verify that CORB size is 1KB (IHDA specification, section 3.3.24: "There is no requirement to support more than one CORB Size.")
        assert_eq!(self.corb_size_in_entries(), CorbSize::TwoHundredFiftySixEntries);

        // setup MMIO space for Command Outbound Ring Buffer – CORB
        let corb_frame_range = memory::physical::alloc(2);
        match corb_frame_range {
            PhysFrameRange { start, end: _ } => {
                self.set_corb_address(start);
            }
        }

        self.reset_corb_write_pointer();
        self.reset_corb_read_pointer();
    }

    pub fn start_corb(&self) {
        // set CORBRUN and CMEIE bits
        self.set_controller_interrupt_enable_bit();
        self.start_corb_dma();
    }

    // ########## RIRBLBASE and RIRBUBASE ##########

     fn set_rirb_address(&self, start_frame: PhysFrame) {
        // _TODO_: assert that the DMA engine is not running before writing to RIRBLASE and RIRBUBASE (see specification, section 3.3.18 and 3.3.19)
        let start_address = start_frame.start_address().as_u64();
        let lbase = (start_address & 0xFFFFFFFF) as u32;
        let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;

        self.rirblbase.write(lbase);
        self.rirbubase.write(ubase);
    }

     fn rirb_address(&self) -> u64 {
        (self.rirbubase.read() as u64) << 32 | (self.rirblbase.read() >> 1 << 1) as u64
    }

    // ########## RIRBWP ##########

    fn rirb_write_pointer(&self) -> u8 {
        (self.rirbwp.read() & 0xFF) as u8
    }

    fn reset_rirb_write_pointer(&self) {
        // _todo: assert that dma is not running
        self.rirbwp.set_bit(15);
    }

    // ########## RINTCNT ##########

    // not implemented yet

    // ########## RIRBCTL ##########

     fn assert_response_interrupt_control_bit(&self) -> bool {
        self.rirbctl.assert_bit(0)
    }

     fn set_response_interrupt_control_bit(&self) {
        self.rirbctl.set_bit(0);
    }

     fn clear_response_interrupt_control_bit(&self) {
        self.rirbctl.clear_bit(0);
    }

     fn assert_rirb_dma_enable_bit(&self) -> bool {
        self.rirbctl.assert_bit(1)
    }

     fn start_rirb_dma(&self) {
        self.rirbctl.set_bit(1);
    }

     fn stop_rirb_dma(&self) {
        self.rirbctl.clear_bit(1);
    }

     fn assert_response_overrun_interrupt_control_bit(&self) -> bool {
        self.rirbctl.assert_bit(2)
    }

     fn set_response_overrun_interrupt_control_bit(&self) {
        self.rirbctl.set_bit(2);
    }

     fn clear_response_overrun_interrupt_control_bit(&self) {
        self.rirbctl.clear_bit(2);
    }

    // ########## RIRBSTS ##########

    // ########## RIRBSIZE ##########

     fn rirb_size_capability(&self) -> RingbufferCapability {
        RingbufferCapability::new(
            self.rirbsize.assert_bit(4),
            self.rirbsize.assert_bit(5),
            self.rirbsize.assert_bit(6),
        )
    }

    pub fn init_rirb(&self) {
        self.stop_rirb_dma();
        self.clear_response_interrupt_control_bit();
        self.clear_response_overrun_interrupt_control_bit();

        // setup MMIO space for Response Inbound Ring Buffer – RIRB
        let rirb_frame_range = memory::physical::alloc(4);
        match rirb_frame_range {
            PhysFrameRange { start, end: _ } => {
                self.set_rirb_address(start);
            }
        }

        self.reset_rirb_write_pointer();
    }

    pub fn start_rirb(&self) {
        self.set_response_interrupt_control_bit();
        self.set_response_overrun_interrupt_control_bit();
        self.start_rirb_dma();

        // CORB/RIRB demo

        Timer::wait(1000);
        unsafe { debug!("CORB entry 0: {:#x}", (self.corb_address() as *mut u32).read()); }
        unsafe { debug!("RIRB entry 0: {:#x}", (self.rirb_address() as *mut u64).read()); }
        unsafe { debug!("CORB entry 1: {:#x}", ((self.corb_address() + 4) as *mut u32).read()); }
        unsafe { debug!("RIRB entry 1: {:#x}", ((self.rirb_address() + 8) as *mut u64).read()); }
        self.corbwp.dump();
        self.corbrp.dump();
        self.rirbwp.dump();

        unsafe { ((self.corb_address() + 4) as *mut u32).write(GetParameter(NodeAddress::new(0, 0), VendorId).as_u32()); }
        // unsafe { ((self.corb_address() + 32) as *mut u32).write(GetParameter(audio_out_widget, OutputAmpCapabilities).as_u32()); }

        // debug!("VendorIdResponse from immediate command: {:?}", VendorIdResponse::try_from(self.immediate_command(GetParameter(NodeAddress::new(0, 0), VendorId))).unwrap());

        self.corbwp().write(self.corbwp.read() + 1);
        Timer::wait(200);
        unsafe { debug!("CORB entry 0: {:#x}", (self.corb_address() as *mut u32).read()); }
        unsafe { debug!("RIRB entry 0: {:#x}", (self.rirb_address() as *mut u64).read()); }
        unsafe { debug!("CORB entry 1: {:#x}", ((self.corb_address() + 4) as *mut u32).read()); }
        unsafe { debug!("RIRB entry 1: {:#x}", ((self.rirb_address() + 8) as *mut u64).read()); }
        self.corbwp.dump();
        self.corbrp.dump();
        self.rirbwp.dump();


        debug!("CORB address: {:#x}", self.corb_address());
        debug!("RIRB address: {:#x}", self.rirb_address());
    }

    // ########## DPLBASE and DPUBASE ##########

    fn enable_dma_position_buffer(&self) {
        self.dpiblbase.set_bit(0);
    }

    fn disable_dma_position_buffer(&self) {
        self.dpiblbase.clear_bit(0);
    }

    fn dma_position_buffer_address(&self) -> u64 {
        (self.dpibubase.read() as u64) << 32 | (self.dpiblbase.read() >> 1 << 1) as u64
    }

    fn set_dma_position_buffer_address(&self, start_frame: PhysFrame) {
        // _TODO_: assert that the DMA engine is not running before writing to DPLASE and DPUBASE (see specification, section 3.3.18 and 3.3.19)
        let start_address = start_frame.start_address().as_u64();
        let lbase = (start_address & 0xFFFFFFFF) as u32;
        let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;

        // preserve DMA Position Buffer Enable bit at position 0 when writing address
        self.dpiblbase.write(lbase | (self.dpiblbase.assert_bit(0) as u32));
        self.dpibubase.write(ubase);
    }

     pub fn init_dma_position_buffer(&self) {
        let dmapib_frame_range = alloc_no_cache_dma_memory(1);

        self.set_dma_position_buffer_address(dmapib_frame_range.start);
        self.enable_dma_position_buffer();
    }

     fn stream_descriptor_position_in_current_buffer(&self, stream_descriptor_number: u32) -> u32 {
        // see specification section 3.6.1
        let address = self.dma_position_buffer_address() + (stream_descriptor_number as u64 * (2 * DMA_POSITION_IN_BUFFER_ENTRY_SIZE_IN_BYTES));
        unsafe { (address as *mut u32).read() }
    }

    // ########## ICOI ##########

    fn write_command_to_immediate_command_output_interface(&self, command: Command) {
        self.icoi.write(command.as_u32());
    }

    // ########## ICII ##########

    fn read_response_from_immediate_command_input_interface(&self) -> u32 {
        self.icii.read()
    }

    // ########## ICIS ##########

    fn assert_immediate_command_busy_bit(&self) -> bool {
        self.icis.assert_bit(0)
    }

    fn set_immediate_command_busy_bit(&self) {
        self.icis.set_bit(0);
    }

    fn clear_immediate_command_busy_bit(&self) {
        self.icis.clear_bit(0);
    }

    fn assert_immediate_result_valid_bit(&self) -> bool {
        self.icis.assert_bit(1)
    }

    fn set_immediate_result_ready_bit(&self) {
        self.icis.set_bit(1);
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.4.3)
    fn clear_immediate_result_ready_bit(&self) {
        self.icis.set_bit(1);
    }

    fn immediate_command(&self, command: Command) -> Response {
        self.write_command_to_immediate_command_output_interface(command);
        self.set_immediate_command_busy_bit();
        let start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        while !self.assert_immediate_result_valid_bit() {
            if timer().read().systime_ms() > start_timer + IMMEDIATE_COMMAND_TIMEOUT_IN_MS {
                panic!("IHDA immediate command timed out")
            }
        }
        Response::new(RawResponse::new(self.read_response_from_immediate_command_input_interface(), command))
    }

    pub fn setup_ihda_config_space(&self) {
        // set Accept Unsolicited Response Enable (UNSOL) bit
        self.clear_unsolicited_response_enable_bit();

        self.set_global_interrupt_enable_bit();
        self.set_controller_interrupt_enable_bit();

        // enable wake events and interrupts for all SDIN (actually, only one bit needs to be set, but this works for now...)
        self.wakeen.set_all_bits();
    }

    // check the bitmask from bits 0 to 14 of the WAKESTS (in the specification also called STATESTS) indicating available codecs
    // then find all function group nodes and widgets associated with a codec
    pub fn scan_for_available_codecs(&self) -> Vec<Codec> {
        let mut codecs: Vec<Codec> = Vec::new();

        for codec_address in 0..MAX_AMOUNT_OF_CODECS {
            if self.wakests().assert_bit(codec_address) {
                let root_node_addr = NodeAddress::new(codec_address, 0);
                let vendor_id = VendorIdResponse::try_from(self.immediate_command(GetParameter(root_node_addr, VendorId))).unwrap();
                let revision_id = RevisionIdResponse::try_from(self.immediate_command(GetParameter(root_node_addr, RevisionId))).unwrap();

                let function_groups = self.scan_codec_for_available_function_groups(root_node_addr);

                codecs.push(Codec::new(codec_address, vendor_id, revision_id, function_groups));
            }
        }
        codecs
    }

    fn scan_codec_for_available_function_groups(&self, root_node_addr: NodeAddress) -> Vec<FunctionGroup> {
        let mut function_groups: Vec<FunctionGroup> = Vec::new();

        let subordinate_node_count = SubordinateNodeCountResponse::try_from(self.immediate_command(GetParameter(root_node_addr, SubordinateNodeCount))).unwrap();
        for node_id in *subordinate_node_count.starting_node_number()..(*subordinate_node_count.starting_node_number() + *subordinate_node_count.total_number_of_nodes()) {
            let function_group_node_address = NodeAddress::new(*root_node_addr.codec_address(), node_id);
            let function_group_type = FunctionGroupTypeResponse::try_from(self.immediate_command(GetParameter(function_group_node_address, FunctionGroupType))).unwrap();
            let audio_function_group_caps = AudioFunctionGroupCapabilitiesResponse::try_from(self.immediate_command(GetParameter(function_group_node_address, AudioFunctionGroupCapabilities))).unwrap();
            let sample_size_rate_caps = SampleSizeRateCAPsResponse::try_from(self.immediate_command(GetParameter(function_group_node_address, SampleSizeRateCAPs))).unwrap();
            let supported_stream_formats = SupportedStreamFormatsResponse::try_from(self.immediate_command(GetParameter(function_group_node_address, SupportedStreamFormats))).unwrap();
            let input_amp_caps = AmpCapabilitiesResponse::try_from(self.immediate_command(GetParameter(function_group_node_address, InputAmpCapabilities))).unwrap();
            let output_amp_caps = AmpCapabilitiesResponse::try_from(self.immediate_command(GetParameter(function_group_node_address, OutputAmpCapabilities))).unwrap();
            let supported_power_states = SupportedPowerStatesResponse::try_from(self.immediate_command(GetParameter(function_group_node_address, SupportedPowerStates))).unwrap();
            let gpio_count = GPIOCountResponse::try_from(self.immediate_command(GetParameter(function_group_node_address, GPIOCount))).unwrap();

            let widgets = self.scan_function_group_for_available_widgets(function_group_node_address);

            function_groups.push(FunctionGroup::new(
                function_group_node_address,
                function_group_type,
                audio_function_group_caps,
                sample_size_rate_caps,
                supported_stream_formats,
                input_amp_caps,
                output_amp_caps,
                supported_power_states,
                gpio_count,
                widgets));
        }
        function_groups
    }

    fn scan_function_group_for_available_widgets(&self, fg_address: NodeAddress) -> Vec<Widget> {
        let mut widgets: Vec<Widget> = Vec::new();

        let subordinate_node_count = SubordinateNodeCountResponse::try_from(self.immediate_command(GetParameter(fg_address, SubordinateNodeCount))).unwrap();
        for node_id in *subordinate_node_count.starting_node_number()..(*subordinate_node_count.starting_node_number() + *subordinate_node_count.total_number_of_nodes()) {
            let widget_address = NodeAddress::new(*fg_address.codec_address(), node_id);
            let widget_info: WidgetInfoContainer;
            let audio_widget_capabilities_info = AudioWidgetCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, AudioWidgetCapabilities))).unwrap();

            match audio_widget_capabilities_info.widget_type() {
                WidgetType::AudioOutput => {
                    let sample_size_rate_caps = SampleSizeRateCAPsResponse::try_from(self.immediate_command(GetParameter(widget_address, SampleSizeRateCAPs))).unwrap();
                    let supported_stream_formats = SupportedStreamFormatsResponse::try_from(self.immediate_command(GetParameter(widget_address, SupportedStreamFormats))).unwrap();
                    let output_amp_caps = AmpCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, OutputAmpCapabilities))).unwrap();
                    let supported_power_states = SupportedPowerStatesResponse::try_from(self.immediate_command(GetParameter(widget_address, SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, ProcessingCapabilities))).unwrap();
                    widget_info = WidgetInfoContainer::AudioOutputConverter(
                        sample_size_rate_caps,
                        supported_stream_formats,
                        output_amp_caps,
                        supported_power_states,
                        processing_capabilities
                    );
                }
                WidgetType::AudioInput => {
                    let sample_size_rate_caps = SampleSizeRateCAPsResponse::try_from(self.immediate_command(GetParameter(widget_address, SampleSizeRateCAPs))).unwrap();
                    let supported_stream_formats = SupportedStreamFormatsResponse::try_from(self.immediate_command(GetParameter(widget_address, SupportedStreamFormats))).unwrap();
                    let input_amp_caps = AmpCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, InputAmpCapabilities))).unwrap();
                    let connection_list_length = ConnectionListLengthResponse::try_from(self.immediate_command(GetParameter(widget_address, ConnectionListLength))).unwrap();
                    let supported_power_states = SupportedPowerStatesResponse::try_from(self.immediate_command(GetParameter(widget_address, SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, ProcessingCapabilities))).unwrap();
                    widget_info = WidgetInfoContainer::AudioInputConverter(
                        sample_size_rate_caps,
                        supported_stream_formats,
                        input_amp_caps,
                        connection_list_length,
                        supported_power_states,
                        processing_capabilities
                    );
                }
                WidgetType::AudioMixer => {
                    let input_amp_caps = AmpCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, InputAmpCapabilities))).unwrap();
                    let output_amp_caps = AmpCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, OutputAmpCapabilities))).unwrap();
                    let connection_list_length = ConnectionListLengthResponse::try_from(self.immediate_command(GetParameter(widget_address, ConnectionListLength))).unwrap();
                    let supported_power_states = SupportedPowerStatesResponse::try_from(self.immediate_command(GetParameter(widget_address, SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, ProcessingCapabilities))).unwrap();
                    let first_connection_list_entries = ConnectionListEntryResponse::try_from(self.immediate_command(GetConnectionListEntry(widget_address, GetConnectionListEntryPayload::new(0)))).unwrap();
                    widget_info = WidgetInfoContainer::Mixer(
                        input_amp_caps,
                        output_amp_caps,
                        connection_list_length,
                        supported_power_states,
                        processing_capabilities,
                        first_connection_list_entries,
                    );
                }
                WidgetType::AudioSelector => {
                    widget_info = WidgetInfoContainer::Selector;
                }

                WidgetType::PinComplex => {
                    let pin_caps = PinCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, PinCapabilities))).unwrap();
                    let input_amp_caps = AmpCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, InputAmpCapabilities))).unwrap();
                    let output_amp_caps = AmpCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, OutputAmpCapabilities))).unwrap();
                    let connection_list_length = ConnectionListLengthResponse::try_from(self.immediate_command(GetParameter(widget_address, ConnectionListLength))).unwrap();
                    let supported_power_states = SupportedPowerStatesResponse::try_from(self.immediate_command(GetParameter(widget_address, SupportedPowerStates))).unwrap();
                    let processing_capabilities = ProcessingCapabilitiesResponse::try_from(self.immediate_command(GetParameter(widget_address, ProcessingCapabilities))).unwrap();
                    let configuration_default = ConfigurationDefaultResponse::try_from(self.immediate_command(GetConfigurationDefault(widget_address))).unwrap();
                    let first_connection_list_entries = ConnectionListEntryResponse::try_from(self.immediate_command(GetConnectionListEntry(widget_address, GetConnectionListEntryPayload::new(0)))).unwrap();
                    widget_info = WidgetInfoContainer::PinComplex(
                        pin_caps,
                        input_amp_caps,
                        output_amp_caps,
                        connection_list_length,
                        supported_power_states,
                        processing_capabilities,
                        configuration_default,
                        first_connection_list_entries,
                    );
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

            widgets.push(Widget::new(widget_address, audio_widget_capabilities_info, widget_info));
        }
        widgets
    }

    pub fn allocate_output_stream(
        &self,
        output_sound_descriptor_number: usize,
        stream_format: StreamFormat,
        buffer_amount: u32,
        pages_per_buffer: u32,
        stream_id: u8
    ) -> Stream {

        Stream::new(self.output_stream_descriptors().get(output_sound_descriptor_number).unwrap(), stream_format, buffer_amount, pages_per_buffer, stream_id)
    }

    fn configure_widget_for_line_out_playback(&self, widget: &Widget, stream: &Stream) {
        match widget.audio_widget_capabilities().widget_type() {
            WidgetType::AudioOutput => {
                // set gain/mute for audio output converter widget (observation: audio output converter widget only owns output amp; mute stays false, no matter what value gets set, but gain reacts to set commands)
                // careful: the gain register is only 7 bits long (bits [6:0]), so the max gain value is 127; writing higher numbers into the u8 for gain will overwrite the mute bit at position 7
                // default gain value is 87
                self.immediate_command(SetAmplifierGainMute(*widget.address(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Both, SetAmplifierGainMuteSide::Both, 0, false, 40)));

                // set stream id
                // channel number for now hard coded to 0
                self.immediate_command(SetChannelStreamId(*widget.address(), SetChannelStreamIdPayload::new(0, *stream.id())));

                // set stream format
                self.immediate_command(SetStreamFormat(*widget.address(), SetStreamFormatPayload::new(*stream.stream_format())));
            }
            WidgetType::AudioInput => {}
            WidgetType::AudioMixer => {
                self.immediate_command(SetAmplifierGainMute(*widget.address(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Input, SetAmplifierGainMuteSide::Both, 0, false, 60)));
            }
            WidgetType::AudioSelector => {}
            WidgetType::PinComplex => {
                // set gain/mute for pin widget (observation: pin widget owns input and output amp; for both, gain stays at 0, no matter what value gets set, but mute reacts to set commands)
                self.immediate_command(SetAmplifierGainMute(*widget.address(), SetAmplifierGainMutePayload::new(SetAmplifierGainMuteType::Both, SetAmplifierGainMuteSide::Both, 0, false, 100)));

                // activate input and output for pin widget
                let pin_widget_control_response = PinWidgetControlResponse::try_from(self.immediate_command(GetPinWidgetControl(*widget.address()))).unwrap();
                /* after the following command, plugging headphones in and out the jack should make an audible noise */
                self.immediate_command(SetPinWidgetControl(*widget.address(), SetPinWidgetControlPayload::enable_input_and_output_amps(pin_widget_control_response)));
            }
            WidgetType::PowerWidget => {}
            WidgetType::VolumeKnobWidget => {}
            WidgetType::BeepGeneratorWidget => {}
            WidgetType::VendorDefinedAudioWidget => {}
        }
    }

    pub fn configure_codec_for_line_out_playback(&self, codec: &Codec, stream: &Stream) {
        let widgets_on_output_path = codec.function_groups().get(0).unwrap().find_widget_path_for_line_out_playback();

        for widget in widgets_on_output_path {
            self.configure_widget_for_line_out_playback(widget, stream);
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CorbSize {
    TwoEntries,
    SixteenEntries,
    TwoHundredFiftySixEntries,
}

impl CorbSize {
    fn as_u16(&self) -> u16 {
        match self {
            CorbSize::TwoEntries => 2,
            CorbSize::SixteenEntries => 16,
            CorbSize::TwoHundredFiftySixEntries => 256,
        }
    }
}

#[derive(Debug, Getters)]
pub struct RingbufferCapability {
    support_two_entries: bool,
    support_sixteen_entries: bool,
    support_two_hundred_fifty_six_entries: bool,
}

impl RingbufferCapability {
    fn new(support_two_entries: bool, support_sixteen_entries: bool, support_two_hundred_fifty_six_entries: bool) -> Self {
        Self {
            support_two_entries,
            support_sixteen_entries,
            support_two_hundred_fifty_six_entries,
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
    pub fn new(address: u64, length_in_bytes: u32, interrupt_on_completion: bool) -> Self {
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
            // probably better use get_bit() function from ihda_node_communication, after moving it to a better place
            // or even better: use a proper library for all the bit operations on unsigned integers
            interrupt_on_completion: ((raw_data >> 96) & 1) == 1,
        }
    }

    pub fn as_u128(&self) -> u128 {
        (self.interrupt_on_completion as u128) << 96 | (self.length_in_bytes as u128) << 64 | self.address as u128
    }
}

#[derive(Debug, Getters)]
pub struct BufferDescriptorList {
    base_address: u64,
    entries: Vec<BufferDescriptorListEntry>,
    last_valid_index: u8,
}

impl BufferDescriptorList {
    pub fn new(cyclic_buffer: &CyclicBuffer) -> Self {
        // setup MMIO space for buffer descriptor list
        // allocate one 4096 bit page which has space for 32 bdl entries with 128 bit each
        // a bdl needs to provide space for at least two entries (256 bit), see specification, section 3.6.2
        const BDL_CAPACITY: u16 = 32;
        let amount_of_entries = cyclic_buffer.audio_buffers().len() as u16;
        if amount_of_entries > BDL_CAPACITY {
            panic!("At the moment a BDL can't have more than 32 entries")
        }
        let bdl_frame_range = alloc_no_cache_dma_memory(1);

        let base_address = match bdl_frame_range {
            PhysFrameRange { start, end: _ } => {
                start.start_address().as_u64()
            }
        };

        let mut entries = Vec::new();
        for buffer in cyclic_buffer.audio_buffers().iter() {
            // interrupt on completion temporarily hard coded to false for all buffers
            entries.push(BufferDescriptorListEntry::new(*buffer.start_address(), *buffer.length_in_bytes(), true))
        }

        Self {
            base_address,
            entries,
            last_valid_index: (amount_of_entries - 1) as u8,
        }
    }

    pub fn get_entry(&self, index: u64) -> BufferDescriptorListEntry {
        unsafe {
            let address = VolatilePtr::new(NonNull::new((self.base_address + (index * BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BYTES)) as *mut u128).unwrap());
            let raw_data = address.read();
            BufferDescriptorListEntry::from(raw_data)
        }
    }

    pub fn set_entry(&self, index: u64, entry: &BufferDescriptorListEntry) {
        unsafe {
            let address = VolatilePtr::new(NonNull::new((self.base_address + (index * BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BYTES)) as *mut u128).unwrap());
            address.write(entry.as_u128())
        };
    }
}


#[derive(Debug, Getters)]
pub struct AudioBuffer {
    start_address: u64,
    length_in_bytes: u32,
}

impl AudioBuffer {
    pub fn new(start_address: u64, length_in_bytes: u32) -> Self {
        Self {
            start_address,
            length_in_bytes,
        }
    }

    pub fn read_sample_from_buffer(&self, index: u64) -> u16 {
        let address = self.start_address + (index * (CONTAINER_16BIT_SIZE_IN_BYTES as u64));
        unsafe { (address as *mut u16).read() }
    }

    pub fn write_sample_to_buffer(&self, sample: u16, index: u64) {
        let address = self.start_address + (index * (CONTAINER_16BIT_SIZE_IN_BYTES as u64));
        unsafe { (address as *mut u16).write(sample); }
    }
}

#[derive(Debug, Getters)]
pub struct CyclicBuffer {
    length_in_bytes: u32,
    audio_buffers: Vec<AudioBuffer>,
}

impl CyclicBuffer {
    pub fn new(buffer_amount: u32, pages_per_buffer: u32) -> Self {
        let buffer_frame_range = alloc_no_cache_dma_memory(buffer_amount * pages_per_buffer);
        let buffer_size_in_bits = pages_per_buffer * PAGE_SIZE as u32;
        let buffer_size_in_bytes = buffer_size_in_bits / 8;
        let start_address = buffer_frame_range.start.start_address().as_u64();
        let mut audio_buffers = Vec::new();
        for index in 0..buffer_amount {
            let buffer = AudioBuffer::new(start_address + (index * buffer_size_in_bits) as u64, buffer_size_in_bytes);
            audio_buffers.push(buffer);
        }
        Self {
            length_in_bytes: buffer_amount * buffer_size_in_bytes,
            audio_buffers,
        }
    }

    pub fn write_samples_to_buffer(&self, buffer_index: usize, samples: &Vec<u16>) {
        let buffer = self.audio_buffers().get(buffer_index).unwrap();
        for (index, sample) in samples.iter().enumerate() {
            buffer.write_sample_to_buffer(*sample, index as u64)
        }
    }
}

#[derive(Getters)]
pub struct Stream<'a> {
    sd_registers: &'a StreamDescriptorRegisters,
    buffer_descriptor_list: BufferDescriptorList,
    cyclic_buffer: CyclicBuffer,
    stream_format: StreamFormat,
    id: u8,
}

impl<'a> Stream<'a> {

    pub fn new(
        sd_registers: &'a StreamDescriptorRegisters,
        stream_format: StreamFormat,
        buffer_amount: u32,
        pages_per_buffer: u32,
        id: u8
    ) -> Self {
        // ########## allocate data buffers and bdl ##########

        let cyclic_buffer = CyclicBuffer::new(buffer_amount, pages_per_buffer);

        let bdl = BufferDescriptorList::new(&cyclic_buffer);


        // ########## construct bdl ##########

        for index in 0..=*bdl.last_valid_index() {
            bdl.set_entry(index as u64, bdl.entries().get(index as usize).unwrap());
        }


        // ########## allocate and configure stream descriptor ##########

        sd_registers.reset_stream();

        sd_registers.set_bdl_pointer_address(*bdl.base_address());

        sd_registers.set_cyclic_buffer_lenght(*cyclic_buffer.length_in_bytes());

        sd_registers.set_last_valid_index(*bdl.last_valid_index());

        sd_registers.set_stream_format(stream_format);
        // sd_registers.set_stream_format(SetStreamFormatPayload::from_response(stream_format));

        sd_registers.set_stream_id(id);

        // sd_registers.set_interrupt_on_completion_enable_bit();
        // sd_registers.set_fifo_error_interrupt_enable_bit();
        // sd_registers.set_descriptor_error_interrupt_enable_bit();

        Self {
            sd_registers,
            buffer_descriptor_list: bdl,
            cyclic_buffer,
            stream_format,
            id,
        }
    }

    // pub fn write_data_to_buffer(&self, buffer_index: usize, samples: Vec<u16>) {
    //     self.cyclic_buffer().write_samples_to_buffer(buffer_index, samples);
    // }

    pub fn write_data_to_buffer(&self, buffer_index: usize, samples: &Vec<u16>) {
        self.cyclic_buffer().write_samples_to_buffer(buffer_index, samples);
    }

    pub fn run(&self) {
        self.sd_registers.set_stream_run_bit();
    }

    pub fn stop(&self) {
        self.sd_registers.clear_stream_run_bit();
    }


}



// #[derive(Clone, Debug)]
// pub enum BitDepth {
//     BitDepth8Bit,
//     BitDepth16Bit,
//     BitDepth20Bit,
//     BitDepth24Bit,
//     BitDepth32Bit,
// }
//
// #[derive(Clone, Debug)]
// pub enum Sample {
//     Sample8Bit(u8),
//     Sample16Bit(u16),
//     Sample20Bit(u32),
//     Sample24Bit(u32),
//     Sample32Bit(u32),
// }
//
// #[derive(Clone, Debug, Getters)]
// pub struct SampleContainer {
//     pub value: Sample,
// }
//
// impl SampleContainer {
//     pub fn new(value: u32, bit_depth: BitDepth) -> Self {
//         match bit_depth {
//             BitDepth::BitDepth8Bit => {
//                 if value > 2.pow(8) - 1 {
//                     panic!("Trying to build sample with value greater than bit depth")
//                 }
//                 Self {
//                     value: Sample8Bit(value as u8),
//                 }
//             }
//             BitDepth::BitDepth16Bit => {
//                 if value > 2.pow(16) - 1 {
//                     panic!("Trying to build sample with value greater than bit depth")
//                 }
//                 Self {
//                     value: Sample16Bit(value as u16),
//                 }
//             }
//             BitDepth::BitDepth20Bit => {
//                 if value > 2.pow(20) - 1 {
//                     panic!("Trying to build sample with value greater than bit depth")
//                 }
//                 Self {
//                     value: Sample20Bit(value),
//                 }
//             }
//             BitDepth::BitDepth24Bit => {
//                 if value > 2.pow(24) - 1 {
//                     panic!("Trying to build sample with value greater than bit depth")
//                 }
//                 Self {
//                     value: Sample24Bit(value),
//                 }
//             }
//             BitDepth::BitDepth32Bit => {
//                 if value > 2.pow(32) - 1 {
//                     panic!("Trying to build sample with value greater than bit depth")
//                 }
//                 Self {
//                     value: Sample32Bit(value)
//                 }
//             }
//         }
//     }
//
//     pub fn length_in_bytes(&self) -> usize {
//         match self.value {
//             Sample8Bit(_) => 1,
//             Sample16Bit(_) => 2,
//             _ => 4,
//         }
//     }
//
//     pub fn as_unsigned<T: PrimInt>(&self) -> T {
//         match self.value {
//             Sample8Bit(value) => { T::from(value).unwrap() }
//             Sample16Bit(value) => { T::from(value).unwrap() }
//             Sample20Bit(value) => { T::from(value).unwrap() }
//             Sample24Bit(value) => { T::from(value).unwrap() }
//             Sample32Bit(value) => { T::from(value).unwrap() }
//         }
//     }
// }
//
// #[derive(Clone, Debug, Getters)]
// pub struct Package {
//     samples: Vec<SampleContainer>,
// }
//
// impl Package {
//     pub fn new(samples: Vec<SampleContainer>) -> Self {
//         Self {
//             samples
//         }
//     }
//
//     pub fn length_in_bytes(&self) -> u32 {
//         (self.samples.len()  * self.samples().get(0).unwrap().length_in_bytes()) as u32
//     }
// }





pub fn alloc_no_cache_dma_memory(frame_count: u32) -> PhysFrameRange {
    let phys_frame_range = memory::physical::alloc(frame_count as usize);

    let kernel_address_space = process_manager().read().kernel_process().unwrap().address_space();
    let start_page = Page::from_start_address(VirtAddr::new(phys_frame_range.start.start_address().as_u64())).unwrap();
    let end_page = Page::from_start_address(VirtAddr::new(phys_frame_range.end.start_address().as_u64())).unwrap();
    let phys_page_range = PageRange { start: start_page, end: end_page };
    kernel_address_space.set_flags(phys_page_range, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE);

    phys_frame_range
}