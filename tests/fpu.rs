//! Architectural x87 assertions for behavior absent from the Unicorn oracle.
use wire::cpu::{Cpu, CpuError, AF, CF, OF, PF, SF, ZF};
use wire::mem::GuestMem;
const CODE: u32 = 0x10000;
const DATA: u32 = 0x20000;
const ONE: u128 = 0x3fff_8000_0000_0000_0000;
const INDEFINITE: u128 = 0xffff_c000_0000_0000_0000;
struct Vm {
    cpu: Cpu,
    mem: GuestMem,
}
impl Vm {
    fn new() -> Self {
        let mut mem = GuestMem::new().unwrap();
        mem.commit(CODE, 0x10000).unwrap();
        mem.commit(DATA, 0x10000).unwrap();
        Self {
            cpu: Cpu::new(CODE, 0x3fff0, 0),
            mem,
        }
    }
    fn run(&mut self, code: &[u8]) -> Result<(), CpuError> {
        self.mem.write_bytes(CODE, code).unwrap();
        self.cpu.eip = CODE;
        while self.cpu.eip != CODE + code.len() as u32 {
            self.cpu.step(&mut self.mem)?;
        }
        Ok(())
    }
    fn load(&mut self, bits: u128) {
        self.mem
            .write_bytes(DATA, &bits.to_le_bytes()[..10])
            .unwrap();
        self.run(&[0xdb, 0x2d, 0, 0, 2, 0]).unwrap();
    }
}
#[test]
fn x87_precision_exceptions_condition_bits_and_unmasked_wait() {
    let mut v = Vm::new();
    v.load(ONE);
    v.load(((16383u128 - 64) << 64) | (1 << 63)); // 2^-64: halfway at extended precision
    v.run(&[0xde, 0xc1]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), ONE);
    assert_eq!(v.cpu.fpu.sw & 0x220, 0x20);
    v.run(&[0xdb, 0xe2]).unwrap();
    assert_eq!(v.cpu.fpu.sw & 0x80ff, 0);
    assert_eq!(v.cpu.fpu.top(), 7); // clex preserves TOP
    v.cpu.fpu.cw = 0xb7f; // round up
    v.load(((16383u128 - 64) << 64) | (1 << 63));
    v.run(&[0xde, 0xc1]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), ONE + 1);
    assert_eq!(v.cpu.fpu.sw & 0x220, 0x220);
    v.run(&[0xdb, 0xe3]).unwrap();
    v.load(0);
    v.load(ONE);
    v.run(&[0xde, 0xf9]).unwrap(); // 0/1: exact zero
    assert_eq!(v.cpu.fpu.sw & 0x3f, 0);
    v.load(ONE);
    v.run(&[0xde, 0xf1]).unwrap(); // 1/0
    assert_eq!(v.cpu.fpu.st_bits(0), 0x7fff_8000_0000_0000_0000);
    assert_eq!(v.cpu.fpu.sw & 4, 4);
    // Unmask the already pending divide-by-zero; FLDCW itself completes.
    v.mem.write_u16(DATA, 0x37b).unwrap();
    v.run(&[0xd9, 0x2d, 0, 0, 2, 0]).unwrap();
    assert!(matches!(
        v.run(&[0x9b]),
        Err(CpuError::FloatingPoint { .. })
    ));
    v.run(&[0xdb, 0xe2, 0x9b]).unwrap();
    // FCOMI must clear OF, SF and AF, including when previously set.
    v.cpu.fpu.cw = 0x37f;
    v.run(&[0xdb, 0xe3, 0xd9, 0xe8, 0xd9, 0xe8]).unwrap();
    v.cpu.eflags |= OF | SF | AF | PF | CF;
    v.run(&[0xdb, 0xf1]).unwrap();
    assert_eq!(v.cpu.eflags & (OF | SF | AF | ZF | PF | CF), ZF);
    // Ordered qNaN comparison raises invalid; quiet comparison does not.
    v.load(0x7fff_c000_0000_0000_0000);
    v.run(&[0xd8, 0xd1]).unwrap();
    assert_eq!(v.cpu.fpu.sw & 0x4501, 0x4501);
    v.run(&[0xdb, 0xe2, 0xdd, 0xe1]).unwrap();
    assert_eq!(v.cpu.fpu.sw & 1, 0);
}
#[test]
fn x87_masked_stack_faults_and_denormals() {
    let mut v = Vm::new();
    for _ in 0..8 {
        v.run(&[0xd9, 0xe8]).unwrap();
    }
    v.run(&[0xd9, 0xe8]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), INDEFINITE);
    assert_eq!(v.cpu.fpu.sw & 0x241, 0x241);
    for _ in 0..8 {
        v.run(&[0xdd, 0xd8]).unwrap();
    }
    v.run(&[0xdb, 0x3d, 0, 0, 2, 0]).unwrap();
    let mut out = [0; 16];
    v.mem.read_bytes(DATA, &mut out[..10]).unwrap();
    assert_eq!(u128::from_le_bytes(out), INDEFINITE);
    assert_eq!(v.cpu.fpu.sw & 0x241, 0x41);
    v.run(&[0xdb, 0xe3]).unwrap();
    v.mem.write_u64(DATA, 1).unwrap();
    v.run(&[0xdd, 0x05, 0, 0, 2, 0]).unwrap();
    assert_eq!(v.cpu.fpu.sw & 2, 2);
    // Negative operand to sqrt returns masked indefinite and sets invalid.
    v.run(&[0xdb, 0xe3]).unwrap();
    v.load(ONE | (1 << 79));
    v.run(&[0xd9, 0xfa]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), INDEFINITE);
    assert_eq!(v.cpu.fpu.sw & 1, 1);
}
#[test]
fn x87_software_math_known_extended_results() {
    let mut v = Vm::new();
    v.load(0x4000_8000_0000_0000_0000); // 2
    v.run(&[0xd9, 0xfa]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), 0x3fff_b504_f333_f9de_6484);
    assert_eq!(v.cpu.fpu.sw & 0x20, 0x20);
    v.run(&[0xdb, 0xe3, 0xd9, 0xe8]).unwrap();
    v.load(0x4000_c000_0000_0000_0000); // log2(3)
    v.run(&[0xd9, 0xf1]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), 0x3fff_cae0_0d1c_fdeb_43d0);
    v.run(&[0xdb, 0xe3]).unwrap();
    v.load(0x3ffe_8000_0000_0000_0000); // 2^0.5 - 1
    v.run(&[0xd9, 0xf0]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), 0x3ffd_d413_cccf_e779_9211);
    v.run(&[0xdb, 0xe3]).unwrap();
    v.load(1 << 79);
    v.run(&[0xd9, 0xf0]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), 1 << 79); // negative zero
                                               // PC=24 does not reduce the exponent range to float32's range.
    v.run(&[0xdb, 0xe3]).unwrap();
    v.cpu.fpu.cw = 0x7f;
    v.load(((16383u128 + 1000) << 64) | (1 << 63));
    v.load(ONE);
    v.run(&[0xde, 0xc9]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), ((16383u128 + 1000) << 64) | (1 << 63));
}
#[test]
fn x87_save_restore_environment_and_full_stack() {
    let mut v = Vm::new();
    v.run(&[0xd9, 0xe8, 0xd9, 0xeb]).unwrap();
    v.cpu.fpu.cw = 0xb7f;
    let before = v.cpu.fpu;
    v.run(&[0xdd, 0x35, 0, 0, 2, 0]).unwrap(); // fnsave 108 bytes
    assert_eq!(v.cpu.fpu.tags, 0xffff);
    assert_eq!(v.cpu.fpu.cw, 0x37f);
    assert_eq!(v.mem.read_u16(DATA + 8).unwrap(), before.tags);
    assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), before.ip);
    v.run(&[0xdd, 0x25, 0, 0, 2, 0]).unwrap();
    assert_eq!(v.cpu.fpu.regs, before.regs);
    assert_eq!(v.cpu.fpu.tags, before.tags);
    assert_eq!(v.cpu.fpu.cw, before.cw);
    assert_eq!(v.cpu.fpu.sw, before.sw);
    // FNSTENV masks exceptions but preserves its stored control word.
    v.run(&[0xd9, 0x35, 0, 0, 2, 0]).unwrap();
    assert_eq!(v.mem.read_u16(DATA).unwrap(), before.cw);
}

