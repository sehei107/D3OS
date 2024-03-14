#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt::LowerHex;
use core::ops::BitOr;
use log::{debug, info};
use num_traits::int::PrimInt;
use pci_types::{Bar, BaseClass, CommandRegister, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::interrupt::interrupt_handler::InterruptHandler;
use crate::{apic, interrupt_dispatcher, memory, pci_bus, process_manager, timer};
use crate::device::pit::Timer;
use crate::interrupt::interrupt_dispatcher::InterruptVector;
use crate::memory::{MemorySpace, PAGE_SIZE};

const PCI_MULTIMEDIA_DEVICE:  BaseClass = 4;
const PCI_IHDA_DEVICE:  SubClass = 3;
const MAX_AMOUNT_OF_CODECS: u8 = 15;

pub struct IHDA {
    crs: ControllerRegisterSet,
}

unsafe impl Sync for IHDA {}
unsafe impl Send for IHDA {}

// representation of a IHDA register
struct Register<T> {
    ptr: *mut T,
    name: &'static str,
}

// the following LowerHex type bound is only necessary because of the dump function which displays T as a hex value
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
struct ControllerRegisterSet {
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

struct Command {
    codec_address: u8,
    node_id: u8,
    verb: u16,
    parameter: u8,
}

impl Command {
    fn value(&self) -> u32 {
        (self.codec_address as u32) << 28 | (self.node_id as u32) << 20 | (self.verb as u32) << 8 | self.parameter as u32
    }
}

struct RootNode {
    codec_address: u8,
    node_id: u8,
    function_groups: Vec<FunctionGroupNode>,
}

impl RootNode {
    fn new(codec_address: u8) -> RootNode {
        RootNode {
            codec_address,
            node_id: 0,
            function_groups: FunctionGroupNode::scan(codec_address),
        }
    }
}

struct FunctionGroupNode {
    node_id: u8,
}

impl FunctionGroupNode {
    fn scan(codec_address: u8) -> Vec<FunctionGroupNode> {
        return Vec::new();
    }
}

struct WidgetNode {
    node_id: u8,
}

#[derive(Default)]
struct IHDAInterruptHandler;

impl InterruptHandler for IHDAInterruptHandler {
    fn trigger(&mut self) {
        debug!("INTERRUPT!!!");
    }
}

impl IHDA {
    pub fn new() -> Self {
        let pci = pci_bus();

        // find ihda devices
        let ihda_devices = pci.search_by_class(PCI_MULTIMEDIA_DEVICE, PCI_IHDA_DEVICE);

        if ihda_devices.len() > 0 {
            // first found ihda device gets picked for initialisation under the assumption that there is exactly one ihda sound card available
            let device = ihda_devices[0];
            let bar0 = device.bar(0, pci.config_space()).unwrap();

            match bar0 {
                Bar::Memory32 { address, size, .. } => {
                    let crs = ControllerRegisterSet::new(address);

                    // set BME bit in command register of PCI configuration space
                    device.update_command(pci.config_space(), |command| {
                        command.bitor(CommandRegister::BUS_MASTER_ENABLE)
                    });

                    // set Memory Space bit in command register of PCI configuration space (so that hardware can respond to memory space access)
                    device.update_command(pci.config_space(), |command| {
                        command.bitor(CommandRegister::MEMORY_ENABLE)
                    });

                    // setup MMIO space (currently one-to-one mapping from physical address space to virtual address space of kernel)
                    let pages = size as usize / PAGE_SIZE;
                    let mmio_page = Page::from_start_address(VirtAddr::new(address as u64)).expect("IHDA MMIO address is not page aligned!");
                    let address_space = process_manager().read().kernel_process().unwrap().address_space();
                    address_space.map(PageRange { start: mmio_page, end: mmio_page + 1 }, MemorySpace::Kernel, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE);

                    // setup interrupt line
                    const CPU_EXCEPTION_OFFSET: u8 = 32;
                    let (_, interrupt_line) = device.interrupt(pci.config_space());
                    let interrupt_vector = InterruptVector::try_from(CPU_EXCEPTION_OFFSET + interrupt_line).unwrap();
                    interrupt_dispatcher().assign(interrupt_vector, Box::new(IHDAInterruptHandler::default()));
                    apic().allow(interrupt_vector);
                    // A fake interrupt via the call of "unsafe { asm!("int 43"); }" from the crate core::arch::asm
                    // will now result in a call of IHDAInterruptHandler's "trigger"-function.

                    return Self {
                        crs,
                    }
                },

                _ => { panic!("Invalid BAR! IHDA always uses Memory32") },
            }
        }

        panic!("No IHDA device found!")
    }

    pub fn init(&self) {
        info!("Initializing IHDA sound card");
        self.reset_controller();
        info!("IHDA Controller reset complete");

        self.setup_ihda_config_space();
        info!("IHDA configuration space set up");

        unsafe {
            self.setup_corb();
            self.setup_rirb();
            self.start_corb();
            self.start_rirb();
        }
        info!("CORB and RIRB setup and running");



        let subordinate_node_count_root = Command { codec_address: 0, node_id: 0, verb: 0xF00, parameter: 4 };     // subordinate node count
        let subordinate_node_count_start = Command { codec_address: 0, node_id: 1, verb: 0xF00, parameter: 4 };     // subordinate node count
        let vendor_id = Command { codec_address: 0, node_id: 0, verb: 0xF00, parameter: 0 };    // vendor id

        // send verb via CORB
        unsafe {
            let first_entry = self.crs.corblbase.read() as *mut u32;
            let second_entry = (self.crs.corblbase.read() + 4) as *mut u32;
            let third_entry = (self.crs.corblbase.read() + 8) as *mut u32;
            let fourth_entry = (self.crs.corblbase.read() + 12) as *mut u32;
            // debug!("first_entry: {:#x}, address: {:#x}", first_entry.read(), first_entry as u32);
            // debug!("second_entry: {:#x}, address: {:#x}", second_entry.read(), second_entry as u32);
            // debug!("third_entry: {:#x}, address: {:#x}", third_entry.read(), third_entry as u32);
            // debug!("fourth_entry: {:#x}, address: {:#x}", fourth_entry.read(), fourth_entry as u32);
            // debug!("CORB before: {:#x}, address: {:#x}", (first_entry as *mut u128).read(), first_entry as u32);
            debug!("RIRB before: {:#x}", (self.crs.rirblbase.read() as *mut u128).read());
            // debug!("RIRB before: {:#x}", ((self.crs.rirblbase.read() + 16) as *mut u128).read());
            second_entry.write(vendor_id.value());
            third_entry.write(subordinate_node_count_root.value());
            self.crs.corbwp.write(self.crs.corbwp.read() + 2);
            // self.crs.corbctl.write(self.crs.corbctl.read() | 0b1);
            // debug!("first_entry: {:#x}, address: {:#x}", first_entry.read(), first_entry as u32);
            // debug!("second_entry: {:#x}, address: {:#x}", second_entry.read(), second_entry as u32);
            // debug!("third_entry: {:#x}, address: {:#x}", third_entry.read(), third_entry as u32);
            // debug!("fourth_entry: {:#x}, address: {:#x}", fourth_entry.read(), fourth_entry as u32);
            // debug!("CORB after: {:#x}, address: {:#x}", (first_entry as *mut u128).read(), first_entry as u32);
            // debug!("RIRB after: {:#x}", (self.crs.rirblbase.read() as *mut u128).read());
            // debug!("RIRB after: {:#x}", ((self.crs.rirblbase.read() + 16) as *mut u128).read());
        }

        unsafe {
            self.crs.gctl.dump();
            self.crs.intctl.dump();
            self.crs.wakeen.dump();
            self.crs.corbctl.dump();
            self.crs.rirbctl.dump();
            self.crs.corbsize.dump();
            self.crs.rirbsize.dump();
            self.crs.corbsts.dump();

            self.crs.corbwp.dump();
            // expect the CORBRP to be equal to CORBWP if sending commands was successful
            self.crs.corbrp.dump();
            self.crs.corblbase.dump();
            self.crs.corbubase.dump();

            self.crs.rirbwp.dump();
            self.crs.rirblbase.dump();
            self.crs.rirbubase.dump();

            self.crs.walclk.dump();
            debug!("RIRB after: {:#x}", (self.crs.rirblbase.read() as *mut u128).read());
            debug!("RIRB after (next entries): {:#x}", ((self.crs.rirblbase.read() + 16) as *mut u128).read());
        }

        // send command via Immediate Command Registers

        unsafe {
            debug!("subordinate_node_count of root node: {:#x}", self.crs.immediate_command(subordinate_node_count_root));
            debug!("subordinate_node_count of starting node: {:#x}", self.crs.immediate_command(subordinate_node_count_start));
            debug!("vendor_id: {:#x}", self.crs.immediate_command(vendor_id));
        }

        /* potential ways to write to a buffer (don't compile yet)

        let wav = include_bytes!("test.wav");
        let phys_addr = current_process().address_space().translate(VirtAddr::new(wav.as_ptr() as u64)).unwrap();

        let audio_buffer = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80];
        let phys_addr = current_process().address_space().translate(VirtAddr::new(audio_buffer.as_ptr() as u64)).unwrap();

        */
        unsafe{
            let codecs = self.scan();
            for codec in codecs {

            }
        }

        // wait two minutes so you can read the previous prints on real hardware where you can't set breakpoints with a debugger
        Timer::wait(120000);

    }

    fn reset_controller(&self) {
        unsafe {
            // set controller reset bit (CRST)
            self.crs.gctl.set_bit(0);
            let start_timer = timer().read().systime_ms();
            // value for CRST_TIMEOUT arbitrarily chosen
            const CRST_TIMEOUT: usize = 100;
            while !self.crs.gctl.assert_bit(0) {
                if timer().read().systime_ms() > start_timer + CRST_TIMEOUT {
                    panic!("IHDA controller reset timed out")
                }
            }

            // according to IHDA specification (section 4.3 Codec Discovery), the system should at least wait .521 ms after reading CRST as 1, so that the codecs have time to self-initialize
            Timer::wait(1);
        }
    }

    fn setup_ihda_config_space(&self) {
        // set Accept Unsolicited Response Enable (UNSOL) bit
        unsafe {
            self.crs.gctl.set_bit(8);
        }

        // set global interrupt enable (GIE) and controller interrupt enable (CIE) bits
        unsafe {
            self.crs.intctl.set_bit(30);
            self.crs.intctl.set_bit(31);
        }

        // enable wake events and interrupts for all SDIN (actually, only one bit needs to be set, but this works for now...)
        unsafe {
            self.crs.wakeen.set_all_bits();
        }
    }

    fn setup_corb(&self) {
        // disable CORB DMA engine (CORBRUN) and CORB memory error interrupt (CMEIE)
        unsafe {
            self.crs.corbctl.clear_all_bits();
        }

        // verify that CORB size is 1KB (IHDA specification, section 3.3.24: "There is no requirement to support more than one CORB Size.")
        let corbsize;
        unsafe {
            corbsize = self.crs.corbsize.read() & 0b11;
        }
        assert_eq!(corbsize, 0b10);

        // setup MMIO space for Command Outbound Ring Buffer – CORB
        let corb_frame_range = memory::physical::alloc(1);
        match corb_frame_range {
            PhysFrameRange { start, end: _ } => {
                let start_address = start.start_address().as_u64();
                let lbase = (start_address & 0xFFFFFFFF) as u32;
                let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;
                unsafe {
                    self.crs.corblbase.write(lbase);
                    self.crs.corbubase.write(ubase);
                }
            }
        }
    }

    fn reset_corb(&self) {
        // clear CORBWP
        unsafe {
            self.crs.corbwp.clear_all_bits();
        }

        //reset CORBRP
        unsafe {
            self.crs.corbrp.set_bit(15);
            let start_timer = timer().read().systime_ms();
            // value for CORBRPRST_TIMEOUT arbitrarily chosen
            const CORBRPRST_TIMEOUT: usize = 10000;
            while self.crs.corbrp.read() != 0x0 {
                if timer().read().systime_ms() > start_timer + CORBRPRST_TIMEOUT {
                    panic!("CORB read pointer reset timed out")
                }
            }
            // on my testing device with a physical IHDA sound card, the CORBRP reset doesn't work like described in the specification (section 3.3.21)
            // actually you are supposed to do something like this:

            // while !self.crs.corbrp.assert_bit(15) {
            //     if timer().read().systime_ms() > start_timer + CORBRPRST_TIMEOUT {
            //         panic!("CORB read pointer reset timed out")
            //     }
            // }
            // self.crs.corbrp.clear_all_bits();
            // while self.crs.corbrp.assert_bit(15) {
            //     if timer().read().systime_ms() > start_timer + CORBRPRST_TIMEOUT {
            //         panic!("CORB read pointer clear timed out")
            //     }
            // }

            // but the physical sound card never writes a 1 back to the CORBRPRST bit so that the code always panicked with "CORB read pointer reset timed out"
            // on the other hand, setting the CORBRPRST bit successfully set the CORBRP register back to 0
            // this is why the code now just checks if the register contains the value 0 after the reset
            // it is still to figure out if the controller really clears "any residual pre-fetched commands in the CORB hardware buffer within the controller" (section 3.3.21)
        }
    }

    fn setup_rirb(&self) {
        // disable RIRB response overrun interrupt control (RIRBOIC), RIRB DMA engine (RIRBDMAEN) and RIRB response interrupt control (RINTCTL)
        unsafe {
            self.crs.rirbctl.clear_all_bits();
        }

        // setup MMIO space for Response Inbound Ring Buffer – RIRB
        let rirb_frame_range = memory::physical::alloc(1);
        match rirb_frame_range {
            PhysFrameRange { start, end: _ } => {
                let start_address = start.start_address().as_u64();
                let lbase = (start_address & 0xFFFFFFFF) as u32;
                let ubase = ((start_address & 0xFFFFFFFF_00000000) >> 32) as u32;
                unsafe {
                    self.crs.rirblbase.write(lbase);
                    self.crs.rirbubase.write(ubase);
                }
            }
        }

        // clear first CORB-entry (might not be necessary)
        unsafe {
            (self.crs.corblbase.read() as *mut u32).write(0x0);
        }

        // reset RIRBWP
        unsafe {
            self.crs.rirbwp.set_bit(15);
        }
    }


    fn start_corb(&self) {
        unsafe {
            // set CORBRUN and CMEIE bits
            self.crs.corbctl.set_bit(0);
            self.crs.corbctl.set_bit(1);

        }
    }
    fn start_rirb(&self) {
        unsafe {
            // set RIRBOIC, RIRBDMAEN  und RINTCTL bits
            self.crs.rirbctl.set_bit(0);
            self.crs.rirbctl.set_bit(1);
            self.crs.rirbctl.set_bit(2);
        }
    }

    // check the bitmask from bits 0 to 14 of the WAKESTS (in the specification also called STATESTS) indicating available codecs
    unsafe fn scan(&self) -> Vec<RootNode> {
        let mut codecs: Vec<RootNode> = Vec::new();
        for index in 0..MAX_AMOUNT_OF_CODECS {
            if self.crs.wakests.assert_bit(index) {
                codecs.push(RootNode::new(index));
            }
        }
        codecs
    }
}
