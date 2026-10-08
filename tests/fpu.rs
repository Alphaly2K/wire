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
#[test]
fn mmx_aliases_physical_x87_registers_and_faults_preserve_state() {
    let mut v = Vm::new();
    v.cpu.fpu.sw = 5 << 11;
    v.cpu.fpu.regs[0] = ONE | 0x1234;
    v.cpu.fpu.regs[1] = ONE | 0x55;
    let flags = v.cpu.eflags;
    v.run(&[0x0f, 0xef, 0xc1]).unwrap();
    assert_eq!(v.cpu.fpu.regs[0], (0xffffu128 << 64) | 0x1261);
    assert_eq!(v.cpu.fpu.tags, 0);
    assert_eq!(v.cpu.fpu.top(), 0);
    assert_eq!(v.cpu.eflags, flags);
    let payloads = v.cpu.fpu.regs;
    v.run(&[0x0f, 0x77]).unwrap();
    assert_eq!(v.cpu.fpu.tags, 0xffff);
    assert_eq!(v.cpu.fpu.regs, payloads);
    v.cpu.fpu.sw = 3 << 11;
    let before = v.cpu.fpu;
    v.cpu.regs[3] = 0x30000;
    assert!(v.run(&[0x0f, 0xef, 0x03]).is_err());
    assert_eq!(v.cpu.fpu.regs, before.regs);
    assert_eq!(v.cpu.fpu.tags, before.tags);
    assert_eq!(v.cpu.fpu.sw, before.sw);
    // PUNPCKLBW reads four bytes at a page end, MOVQ still reads eight.
    v.cpu.regs[3] = DATA + 0xfffc;
    v.mem.write_u32(DATA + 0xfffc, 0x04030201).unwrap();
    v.run(&[0x0f, 0x60, 0x03]).unwrap();
    assert!(v.run(&[0x0f, 0x6f, 0x03]).is_err());
}

