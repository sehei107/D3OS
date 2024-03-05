#![allow(dead_code)]

use alloc::boxed::Box;
use core::fmt::LowerHex;
use core::ops::BitOr;
use log::debug;
use pci_types::{Bar, BaseClass, CommandRegister, SubClass};
use x86_64::structures::paging::{Page, PageTableFlags};
use x86_64::structures::paging::page::PageRange;
use x86_64::VirtAddr;
use crate::interrupt::interrupt_handler::InterruptHandler;
use crate::{apic, interrupt_dispatcher, pci_bus};
use crate::device::pit::Timer;
use crate::interrupt::interrupt_dispatcher::InterruptVector;
use crate::memory::{MemorySpace, PAGE_SIZE};
use crate::process::process::current_process;

const PCI_MULTIMEDIA_DEVICE:  BaseClass = 4;
const PCI_IHDA_DEVICE:  SubClass = 3;

pub struct IHDA {
    mmio_address: u32,
}

// representation of a IHDA register
struct Register<T> {
    ptr: *mut T,
    name: &'static str,
}

// the following LowerHex type bound is only necessary because of the dump function which displays T as a hex value
impl<T: LowerHex> Register<T> {
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

                    // setup MMIO space (currently one-to-one mapping from physical address space to virtual address space of kernel)
                    let pages = size as usize / PAGE_SIZE;
                    let mmio_page = Page::from_start_address(VirtAddr::new(address as u64)).expect("IHDA MMIO address is not page aligned!");
                    let address_space = current_process().address_space();
                    address_space.map(PageRange { start: mmio_page, end: mmio_page + pages as u64 }, MemorySpace::Kernel, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE);

                    // setup interrupt line
                    const CPU_EXCEPTION_OFFSET: u8 = 32;
                    let (_, interrupt_line) = device.interrupt(pci.config_space());
                    let interrupt_vector = InterruptVector::try_from(CPU_EXCEPTION_OFFSET + interrupt_line).unwrap();
                    interrupt_dispatcher().assign(interrupt_vector, Box::new(IHDAInterruptHandler::default()));
                    apic().allow(interrupt_vector);
                    // A fake interrupt via the call of "unsafe { asm!("int 43"); }" from the crate core::arch::asm
                    // will now result in a call of IHDAInterruptHandler's "trigger"-function.

                    // set controller reset bit (CRST)
                    unsafe {
                        crs.gctl.write(crs.gctl.read() | 0x00000001);
                        let mut crst_timer: u8 = 0;
                        // value for CRST_TIMEOUT arbitrarily chosen
                        const CRST_TIMEOUT: u8 = 100;
                        while (crs.gctl.read() & 0x00000001) != 1 {
                            crst_timer += 1;
                            if crst_timer > CRST_TIMEOUT {
                                panic!("IHDA controller reset timed out")
                            }
                        }
                    }
                    // according to IHDA specification (section 4.3 Codec Discovery), the system should at least wait .521 ms after reading CRST as 1, so that the codecs have time to self-initialize
                    Timer::wait(1);

                    // set global interrupt enable (GIE) and controller interrupt enable (CIE) bits
                    unsafe {
                        crs.intctl.write(crs.intctl.read() | 0xC0000000);
                        assert_eq!(crs.intctl.read() & 0xC0000000, 0xC0000000);
                    }

                    // send command via Immediate Command Registers
                    let verb = Command { codec_address: 0, node_id: 0, verb: 0xF00, parameter: 4 }.value();     // subordinate node count
                    let verb1 = Command { codec_address: 0, node_id: 0, verb: 0xF00, parameter: 0 }.value();    // vendor id

                    unsafe {
                        crs.icis.write(0b10);
                        crs.icoi.write(verb);
                        crs.icis.write(0b1);
                        assert_eq!(crs.icis.read() & 0b10, 0b10);
                        crs.icii.dump();
                        crs.icis.write(0b10);
                        crs.icoi.write(verb1);
                        crs.icis.write(0b1);
                        assert_eq!(crs.icis.read() & 0b10, 0b10);
                        crs.icii.dump();
                    }

                    /* potential ways to write to a buffer (don't compile yet)

                    let wav = include_bytes!("test.wav");
                    let phys_addr = current_process().address_space().translate(VirtAddr::new(wav.as_ptr() as u64)).unwrap();

                    let audio_buffer = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80];
                    let phys_addr = current_process().address_space().translate(VirtAddr::new(audio_buffer.as_ptr() as u64)).unwrap();

                    */

                    return Self {
                        mmio_address: address,
                    }
                },

                _ => { panic!("Invalid BAR! IHDA always uses Memory32") },
            }
        }

        panic!("No IHDA device found!")
    }
}
