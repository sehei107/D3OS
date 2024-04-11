#![allow(dead_code)]

use alloc::vec::Vec;
use core::fmt::LowerHex;
use log::debug;
use num_traits::int::PrimInt;
use derive_getters::Getters;
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::{Page, PageTableFlags, PhysFrame};
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::device::ihda_node_communication::{AmpCapabilitiesResponse, AudioFunctionGroupCapabilitiesResponse, AudioWidgetCapabilitiesResponse, ConfigurationDefaultResponse, ConnectionListEntryResponse, ConnectionListLengthResponse, FunctionGroupTypeResponse, GPIOCountResponse, Response, PinCapabilitiesResponse, ProcessingCapabilitiesResponse, RevisionIdResponse, SampleSizeRateCAPsResponse, SubordinateNodeCountResponse, SupportedPowerStatesResponse, SupportedStreamFormatsResponse, VendorIdResponse, RawResponse, Command, StreamFormatResponse, SetStreamFormatPayload};
use crate::device::pit::Timer;
use crate::{memory, process_manager, timer};
use crate::device::ihda_types::Sample::{Sample16Bit, Sample20Bit, Sample24Bit, Sample32Bit, Sample8Bit};
use crate::memory::PAGE_SIZE;

const SOUND_DESCRIPTOR_REGISTERS_LENGTH_IN_BYTES: u64 = 0x20;
const OFFSET_OF_FIRST_SOUND_DESCRIPTOR: u64 = 0x80;
const MAX_AMOUNT_OF_CODECS: u8 = 15;
const MAX_AMOUNT_OF_BIDRECTIONAL_STREAMS: u8 = 30;
const MAX_AMOUNT_OF_SDIN_SIGNALS: u8 = 15;
const MAX_AMOUNT_OF_CHANNELS_PER_STREAM: u8 = 16;
// TIMEOUT values arbitrarily chosen
const BIT_ASSERTION_TIMEOUT_IN_MS: usize = 10000;
const IMMEDIATE_COMMAND_TIMEOUT_IN_MS: usize = 100;
const BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BITS: u64 = 128;
const MAX_AMOUNT_OF_BUFFER_DESCRIPTOR_LIST_ENTRIES: u64 = 256;
const DMA_POSITION_IN_BUFFER_ENTRY_SIZE: u64 = 32;
const CONTAINER_SIZE_FOR_24BIT_SAMPLE: u32 = 32;
const CONTAINER_SIZE_FOR_32BIT_SAMPLE: u32 = 32;



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
    pub fn stream_format(&self) -> StreamFormatResponse {
        StreamFormatResponse::new(self.sdfmt.read() as u32)
    }

    pub fn set_stream_format(&self, set_stream_format_payload: SetStreamFormatPayload) {
        self.sdfmt.write(set_stream_format_payload.as_u16());
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
pub struct ControllerRegisterInterface {
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

impl ControllerRegisterInterface {
    pub fn new(mmio_base_address: u64) -> Self {
        // the following read addresses the Global Capacities (GCAP) register, which contains information on the amount of
        // input, output and bidirectional stream descriptors of a specific IHDA sound card (see section 3.3.2 of the specification)
        let gctl = unsafe { (mmio_base_address as *mut u16).read() as u64 };
        let input_stream_descriptor_amount = (gctl >> 8) & 0xF;
        let output_stream_descriptor_amount = (gctl >> 12) & 0xF;
        let bidirectional_stream_descriptor_amount = (gctl >> 3) & 0b1_1111;

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
    pub fn supports_64bit_bdl_addresses(&self) -> bool {
        self.gcap.assert_bit(0)
    }

    pub fn number_of_serial_data_out_signals(&self) -> u8 {
        match (self.gcap.read() >> 1) & 0b11 {
            0b00 => 1,
            0b01 => 2,
            0b10 => 4,
            _ => panic!("IHDA sound card reports an invalid number of Serial Data Out Signals")
        }
    }

    pub fn number_of_bidirectional_streams_supported(&self) -> u8 {
        let bss = ((self.gcap.read() >> 3) & 0b1_1111) as u8;
        if bss > MAX_AMOUNT_OF_BIDRECTIONAL_STREAMS {
            panic!("IHDA sound card reports an invalid number of Bidirectional Streams Supported")
        }
        bss
    }

    pub fn number_of_input_streams_supported(&self) -> u8 {
        ((self.gcap.read() >> 8) & 0xF) as u8
    }

    pub fn number_of_output_streams_supported(&self) -> u8 {
        ((self.gcap.read() >> 12) & 0xF) as u8
    }

    // ########## VMIN and VMAJ ##########
    pub fn specification_version(&self) -> (u8, u8) {
        (self.vmaj.read(), self.vmin.read())
    }

    // ########## OUTPAY ##########
    pub fn output_payload_capacity_in_words(&self) -> u16 {
        self.outpay.read()
    }

    // ########## INPAY ##########
    pub fn input_payload_capacity_in_words(&self) -> u16 {
        self.inpay.read()
    }

    // ########## GCTL ##########
    pub fn reset_controller(&self) {
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

    // pub fn initiate_flush();

    pub fn assert_unsol_bit(&self) -> bool {
        self.gctl.assert_bit(8)
    }

    pub fn set_unsol_bit(&self) {
        self.gctl.set_bit(8);
    }

    pub fn clear_unsol_bit(&self) {
        self.gctl.clear_bit(8);
    }

    // ########## WAKEEN ##########

    pub fn assert_sdin_wake_enable_bit(&self, sdin_index: u8) -> bool {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakeen.assert_bit(sdin_index)
    }

    pub fn set_sdin_wake_enable_bit(&self, sdin_index : u8) {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakeen.set_bit(sdin_index);
    }

    pub fn clear_sdin_wake_enable_bit(&self, sdin_index : u8) {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakeen.clear_bit(sdin_index);
    }

    // ########## WAKESTS ##########

    pub fn assert_sdin_state_change_status_bit(&self, sdin_index: u8) -> bool {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakests.assert_bit(sdin_index)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.9)
    pub fn clear_sdin_state_change_status_bit(&self, sdin_index : u8) {
        if sdin_index > MAX_AMOUNT_OF_SDIN_SIGNALS - 1 { panic!("index of SDIN signal out of range") }
        self.wakests.set_bit(sdin_index);
    }

    // ########## GSTS ##########

    pub fn assert_flush_status_bit(&self) -> bool {
        self.gsts.assert_bit(1)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.10)
    pub fn clear_flush_status_bit(&self) {
        self.gctl.set_bit(1);
    }

    // ########## GCAP2 ##########
    pub fn energy_efficient_audio_capability(&self) -> bool {
        self.gsts.assert_bit(0)
    }

    // ########## OUTSTRMPAY ##########
    pub fn output_stream_payload_capability_in_words(&self) -> u16 {
        self.outstrmpay.read()
    }

    // ########## INSTRMPAY ##########
    pub fn input_stream_payload_capability_in_words(&self) -> u16 {
        self.instrmpay.read()
    }

    // ########## INTCTL ##########

    // pub fn assert_stream_interrupt_enable_bit(&self) -> bool;
    //
    // pub fn set_stream_interrupt_enable_bit(&self);
    //
    // pub fn clear_stream_interrupt_enable_bit(&self);

    pub fn assert_controller_interrupt_enable_bit(&self) -> bool {
        self.intctl.assert_bit(30)
    }

    pub fn set_controller_interrupt_enable_bit(&self) {
        self.intctl.set_bit(30);
    }

    pub fn clear_controller_interrupt_enable_bit(&self) {
        self.intctl.clear_bit(30);
    }

    pub fn assert_global_interrupt_enable_bit(&self) -> bool {
        self.intctl.assert_bit(31)
    }

    pub fn set_global_interrupt_enable_bit(&self) {
        self.intctl.set_bit(31);
    }

    pub fn clear_global_interrupt_enable_bit(&self) {
        self.intctl.clear_bit(31);
    }

    // ########## INTCTL ##########

    // not implemented yet

    // ########## WALCLK ##########

    pub fn wall_clock_counter(&self) -> u32 {
        self.walclk.read()
    }

    // ########## SSYNC ##########

    // not implemented yet

    // ########## CORBLBASE and CORBUBASE ##########

    pub fn set_corb_address(&self, start_frame: PhysFrame) {
        // _TODO_: assert that the DMA engine is not running before writing to CORBLASE and CORBUBASE (see specification, section 3.3.18 and 3.3.19)
        let start_address = start_frame.start_address().as_u64();
        let lbase = (start_address & 0xFFFFFFFF) as u32;
        let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;

        self.corblbase.write(lbase);
        self.corbubase.write(ubase);
    }

    pub fn corb_address(&self) -> u64 {
        (self.corbubase.read() as u64) << 32 | (self.corblbase.read() >> 1 << 1) as u64
    }

    // ########## CORBWP ##########

    fn current_corb_write_pointer_offset(&self) -> u8 {
        (self.corbwp.read() & 0xFF) as u8
    }

    fn set_corb_write_pointer_offset(&self, offset: u8) {
        self.corbwp.write(offset as u16);
    }

    // ########## CORBRP ##########

    fn current_corb_read_pointer_offset(&self) -> u8 {
        (self.corbrp.read() & 0xFF) as u8
    }

    fn reset_corb_read_pointer(&self) {
        self.corbrp().set_bit(15);
        let start_timer = timer().read().systime_ms();
        // value for CORBRPRST_TIMEOUT arbitrarily chosen
        
        while self.corbrp().read() != 0x0 {
            if timer().read().systime_ms() > start_timer + BIT_ASSERTION_TIMEOUT_IN_MS {
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

    // ########## CORBCTL ##########

    pub fn assert_corb_memory_error_interrupt_enable_bit(&self) -> bool {
        self.corbctl.assert_bit(0)
    }

    pub fn set_corb_memory_error_interrupt_enable_bit(&self) {
        self.corbctl.set_bit(0);
    }

    pub fn clear_corb_memory_error_interrupt_enable_bit(&self) {
        self.corbctl.clear_bit(0);
    }

    pub fn start_corb_dma(&self) {
        self.corbctl.set_bit(1);
        
        // software must read back value (see specification, section 3.3.22)
        let start_timer = timer().read().systime_ms();
        while !self.corbctl.assert_bit(1) {
            if timer().read().systime_ms() > start_timer + BIT_ASSERTION_TIMEOUT_IN_MS {
                panic!("IHDA controller reset timed out")
            }
        }
    }

    pub fn stop_corb_dma(&self) {
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

    pub fn assert_corb_memory_error_indication_bit(&self) -> bool {
        self.corbsts.assert_bit(0)
    }

    // bit gets cleared by writing a 1 to it (see specification, section 3.3.10)
    pub fn clear_corb_memory_error_indication_bit(&self) {
        self.corbsts.set_bit(0);
    }

    // ########## CORBSIZE ##########

    pub fn corb_size_in_entries(&self) -> CorbSize {
        match (self.corbsize.read()) & 0b11 {
            0b00 => CorbSize::TwoEntries,
            0b01 => CorbSize::SixteenEntries,
            0b10 => CorbSize::TwoHundredFiftySixEntries,
            _ => panic!("IHDA sound card reports an invalid CORB size")
        }
    }

    pub fn set_corb_size_in_entries(&self, corb_size: CorbSize) {
        match corb_size {
            CorbSize::TwoEntries => self.corbsize.write(self.corbsize.read() & 0b1111_11_00),
            CorbSize::SixteenEntries => self.corbsize.write(self.corbsize.read() & 0b1111_11_00 | 0b01),
            CorbSize::TwoHundredFiftySixEntries => self.corbsize.write(self.corbsize.read() & 0b1111_11_00 | 0b10),
        }
    }

    pub fn corb_size_capability(&self) -> CorbSizeCapability {
        CorbSizeCapability::new(
            self.corbsize.assert_bit(4),
            self.corbsize.assert_bit(5),
            self.corbsize.assert_bit(6),
        )
    }



    // _TODO_: Whole RIRB implementation



    // ########## DPLBASE and DPUBASE ##########

    pub fn enable_dma_position_buffer(&self) {
        self.dpiblbase.set_bit(0);
    }

    pub fn disable_dma_position_buffer(&self) {
        self.dpiblbase.clear_bit(0);
    }

    pub fn dma_position_buffer_address(&self) -> u64 {
        (self.dpibubase.read() as u64) << 32 | (self.dpiblbase.read() >> 1 << 1) as u64
    }

    pub fn set_dma_position_buffer_address(&self, start_frame: PhysFrame) {
        // _TODO_: assert that the DMA engine is not running before writing to DPLASE and DPUBASE (see specification, section 3.3.18 and 3.3.19)
        let start_address = start_frame.start_address().as_u64();
        let lbase = (start_address & 0xFFFFFFFF) as u32;
        let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;

        // preserve DMA Position Buffer Enable bit at position 0 when writing address
        self.dpiblbase.write(lbase | (self.dpiblbase.assert_bit(0) as u32));
        self.dpibubase.write(ubase);
    }

    pub fn stream_descriptor_position_in_current_buffer(&self, stream_descriptor_number: u32) -> u32 {
        let address = self.dma_position_buffer_address() + (stream_descriptor_number as u64 * DMA_POSITION_IN_BUFFER_ENTRY_SIZE);
        // debug!("address: {:#x}", address);
        unsafe { (address as *mut u32).read() }
    }

    
    
    // _TODO_: review the following functions until end of impl block and use functions above instead of direct reads and writes

    fn immediate_command(&self, command: &Command) -> RawResponse {
        self.icis().write(0b10);
        self.icoi().write(command.as_u32());
        self.icis().write(0b1);
        let start_timer = timer().read().systime_ms();
        // value for CRST_TIMEOUT arbitrarily chosen
        while (self.icis().read() & 0b10) != 0b10 {
            if timer().read().systime_ms() > start_timer + IMMEDIATE_COMMAND_TIMEOUT_IN_MS {
                panic!("IHDA immediate command timed out")
            }
        }
        RawResponse::new(self.icii().read(), command.clone())
    }

    pub fn setup_ihda_config_space(&self) {
        // set Accept Unsolicited Response Enable (UNSOL) bit
        self.gctl().set_bit(8);

        // set global interrupt enable (GIE) and controller interrupt enable (CIE) bits
        self.intctl().set_bit(30);
        self.intctl().set_bit(31);

        // enable wake events and interrupts for all SDIN (actually, only one bit needs to be set, but this works for now...)
        self.wakeen().set_all_bits();
    }

    pub fn init_corb(&self) {
        // disable CORB DMA engine (CORBRUN) and CORB memory error interrupt (CMEIE)
        self.corbctl().clear_all_bits();

        // verify that CORB size is 1KB (IHDA specification, section 3.3.24: "There is no requirement to support more than one CORB Size.")
        let corbsize = self.corbsize().read() & 0b11;

        assert_eq!(corbsize, 0b10);

        // setup MMIO space for Command Outbound Ring Buffer – CORB
        let corb_frame_range = memory::physical::alloc(1);
        match corb_frame_range {
            PhysFrameRange { start, end: _ } => {
                self.set_corb_address(start);
            }
        }

        // the following call leads to panic in QEMU because of timeout, but it seems to work on real hardware without a reset...
        // IHDA::reset_corb(crs);
    }

    pub fn reset_corb(&self) {
        // clear CORBWP
        self.corbwp().clear_all_bits();

        //reset CORBRP
        self.reset_corb_read_pointer();
        self.corbrp().set_bit(15);
        let start_timer = timer().read().systime_ms();
        // value for CORBRPRST_TIMEOUT arbitrarily chosen
        const CORBRPRST_TIMEOUT: usize = 10000;
        while self.corbrp().read() != 0x0 {
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

    pub fn set_rirb_address(&self, start_frame: PhysFrame) {
        // _TODO_: assert that the DMA engine is not running before writing to CORBLASE and CORBUBASE (see specification, section 3.3.18 and 3.3.19)
        let start_address = start_frame.start_address().as_u64();
        let lbase = (start_address & 0xFFFFFFFF) as u32;
        let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;

        self.rirblbase.write(lbase);
        self.rirbubase.write(ubase);
    }

    pub fn rirb_address(&self) -> u64 {
        (self.rirbubase.read() as u64) << 32 | (self.rirblbase.read() >> 1 << 1) as u64
    }

    pub fn init_rirb(&self) {
        // disable RIRB response overrun interrupt control (RIRBOIC), RIRB DMA engine (RIRBDMAEN) and RIRB response interrupt control (RINTCTL)
        self.rirbctl().clear_all_bits();

        // setup MMIO space for Response Inbound Ring Buffer – RIRB
        let rirb_frame_range = memory::physical::alloc(1);
        match rirb_frame_range {
            PhysFrameRange { start, end: _ } => {
                self.set_rirb_address(start);
            }
        }

        // reset RIRBWP
        self.rirbwp().set_bit(15);
    }

    pub fn start_corb(&self) {
        // set CORBRUN and CMEIE bits
        self.corbctl().set_bit(0);
        self.corbctl().set_bit(1);
    }

    pub fn start_rirb(&self) {
        // set RIRBOIC, RIRBDMAEN  und RINTCTL bits
        self.rirbctl().set_bit(0);
        self.rirbctl().set_bit(1);
        self.rirbctl().set_bit(2);
    }

    pub fn send_command(&self, command: &Command) -> Response {
        let response = self.immediate_command(command);
        Response::from_raw_response(response)
    }
}

#[derive(Debug)]
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
pub struct CorbSizeCapability {
    support_two_entries: bool,
    support_sixteen_entries: bool,
    support_two_hundred_fifty_six_entries: bool,
}

impl CorbSizeCapability {
    fn new(support_two_entries: bool, support_sixteen_entries: bool, support_two_hundred_fifty_six_entries: bool) -> Self {
        Self {
            support_two_entries,
            support_sixteen_entries,
            support_two_hundred_fifty_six_entries,
        }
    }
}


#[derive(Clone, Debug, Getters)]
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
            entries.push(BufferDescriptorListEntry::new(*buffer.start_address(), *buffer.length_in_bytes(), false))
        }

        Self {
            base_address,
            entries,
            last_valid_index: (amount_of_entries - 1) as u8,
        }
    }

    pub fn get_entry(&self, index: u64) -> BufferDescriptorListEntry {
        let address = (self.base_address + (index * BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BITS)) as *mut u128;
        // debug!("bdl entry [{}] address: {:#x}", index, address);
        let raw_data = unsafe { address.read() };
        BufferDescriptorListEntry::from(raw_data)
    }

    pub fn set_entry(&self, index: u64, entry: &BufferDescriptorListEntry) {
        let address = (self.base_address + (index * BUFFER_DESCRIPTOR_LIST_ENTRY_SIZE_IN_BITS)) as *mut u128;
        unsafe { address.write(entry.as_u128()) };

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

    pub fn read_sample_from_buffer(&self, index: u64) -> u32 {
        let address = self.start_address + (index * (CONTAINER_SIZE_FOR_32BIT_SAMPLE as u64));
        // debug!("read_address: {:#x}", address);
        unsafe { (address as *mut u32).read() }
    }

    pub fn write_sample_to_buffer(&self, sample: u32, index: u64) {
        let address = self.start_address + (index * (CONTAINER_SIZE_FOR_32BIT_SAMPLE as u64));
        // debug!("write_address: {:#x}", address);
        unsafe { (address as *mut u32).write(sample); }
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
        let buffer_size_in_bytes = (pages_per_buffer * PAGE_SIZE as u32) / 8;
        let start_address = buffer_frame_range.start.start_address().as_u64();
        let mut audio_buffers = Vec::new();
        for index in 0..buffer_amount {
            let buffer = AudioBuffer::new(start_address + (index * buffer_size_in_bytes) as u64, buffer_size_in_bytes);
            audio_buffers.push(buffer);
        }
        Self {
            length_in_bytes: buffer_amount * buffer_size_in_bytes,
            audio_buffers,
        }
    }

    pub fn write_samples_to_buffer(&self, buffer_index: u32, samples: Vec<u32>) {
        for sample_index in 0..samples.len() {
            let sample = *samples.get(sample_index).unwrap();
            let buffer = self.audio_buffers().get(buffer_index as usize).unwrap();
            buffer.write_sample_to_buffer(sample, sample_index as u64)
        }
    }
}



#[derive(Clone, Debug)]
pub enum BitDepth {
    BitDepth8Bit,
    BitDepth16Bit,
    BitDepth20Bit,
    BitDepth24Bit,
    BitDepth32Bit,
}

#[derive(Clone, Debug)]
pub enum Sample {
    Sample8Bit(u8),
    Sample16Bit(u16),
    Sample20Bit(u32),
    Sample24Bit(u32),
    Sample32Bit(u32),
}


#[derive(Clone, Debug, Getters)]
pub struct SampleContainer {
    value: Sample,
}

impl SampleContainer {
    pub fn from(value: u32, bit_depth: BitDepth) -> Self {
        match bit_depth {
            BitDepth::BitDepth8Bit => {
                if value > 2.pow(8) - 1 {
                    panic!("Trying to build sample with value greater than bit depth")
                }
                Self {
                    value: Sample8Bit(value as u8),
                }
            }
            BitDepth::BitDepth16Bit => {
                if value > 2.pow(16) - 1 {
                    panic!("Trying to build sample with value greater than bit depth")
                }
                Self {
                    value: Sample16Bit(value as u16),
                }
            }
            BitDepth::BitDepth20Bit => {
                if value > 2.pow(20) - 1 {
                    panic!("Trying to build sample with value greater than bit depth")
                }
                Self {
                    value: Sample20Bit(value),
                }
            }
            BitDepth::BitDepth24Bit => {
                if value > 2.pow(24) - 1 {
                    panic!("Trying to build sample with value greater than bit depth")
                }
                Self {
                    value: Sample24Bit(value),
                }
            }
            BitDepth::BitDepth32Bit => {
                if value > 2.pow(32) - 1 {
                    panic!("Trying to build sample with value greater than bit depth")
                }
                Self {
                    value: Sample32Bit(value)
                }
            }
        }
    }

    pub fn as_unsigned<T: PrimInt>(&self) -> T {
        match self.value {
            Sample8Bit(value) => { T::from(value).unwrap() }
            Sample16Bit(value) => { T::from(value).unwrap() }
            Sample20Bit(value) => { T::from(value).unwrap() }
            Sample24Bit(value) => { T::from(value).unwrap() }
            Sample32Bit(value) => { T::from(value).unwrap() }
        }
    }
}



pub fn alloc_no_cache_dma_memory(frame_count: u32) -> PhysFrameRange {
    let phys_frame_range = memory::physical::alloc(frame_count as usize);

    let kernel_address_space = process_manager().read().kernel_process().unwrap().address_space();
    let start_page = Page::from_start_address(VirtAddr::new(phys_frame_range.start.start_address().as_u64())).unwrap();
    let end_page = Page::from_start_address(VirtAddr::new(phys_frame_range.end.start_address().as_u64())).unwrap();
    let phys_page_range = PageRange { start: start_page, end: end_page };
    kernel_address_space.set_flags(phys_page_range, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE);

    phys_frame_range
}