#[test]
fn x87_logarithms_keep_precision_near_one_and_for_tiny_increments() {
    use rustc_apfloat::ieee::X87DoubleExtended;
    use rustc_apfloat::{Float, Round};
    // Decimal references evaluated independently at 125 decimal digits.
    let mut v = Vm::new();
    v.run(&[0xd9, 0xe8]).unwrap();
    v.load(0x3ffe_ffff_ffff_ffff_ffff);
    v.run(&[0xd9, 0xf1]).unwrap();
    let expected = X87DoubleExtended::from_str_r(
        "-7.8208654878293888820135163822727787838823864674840411749616376377419e-20",
        Round::NearestTiesToEven,
    )
    .unwrap()
    .value;
    assert_eq!(v.cpu.fpu.st_bits(0), expected.to_bits());
    v.run(&[0xdb, 0xe3, 0xd9, 0xe8]).unwrap();
    v.load(((16383u128 - 200) << 64) | (1 << 63));
    v.run(&[0xd9, 0xf9]).unwrap();
    let expected = X87DoubleExtended::from_str_r(
        "8.9779132807465238150941827694775345049113872635800534227892744983177e-61",
        Round::NearestTiesToEven,
    )
    .unwrap()
    .value;
    assert_eq!(v.cpu.fpu.st_bits(0), expected.to_bits());
    v.run(&[0xdb, 0xe3]).unwrap();
    v.load(0x7fff_8000_0000_0000_0042);
    v.run(&[0xd9, 0xfa]).unwrap();
    assert_eq!(v.cpu.fpu.st_bits(0), 0x7fff_c000_0000_0000_0042);
    assert_eq!(v.cpu.fpu.sw & 1, 1);
}