#[test]
fn x87_trigonometry_preserves_extended_precision_stack_and_range() {
    // Independent 150-digit Decimal Taylor/reduction references, rounded to
    // extended precision. The large argument exercises split-pi reduction.
    for (x, sin, cos) in [
        (ONE, 0x3ffe_d76a_a478_4867_7021, 0x3ffe_8a51_407d_a834_5c92),
        (
            0x403d_8000_0000_0000_0001,
            0xbffe_f537_162c_eea4_dddd,
            0xbffd_930b_5e4d_26d6_9ce0,
        ),
    ] {
        for pc in [0x7f, 0x27f, 0x37f] {
            let mut v = Vm::new();
            v.cpu.fpu.cw = pc;
            v.load(x);
            v.run(&[0xd9, 0xfb]).unwrap();
            assert_eq!(v.cpu.fpu.top(), 6);
            assert_eq!(v.cpu.fpu.st_bits(0), cos);
            assert_eq!(v.cpu.fpu.st_bits(1), sin);
            assert_eq!(v.cpu.fpu.sw & 0x420, 0x20);
        }
    }
    for x in [0, 1 << 79] {
        let mut v = Vm::new();
        v.load(x);
        v.run(&[0xd9, 0xfb]).unwrap();
        assert_eq!(v.cpu.fpu.st_bits(0), ONE);
        assert_eq!(v.cpu.fpu.st_bits(1), x);
        assert_eq!(v.cpu.fpu.sw & 0x3f, 0);
    }
    for op in [0xfb, 0xfe, 0xff] {
        let mut v = Vm::new();
        let x = 0x403e_8000_0000_0000_0000; // 2^63, out of range
        v.load(x);
        v.run(&[0xd9, op]).unwrap();
        assert_eq!(v.cpu.fpu.top(), 7);
        assert_eq!(v.cpu.fpu.st_bits(0), x);
        assert_eq!(v.cpu.fpu.sw & 0x43f, 0x400);
    }
}
#[test]
fn x87_trigonometry_tiny_directed_rounding_and_invalid_operands() {
    use rustc_apfloat::{ieee::X87DoubleExtended, Float};
    let tiny = ((16383u128 - 100) << 64) | (1 << 63);
    for sign in [0, 1 << 79] {
        for rc in 0..4 {
            let x = tiny | sign;
            let toward = rc == 3 || (rc == 1 && sign == 0) || (rc == 2 && sign != 0);
            let expected = if toward {
                let f = X87DoubleExtended::from_bits(x);
                if sign == 0 {
                    f.next_down().value
                } else {
                    f.next_up().value
                }
                .to_bits()
            } else {
                x
            };
            let mut v = Vm::new();
            v.cpu.fpu.cw = 0x37f | (rc << 10);
            v.load(x);
            v.run(&[0xd9, 0xfb]).unwrap();
            assert_eq!(v.cpu.fpu.st_bits(1), expected);
            assert_eq!(
                v.cpu.fpu.st_bits(0),
                if rc == 1 || rc == 3 {
                    0x3ffe_ffff_ffff_ffff_ffff
                } else {
                    ONE
                }
            );
        }
    }
    for (x, result, invalid) in [
        (0x7fff_8000_0000_0000_0000, INDEFINITE, 1),
        (0x7fff_c000_0000_0000_0011, 0x7fff_c000_0000_0000_0011, 0),
    ] {
        let mut v = Vm::new();
        v.load(x);
        v.run(&[0xd9, 0xfb]).unwrap();
        assert_eq!(v.cpu.fpu.st_bits(0), result);
        assert_eq!(v.cpu.fpu.st_bits(1), result);
        assert_eq!(v.cpu.fpu.sw & 1, invalid);
    }
    let mut v = Vm::new();
    for _ in 0..8 {
        v.load(ONE);
    }
    v.run(&[0xd9, 0xfb]).unwrap();
    assert_eq!(v.cpu.fpu.sw & 0x241, 0x241);
    assert_eq!(v.cpu.fpu.st_bits(0), INDEFINITE);
    assert_eq!(v.cpu.fpu.st_bits(1), INDEFINITE);
    let mut v = Vm::new();
    v.load(0x7fff_8000_0000_0000_0000);
    v.cpu.fpu.cw &= !1;
    v.run(&[0xd9, 0xfb]).unwrap();
    assert_eq!(v.cpu.fpu.top(), 7);
    assert_eq!(v.cpu.fpu.st_bits(0), 0x7fff_8000_0000_0000_0000);
    assert!(matches!(
        v.run(&[0x9b]),
        Err(CpuError::FloatingPoint { .. })
    ));
}
#[test]
fn sse_compare_predicates_nan_daz_exceptions_and_scalar_upper_lanes() {
    for predicate in 0..8u8 {
        let mut v = Vm::new();
        v.cpu.xmm[0][..8].copy_from_slice(&0x7ff8_0000_0000_0001u64.to_le_bytes());
        v.cpu.xmm[0][8..].fill(0x5a);
        v.cpu.xmm[1][..8].copy_from_slice(&1f64.to_le_bytes());
        let flags = v.cpu.eflags;
        v.run(&[0xf2, 0x0f, 0xc2, 0xc1, predicate]).unwrap();
        assert_eq!(
            v.cpu.xmm[0][..8],
            if matches!(predicate, 3..=6) {
                [255; 8]
            } else {
                [0; 8]
            }
        );
        assert_eq!(v.cpu.xmm[0][8..], [0x5a; 8]);
        assert_eq!(v.cpu.eflags, flags);
        assert_eq!(v.cpu.mxcsr & 1, matches!(predicate, 1 | 2 | 5 | 6) as u32);
    }
    let mut v = Vm::new();
    v.cpu.xmm[0][..8].copy_from_slice(&1u64.to_le_bytes());
    v.cpu.mxcsr |= 0x40; // DAZ: the smallest subnormal compares equal to zero
    v.run(&[0xf2, 0x0f, 0xc2, 0xc1, 0]).unwrap();
    assert_eq!(v.cpu.xmm[0][..8], [255; 8]);
    assert_eq!(v.cpu.mxcsr & 3, 0);
    let mut v = Vm::new();
    v.cpu.xmm[0][..8].copy_from_slice(&1u64.to_le_bytes());
    v.run(&[0xf2, 0x0f, 0xc2, 0xc1, 0]).unwrap();
    assert_eq!(v.cpu.xmm[0][..8], [0; 8]);
    assert_eq!(v.cpu.mxcsr & 3, 2);
    let mut v = Vm::new();
    v.cpu.xmm[0][..8].copy_from_slice(&0x7ff0_0000_0000_0001u64.to_le_bytes());
    let before = v.cpu.xmm[0];
    v.cpu.mxcsr &= !0x80;
    assert!(matches!(
        v.run(&[0xf2, 0x0f, 0xc2, 0xc1, 0]),
        Err(CpuError::SimdFloatingPoint { .. })
    ));
    assert_eq!(v.cpu.xmm[0], before);
    assert_eq!(v.cpu.mxcsr & 1, 1);
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
    // FLDPI must follow the current RC even after another RC used the cache.
    for (rc, significand) in [
        (0, 0xc90f_daa2_2168_c235u128),
        (1, 0xc90f_daa2_2168_c234),
        (2, 0xc90f_daa2_2168_c235),
        (3, 0xc90f_daa2_2168_c234),
        (0, 0xc90f_daa2_2168_c235),
    ] {
        v.run(&[0xdb, 0xe3]).unwrap();
        v.cpu.fpu.cw = 0x37f | (rc << 10);
        v.run(&[0xd9, 0xeb]).unwrap();
        assert_eq!(v.cpu.fpu.st_bits(0), (0x4000u128 << 64) | significand);
        assert_eq!(v.cpu.fpu.sw & 0x3f, 0); // constants do not raise inexact
    }
    v.run(&[0xdb, 0xe3]).unwrap();
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
    assert_eq!(v.mem.read_u32(DATA + 12).unwrap() as u64, before.ip);
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
